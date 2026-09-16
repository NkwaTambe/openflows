// crates/agent-nexus/src/hooks/guard.rs
//! Slice C — stateful, phase-aware `pre_tool_use` write/bash guard.
//!
//! On top of the generic `apply_policy()` (rm -rf, force-push, redis-cli, ...),
//! this adds per-role, phase-aware decisions for the **complete** Forge
//! lifecycle. The harness phases are authoritative:
//! `planning → building → testing → review_ready`, with `blocked` as the
//! failure escape hatch. There is no separate `pr_ready` phase: opening a PR is
//! the artifact produced *inside* `review_ready`.
//!
//! The guard asks one question of every tool call — "is the agent doing exactly
//! what its current phase says it should?":
//!   - `planning`     — first/next write must be the plan (PLAN.md / `plan write`).
//!   - `building`     — implementation; **the plan MUST already exist**. If a
//!     `building` ticket has no plan, the agent is out of sequence and is told to
//!     run `/plan` first (deny, relayed to the model via `post_tool_use`).
//!   - `testing`      — verify; plan must exist, source fixes permitted.
//!   - `review_ready` — terminal; source writes denied, rework re-enters earlier.
//!   - `blocked`      — only blocker report + read-only probes.
//!   - `sentinel`     — read-only reviewer; all writes denied.
//!
//! The guard only reads Redis state — it never persists. Its denial reasons are
//! also **guidelines** that `post_tool_use` collects and feeds back to the model.

use super::context::{read_ticket_state, resolve_chat};
use crate::hooks::types::HookDecision;
use pocketflow_core::SharedStore;
use serde_json::Value;

/// The phases of a Forge worker's lifecycle, in order.
const PHASES: &[&str] = &["planning", "building", "testing", "review_ready", "blocked"];

/// True iff `phase` is a recognised Forge lifecycle phase.
pub fn is_valid_phase(phase: &str) -> bool {
    PHASES.contains(&phase)
}

/// Is the tool a write-ish tool?
fn is_write_tool(name: &str) -> bool {
    matches!(
        name,
        "write" | "edit" | "create" | "patch" | "Write" | "Edit" | "Create" | "Patch"
    )
}

/// Is the tool the harness coordination CLI (status/plan/gate/pr/handoff)?
fn is_harness(name: &str) -> bool {
    name.to_lowercase().contains("harness")
}

/// Is the tool a shell (Bash/exec) call?
fn is_shell(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "bash" | "sh" | "shell" | "execute" | "exec"
    )
}

fn shell_command_writes(command: &str) -> bool {
    match shell_write_targets(command) {
        Some(targets) => !targets.is_empty(),
        None => true,
    }
}

fn shell_writes_source(command: &str) -> bool {
    match shell_write_targets(command) {
        Some(targets) => targets
            .iter()
            .any(|target| !is_allowed_lifecycle_artifact(target)),
        None => shell_command_writes(command),
    }
}

fn shell_write_targets(command: &str) -> Option<Vec<String>> {
    let words = shell_words(command);
    if words.is_empty() {
        return Some(Vec::new());
    }

    let mut targets = redirection_targets(&words);
    targets.extend(tee_targets(&words));
    targets.extend(command_operand_targets(&words));
    if !targets.is_empty() {
        return Some(targets);
    }

    let cmd = command.to_lowercase();
    if cmd.contains("python ")
        || cmd.contains("python3 ")
        || cmd.contains("node ")
        || cmd.contains("cargo fmt")
        || cmd.contains("git apply")
        || cmd.contains("apply_patch")
        || cmd.contains("sed -i")
        || cmd.contains("perl -i")
    {
        return None;
    }

    // Unknown-but-write-capable commands (dd of=, curl -o, wget -O, git
    // checkout/restore/..., sh -c ...) have filesystem effects the operand
    // parser does not capture. Fail closed: treat them as potential writes in
    // write-restricted phases rather than assuming they are read-only.
    if is_write_capable_command(command) {
        return None;
    }

    Some(Vec::new())
}

/// The known forwarding wrappers that merely pass control to the real command.
const FORWARDING_WRAPPERS: &[&str] = &["nohup", "nice", "sudo", "env", "setsid", "time", "command"];

/// Wrapper options that consume a *separate* argument token (`sudo -u root`).
/// These must be skipped together with their value so the value is not mistaken
/// for the executable.
const WRAPPER_OPT_WITH_ARG: &[&str] = &[
    "-u",
    "--user",
    "-g",
    "--group",
    "-C",
    "--chroot",
    "-p",
    "--prompt",
    "-D",
    "--chdir",
    "-n",
    "--adjustment",
    "-S",
    "--set-home",
    "-o",
    "--output",
];

/// Is `word` a wrapper option (leading dash, not `-` alone)?
fn is_option_token(word: &str) -> bool {
    word.len() > 1 && word.starts_with('-')
}

/// Is `word` an environment assignment (`VAR=value`)?
fn is_env_assignment(word: &str) -> bool {
    let Some(eq) = word.find('=') else {
        return false;
    };
    eq > 0 && !word[..eq].contains(['-', '/'])
}

/// Resolve the index of the real executable after a forwarding-wrapper prefix.
///
/// A wrapper is not simply "the first token that is not a wrapper name":
/// wrapper *arguments* (`sudo -u root`, `env SOME=1 git`, `nice -n 10 git`)
/// would otherwise be mistaken for the command and let a write-capable
/// subcommand (e.g. `git checkout`) slip past detection (P1: "Wrapper Arguments
/// Hide Writes"). We skip wrapper names, their value-consuming options and the
/// option values, leading `--opt=value` options, and `VAR=value` assignments
/// before settling on the executable.
fn resolve_executable_index(segment: &[String]) -> usize {
    let mut i = 0;
    while i < segment.len() {
        let w = segment[i].as_str();
        if FORWARDING_WRAPPERS.contains(&w) {
            i += 1;
            continue;
        }
        if WRAPPER_OPT_WITH_ARG.contains(&w) {
            i += 2; // skip the option and its separate value token
            continue;
        }
        if is_option_token(w) || is_env_assignment(w) {
            i += 1;
            continue;
        }
        return i;
    }
    segment.len().saturating_sub(1)
}

/// Can one command *segment* (a portion separated by `&&`/`||`/`;`/`|`) write a
/// file in a way the redirection / operand parsers do not capture?
///
/// Wrappers (`sudo`, `nohup`, `env`, ...) that merely forward to the real
/// command are stripped first — including their arguments/options — and the
/// subcommand of a compound command (e.g. `git checkout`) is resolved *relative
/// to the executable that was actually found*. Reading the subcommand from a
/// fixed token would break under a wrapper (`sudo -u root git checkout` or
/// `env SOME=1 git checkout`) and let the write slip past.
fn segment_is_write_capable(segment: &[String]) -> bool {
    let cmd_idx = resolve_executable_index(segment);
    let cmd = segment[cmd_idx].as_str();
    let rest = &segment[cmd_idx + 1..];
    match cmd {
        "dd" => true,
        "curl" | "wget" => rest
            .iter()
            .any(|w| w == "-o" || w == "-O" || w.starts_with("--output")),
        "git" => {
            // First positional argument after the resolved `git` is the
            // subcommand. Resolving it relative to `git` (not to token 1)
            // keeps wrapper-invoked checkouts detected.
            let sub = rest
                .iter()
                .find(|w| !w.starts_with('-') && !is_shell_separator(w))
                .map(|s| s.as_str())
                .unwrap_or("");
            matches!(
                sub,
                "checkout"
                    | "restore"
                    | "reset"
                    | "stash"
                    | "rm"
                    | "mv"
                    | "apply"
                    | "am"
                    | "merge"
                    | "rebase"
                    | "cherry-pick"
                    | "clean"
                    | "pull"
            )
        }
        "sh" | "bash" | "zsh" | "ksh" | "dash" => rest.iter().any(|w| w == "-c"),
        _ => false,
    }
}

/// Detect commands whose own syntax can write files in a way the redirection /
/// operand parsers do not capture (`dd of=`, `curl -o`, `wget -O`,
/// `git checkout`/`git restore`, a nested `sh -c`, ...). Such commands must be
/// treated as write-capable, otherwise they could silently alter source in a
/// write-restricted phase.
///
/// Every command *segment* is inspected: a write hidden behind a shell
/// operator (`true && dd of=src/lib.rs`, `git pull ; git reset`) must not
/// escape detection by hiding in a later segment.
fn is_write_capable_command(command: &str) -> bool {
    let words = shell_words(command);
    if words.is_empty() {
        return false;
    }
    words
        .split(|w| is_shell_separator(w))
        .filter(|segment| !segment.is_empty())
        .any(segment_is_write_capable)
}

fn redirection_targets(words: &[String]) -> Vec<String> {
    let mut targets = Vec::new();
    for (idx, word) in words.iter().enumerate() {
        let op = word.as_str();
        if matches!(op, ">" | ">>" | "1>" | "1>>" | "2>" | "2>>") {
            if let Some(target) = words.get(idx + 1) {
                targets.push(clean_shell_word(target));
            }
        } else if let Some(target) = op.strip_prefix(">>").or_else(|| op.strip_prefix('>')) {
            if !target.is_empty() {
                targets.push(clean_shell_word(target));
            }
        } else if let Some(target) = op.strip_prefix("1>").or_else(|| op.strip_prefix("2>")) {
            if !target.is_empty() {
                targets.push(clean_shell_word(target));
            }
        }
    }
    targets
}

fn tee_targets(words: &[String]) -> Vec<String> {
    let mut targets = Vec::new();
    for (idx, word) in words.iter().enumerate() {
        if word != "tee" {
            continue;
        }
        for arg in &words[idx + 1..] {
            if arg.starts_with('-') {
                continue;
            }
            if is_shell_separator(arg) {
                break;
            }
            targets.push(clean_shell_word(arg));
        }
    }
    targets
}

fn command_operand_targets(words: &[String]) -> Vec<String> {
    let mut targets = Vec::new();
    let mut idx = 0;
    while idx < words.len() {
        let cmd = words[idx].as_str();
        if matches!(cmd, "cp" | "mv" | "install") {
            if let Some(target) = words[idx + 1..]
                .iter()
                .rev()
                .find(|arg| !arg.starts_with('-') && !is_shell_separator(arg))
            {
                targets.push(clean_shell_word(target));
            }
        } else if matches!(cmd, "touch" | "mkdir" | "rm" | "truncate") {
            targets.extend(
                words[idx + 1..]
                    .iter()
                    .take_while(|arg| !is_shell_separator(arg))
                    .filter(|arg| !arg.starts_with('-'))
                    .map(|arg| clean_shell_word(arg)),
            );
        }
        idx += 1;
    }
    targets
}

fn shell_words(command: &str) -> Vec<String> {
    command
        .replace(">>", " __OPENFLOWS_REDIR__ ")
        .replace('>', " __OPENFLOWS_REDIR__ ")
        .replace('|', " | ")
        .replace(';', " ; ")
        .split_whitespace()
        .map(|word| {
            if word == "__OPENFLOWS_REDIR__" {
                ">".to_string()
            } else {
                clean_shell_word(word)
            }
        })
        .filter(|word| !word.is_empty())
        .collect()
}

fn clean_shell_word(word: &str) -> String {
    // `;` is intentionally not trimmed: `shell_words` emits it as a standalone
    // shell-separator token so compound commands (`a ; b`) stay parseable as
    // separate segments. Trimming it here collapses the segments into one and
    // hides any write in a later segment.
    word.trim_matches(|c| matches!(c, '"' | '\'' | '(' | ')'))
        .to_string()
}

fn is_shell_separator(word: &str) -> bool {
    matches!(word, "|" | ";" | "&&" | "||")
}

fn is_allowed_lifecycle_artifact(path: &str) -> bool {
    let p = path.to_lowercase();
    let file = p.rsplit(['/', '\\']).next().unwrap_or(p.as_str());
    matches!(
        file,
        "plan.md"
            | "plan"
            | "status.json"
            | "blocker"
            | "blocker.md"
            | "blockers.md"
            | "handoff.md"
            | "final-review.md"
            | "review.md"
            | "review-report.md"
    ) || file.ends_with("-eval.md")
}

fn is_write_attempt(tool_name: &str, input: &Value) -> bool {
    is_write_tool(tool_name) || (is_shell(tool_name) && shell_writes_source(&command_text(input)))
}

/// Is the tool a read-only probe (bash read of status/dispatch/plan)?
///
/// `status set` is deliberately *not* a probe: it mutates durable state and,
/// in a blocked worker, would otherwise allow an unapproved phase transition
/// (e.g. `status set building`) to escape the blockage.
///
/// Probes are matched **per shell segment**, never by substring over the whole
/// command, so a compound command such as `git status; touch src/lib.rs` is not
/// a probe: its write segment fails the check and the whole call is denied
/// instead of allowing a trailing source write past the blocked gate.
///
/// An empty or unknown command is **not** a probe: at least one recognized,
/// non-empty probe segment must match or the whole command is denied, so a
/// wrapper/argv-driven invocation with no `command` field cannot masquerade as
/// a read-only probe (P1: "Argv Commands Bypass Blocking").
fn is_probe_command(command: &str) -> bool {
    let mut saw_probe = false;
    for seg in command.split([';', '&', '|']) {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        if !is_probe_segment(seg) {
            return false;
        }
        saw_probe = true;
    }
    saw_probe
}

/// Whether a single shell segment is a recognised read-only probe.
fn is_probe_segment(seg: &str) -> bool {
    let cmd = seg.to_lowercase();
    cmd.contains("status get")
        || cmd.contains("gate status")
        || cmd.contains("dispatch read")
        || cmd.contains("plan read")
        || cmd.contains("plan write")
        || cmd.contains("review read")
        || cmd.contains("git status")
        || cmd.contains("git diff")
        || cmd == "ls"
}

fn command_text(input: &Value) -> String {
    if let Some(command) = input.get("command").and_then(|c| c.as_str()) {
        return command.to_string();
    }
    if let Some(argv) = input.get("argv").and_then(|v| v.as_array()) {
        return argv
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");
    }
    input.as_str().unwrap_or_default().to_string()
}

/// An *exact* return-to-planning (or blocked-hold) transition command.
///
/// The whole command must match, not merely contain the transition substring.
/// A compound invocation like
/// `openflows-harness status set planning && dd of=src/lib.rs` contains
/// `status set planning` but is not a replan — allowing it would let the
/// blocked worker smuggle an unauthorized trailing write past the gate
/// (P1: "Compound Replans Bypass Blocking").
fn is_exact_replan(command: &str) -> bool {
    let cmd = command.trim().to_lowercase();
    cmd == "openflows-harness status set planning" || cmd == "openflows-harness status set blocked"
}

/// Coordination commands that must remain available when durable state is
/// inconsistent, especially `phase=building` with a missing/undecodable plan.
fn is_plan_recovery_command(tool_name: &str, input: &Value) -> bool {
    let lower = tool_name.to_lowercase();
    let command = if is_shell(&lower) || is_harness(&lower) {
        command_text(input).to_lowercase()
    } else {
        String::new()
    };

    if command.is_empty() || !command.contains("openflows-harness") {
        return false;
    }

    command.contains("plan write")
        || command.contains("status set planning")
        || command.contains("status set blocked")
        || command.contains("status get")
        || command.contains("plan read")
        || command.contains("dispatch read")
}

/// The path a write tool targets, from `tool_input`.
fn target_path(input: &Value) -> String {
    input
        .get("path")
        .or_else(|| input.get("file_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// True when the write is to / creates the plan artifact.
///
/// Plan writes are determined from the *validated target path* or the exact
/// harness `plan write` command — never from an unvalidated `is_plan` flag on
/// the tool input. Trusting a caller-supplied `is_plan: true` marker would let
/// a write to any source path masquerade as a plan write and slip past the
/// phase gate.
fn is_plan_write(tool_name: &str, input: &Value) -> bool {
    if tool_name.to_lowercase().contains("plan") {
        return true;
    }
    let path = target_path(input).to_lowercase();
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path.as_str());
    if matches!(file, "plan.md" | "plan") {
        return true;
    }
    // A harness `plan write` call uploads the plan.
    if is_harness(tool_name) {
        let cmd = input
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_lowercase();
        if cmd.contains("plan write") {
            return true;
        }
    }
    false
}

/// Is a write targeting the blocker report (allowed in the `blocked` phase)?
fn is_blocker_write(tool_name: &str, input: &Value) -> bool {
    let path = target_path(input).to_lowercase();
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path.as_str());
    if matches!(
        file,
        "status.json" | "blocker" | "blocker.md" | "blockers.md"
    ) {
        return true;
    }
    let _ = tool_name;
    false
}

/// Phase-appropriate guidance string for the model, relayed via `post_tool_use`
/// and `user_prompt_submit` so the agent always knows what its lifecycle expects.
pub fn phase_guidance(phase: &str, plan_exists: bool, role: &str) -> String {
    if role.eq_ignore_ascii_case("sentinel") {
        return "You are SENTINEL, the read-only reviewer. Your job is to review \
                 plans and PRs and submit verdicts — you must not write or edit source. \
                 Read the plan/PR, run read-only checks, and use `openflows-harness \
                 review submit` / `gate approve`."
            .to_string();
    }

    match phase {
        "planning" if !plan_exists => {
            "FORGE planning phase: a plan does not exist yet. Run `/plan` now to \
             analyze the ticket and write PLAN.md, then `openflows-harness plan write \
             --file PLAN.md`, then `openflows-harness status set planning`, and HALT \
             for SENTINEL gate approval before touching any source file."
                .to_string()
        }
        "planning" => "FORGE planning phase: your plan is written and awaiting SENTINEL gate \
             approval. Do not write source yet. Run `openflows-harness gate status \
             --phase planning`; HALT for approval, then `status set building`."
            .to_string(),
        "building" if !plan_exists => {
            "FORGE is in the building phase but NO plan exists in the shared store. \
             This is out of sequence. Stop and run `/plan` first: write PLAN.md, \
             upload it with `openflows-harness plan write --file PLAN.md`, and obtain \
             SENTINEL gate approval before continuing to build."
                .to_string()
        }
        "building" => "FORGE building phase: implement per PLAN.md. Write source and tests, run \
             the test suite, and signal completion with `openflows-harness status set`."
            .to_string(),
        "testing" => "FORGE testing phase: you are verifying behavior. Run the test suite, fix \
             failing tests, and only then `openflows-harness status set review_ready` \
             and open the PR."
            .to_string(),
        "review_ready" => {
            "FORGE review_ready phase: a PR is open and SENTINEL is reviewing it. Do \
             not modify source while under review — rework must re-enter \
             `status set planning`/`building` first."
                .to_string()
        }
        "blocked" => "FORGE blocked phase: record an exact, answerable blocker (STATUS.json) \
             and wait for NEXUS/human intervention. Do not write source or build."
            .to_string(),
        _ => {
            format!(
                "Signal the harness phase with `openflows-harness status set <phase>` and \
                 work to the phase's contract. Valid phases: {}.",
                PHASES.join(", ")
            )
        }
    }
}

/// Stateful phase-aware guard. `base` is the generic policy decision already
/// computed for non-phase cases (typically `observe()`); if `base` denies, we
/// keep the denial and don't downgrade it.
///
/// Also returns a `model_context` guidance string (through the returned
/// decision) whenever a phase rule fires, so `post_tool_use` can relay the
/// guideline back to the model.
pub async fn phase_guard(
    store: &SharedStore,
    chat_id: &str,
    role: &str,
    tool_name: &str,
    input: &Value,
    base: HookDecision,
) -> HookDecision {
    // Never downgrade an existing denial / rewrite from the generic policy.
    if base.deny || base.rewrite.is_some() {
        return base;
    }

    let lower = tool_name.to_lowercase();

    // SENTINEL: readonly reviewer → deny all writes.
    if role.eq_ignore_ascii_case("sentinel") && is_write_attempt(&lower, input) {
        return HookDecision::deny(
            "openflows policy: SENTINEL is a readonly reviewer — writes are blocked",
        )
        .with_model_context(phase_guidance("", false, role));
    }

    // Non-forge roles fall through to base.
    if !role.eq_ignore_ascii_case("forge") {
        return base;
    }

    // Resolve durable state.
    let hc = resolve_chat(store, chat_id).await;
    let Some(ticket) = hc.ticket_id else {
        return base;
    };
    let st = read_ticket_state(store, &ticket).await;
    let phase = st.phase.as_deref().unwrap_or("unset");
    let guidance = phase_guidance(phase, st.plan_exists, role);
    let mut decision = HookDecision::observe();

    // Only gate write-ish tools and harness/shell coordination. Non-write,
    // non-coordination tools (e.g. Read, MCP reads) fall through.
    if !is_write_tool(&lower) && !is_harness(&lower) && !is_shell(&lower) {
        return base;
    }

    // ── PLANNING ──────────────────────────────────────────────────────────
    if phase == "planning" {
        if is_write_attempt(&lower, input) && !st.plan_exists && !is_plan_write(&lower, input) {
            // First write must be the plan.
            decision = HookDecision::deny(
                "openflows policy: FORGE is in the planning phase and must write \
                 PLAN.md (or `openflows-harness plan write`) before touching source",
            );
        }
        return decision.with_model_context(guidance);
    }

    // ── BUILDING ──────────────────────────────────────────────────────────
    if phase == "building" {
        if !st.plan_exists {
            if is_plan_write(&lower, input) || is_plan_recovery_command(&lower, input) {
                return decision.with_model_context(guidance);
            }
            // Out-of-sequence: trying to build before a plan was ever uploaded.
            decision = HookDecision::deny(
                "openflows policy: FORGE is in the building phase but no plan exists \
                 in the shared store. Write and upload a plan first: run `/plan`, then \
                 `openflows-harness plan write --file PLAN.md`.",
            );
        }
        // Otherwise building writes are allowed; attach guidance regardless.
        return decision.with_model_context(guidance);
    }

    // ── TESTING ───────────────────────────────────────────────────────────
    if phase == "testing" {
        if !st.plan_exists {
            if is_plan_write(&lower, input) || is_plan_recovery_command(&lower, input) {
                return decision.with_model_context(guidance);
            }
            decision = HookDecision::deny(
                "openflows policy: FORGE is in the testing phase but no plan exists. \
                 Leave testing: run `/plan` and get the plan approved before building.",
            );
        }
        return decision.with_model_context(guidance);
    }

    // ── REVIEW_READY ──────────────────────────────────────────────────────
    if phase == "review_ready" {
        if is_write_attempt(&lower, input) {
            // Under review: source must not change; re-enter an earlier phase.
            decision = HookDecision::deny(
                "openflows policy: FORGE is in review_ready (PR under review). Do not \
                 modify source; for rework run `openflows-harness status set planning` \
                 (or `building`) first.",
            );
        }
        return decision.with_model_context(guidance);
    }

    // ── BLOCKED ───────────────────────────────────────────────────────────
    if phase == "blocked" {
        // Only read-only probes, blocker writes, and *returning to planning*
        // (re-planning) are permitted. Arbitrary forward transitions (e.g.
        // `status set building`) require explicit NEXUS/human authorization
        // and must not be reachable from a blocked worker.
        //
        // Classify the *normalized* command via `command_text` so the same
        // representation the write detector uses is what we test against: a
        // tool invoked through `argv` (with no `command` field) must not fall
        // back to an empty string that vacuously matches a probe (P1: "Argv
        // Commands Bypass Blocking").
        let command = command_text(input).to_lowercase();
        // A probe is only read-only if it is truly write-free: a probe segment
        // that embeds a source write (e.g. `git status > src/lib.rs` or
        // `git status | tee src/lib.rs`) must not slip past the blocked gate.
        let is_readonly_probe =
            is_shell(&lower) && is_probe_command(&command) && !shell_writes_source(&command);
        let is_replan = (is_harness(&lower) || is_shell(&lower)) && is_exact_replan(&command);
        let is_blocker = is_write_tool(&lower) && is_blocker_write(&lower, input);
        if !is_readonly_probe && !is_replan && !is_blocker {
            decision = HookDecision::deny(
                "openflows policy: FORGE is blocked. Only record a blocker (STATUS.json), \
                 probe state, or return to `status set planning`; do not write source or \
                 build.",
            );
        }
        return decision.with_model_context(guidance);
    }

    // Unknown/unset phase → attach general guidance, do not deny.
    decision.with_model_context(guidance)
}
