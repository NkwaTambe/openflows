# Delegated Verification: How SENTINEL Gets FORGE to Run Code, Without Ever Touching FORGE's Workspace

**Status:** Design reference for tracking issue [The-AgenticFlow/openflows#143](https://github.com/The-AgenticFlow/openflows/issues/143)
**Scope:** Cross-workspace command execution for acceptance-criteria verification, via an A2A relay hosted by `nexus`
**Related:** `docs/architecture/agent-orchestration-protocol.md`, `docs/architecture/OpenFlows_Coder_Integrated_Architecture.md`, `docs/governance.md`

---

## The problem: a reviewer that can't run the tests

SENTINEL's job is to gate FORGE's work at two points: reviewing `PLAN.md` before implementation starts, and reviewing the PR before it merges. Both of those reviews are, today, read-only — SENTINEL reads an artifact (a plan, a diff) and reasons about it.

That's fine for "does this plan make sense" or "is this diff internally consistent." It falls apart for a very ordinary question: **does the code actually work?** Answering that requires *running* something — `cargo test`, `npm test`, a lint pass, a build. And that command has to run somewhere with the repository checked out, which today means it has to run inside FORGE's Coder workspace, not SENTINEL's.

The naive fix — give SENTINEL shell access to FORGE's workspace — was rejected for three reasons that all come back to the same principle: **the reviewer must remain isolated from the thing it reviews.**

- It breaks the review's integrity. A reviewer that can mutate the artifact it's reviewing (intentionally, or via a prompt-injected ticket) is not a reviewer anymore.
- It couples SENTINEL's lifecycle to FORGE's workspace. If FORGE's workspace is torn down, gone, or mid-provisioning, SENTINEL loses a capability it needs for something as basic as approving a gate.
- Coder workspaces don't share a filesystem (see the "no shared filesystem across workspaces" discussion in `OpenFlows_Coder_Integrated_Architecture.md`), so this would have meant inventing a new cross-workspace mount just to serve one use case.

The design that shipped instead: SENTINEL never enters FORGE's workspace. It **delegates**. It sends FORGE a typed request — "run this command, tell me what happened" — and gets back a structured result. FORGE stays the only thing that touches its own filesystem. SENTINEL stays a pure reviewer, just one that can now ask for evidence instead of only reading claims.

## Why A2A, and why not just reuse Redis

OpenFlows already has a cross-workspace coordination mechanism: the Redis SharedStore, accessed exclusively through `openflows-harness`. `PLAN.md` lives there as `pair:{id}:plan`; handoffs, PR info, and review verdicts all live there too. So the obvious question is: why introduce a second protocol (A2A) instead of just writing a "please run this command" key to Redis and polling for the answer?

The two mechanisms are solving genuinely different problems, and conflating them would have made both worse:

| | Redis SharedStore | A2A |
|---|---|---|
| **Nature** | Durable artifact store | Live task/message exchange |
| **Good at** | "What is the current plan?" "What did the review say?" — anything that should survive a restart and be replayed/audited | "Go do this now, stream me progress, tell me when it's done" |
| **Semantics** | Poll-and-read | Request/response with SSE streaming, cancellation, resubscribe-after-disconnect |
| **Ecosystem** | Bespoke, tenant-namespaced keys | Standard task lifecycle, Agent Cards, JSON-RPC methods with existing tooling |

A `verify` request is fundamentally a *task*, not a *fact*. It has a lifecycle — submitted, running, producing incremental output, completing or timing out — that Redis's plain-key model can express, but only by reinventing exactly what A2A already standardizes (task IDs, progress streaming, cancellation, resubscription). Doing that with hand-rolled Redis polling would have produced a bespoke pseudo-A2A with worse ergonomics and no compatible tooling. So the design draws a firm line:

> **Redis remains the single source of truth for durable artifacts. A2A is used only for live task exchange, and every terminal A2A result is mirrored into Redis before the task is acknowledged complete.**

That second sentence matters as much as the first: A2A doesn't get to be a second, competing source of truth. The moment a `verify` task finishes, its result is written to `pair:{id}:verification` in Redis — so even though the *conversation* happened over A2A, the *record* lives in the same durable store as everything else, replayable and auditable the same way a PR or a plan is.

## Why a relay, not peer-to-peer

Given A2A for the live exchange, the next question is topology: does SENTINEL dial FORGE directly, or does something sit in between?

Coder workspaces are reliably good at making outbound connections and bad at accepting inbound ones — there's no stable address for "the FORGE workspace for pair T-048" that SENTINEL could dial, short of wiring up `coder_app` URLs and a discovery mechanism for every pair. So the design puts a **relay inside `nexus`**, the existing control-plane workspace, and has both SENTINEL and FORGE open outbound connections to it. Nexus routes messages by `(pair_id, role)`.

This isn't just a networking convenience. It's the enforcement point:

- **Authorization** — nexus is the only place that knows the pair's role map, so it's the only place that can refuse a request from a SENTINEL trying to address a FORGE outside its own pair.
- **Command allowlisting** — nexus rejects `verify` requests whose command doesn't match a known-safe prefix (`cargo test`, `npm test`, etc.) before it ever reaches an executor. Rejections are logged to `audit:a2a:rejected`.
- **Audit** — every accepted request and its result gets a durable trail, because nexus is the chokepoint that writes it.
- **A single kill switch** — if something goes wrong with delegated verification as a feature, there's one relay to disable, not N peer connections.

The alternative — SENTINEL and FORGE dialing each other directly once nexus hands out short-lived tokens — was considered and explicitly deferred. It solves nothing that the relay doesn't already solve for v1, and it adds a token-exchange and NAT-traversal problem that isn't worth taking on before the simpler design has been proven.

## What actually gets sent

The whole capability rests on one task type: `verify`. It's deliberately narrow — not "run any shell command," but "run *this specific, allowlisted* command and tell me what happened":

```jsonc
// SENTINEL → nexus → FORGE
{
  "task_type": "verify",
  "pair_id": "T-048",
  "verify": {
    "kind": "command",
    "cwd": "repo",
    "argv": ["cargo", "test", "--package", "foo"],
    "timeout_secs": 600,
    "expect": { "exit_code": 0 }
  }
}
```

```jsonc
// FORGE → nexus → SENTINEL, and mirrored to pair:T-048:verification
{
  "task_id": "…",
  "exit_code": 0,
  "duration_ms": 12843,
  "stdout_ref": "audit:a2a:{task_id}:stdout",
  "artifacts": [],
  "executor": { "role": "forge", "workspace": "forge-T-048" }
}
```

Everything about this shape is designed to be boring on purpose:

- `kind: "command"` is the only implemented kind. `artifact_check` (verify a file hash without running anything) is reserved but not built — no reason to build the general case before the specific one has proven itself.
- `cwd` is an enum (`repo` | `worktree`), not a free-form path. Nexus validates that the resolved path can't escape the pair's own checkout.
- `argv` must match the allowlist. There is no "sudo bypass" flag, no free-form shell string, no `sh -c`.
- The result never contains raw stdout/stderr inline — only Redis references, capped at 1 MiB per stream. Big test output doesn't get to blow up the A2A message size; it lives in the same durable store as everything else.

This is what makes the isolation argument actually hold up: SENTINEL isn't gaining "run arbitrary code in FORGE's workspace," it's gaining "ask FORGE to run one of a handful of pre-approved test/build commands." The blast radius of a compromised or misbehaving SENTINEL is bounded by the allowlist nexus enforces, not by SENTINEL's imagination.

## Failure is the interesting part

A synchronous "run this and get a result" RPC is easy to design for the happy path. The reason this took real design work is the failure modes, because every one of them has a wrong answer that looks tempting:

- **FORGE's workspace is offline.** The tempting wrong answer is to silently retry until it comes back. The actual answer: nexus reports `executor_unavailable` immediately, and SENTINEL records a `blocked` verdict. A missing executor is not evidence of anything — it's not "probably fine," it's unknown, and unknown does not get to approve a gate.
- **The command runs forever.** FORGE enforces `timeout_secs` itself (process-group kill), and the result explicitly carries `timed_out: true`. A `VerifyResult::satisfies()` check treats a timeout as failing regardless of what the exit code claims to be — you cannot time out into a pass.
- **SENTINEL disconnects mid-task.** The task keeps running in FORGE's workspace; nexus buffers events; SENTINEL reconnects via `tasks/resubscribe` and gets the tail it missed. Crucially, the result still gets mirrored to Redis *even if SENTINEL never reconnects* — the verification isn't lost just because the reviewer's connection blipped.
- **The same request gets submitted twice** (a retry after a network hiccup, say). Nexus dedups on `(pair_id, sha256(request_body))` within a TTL window, so a flaky connection doesn't cause FORGE's workspace to run the same test suite three times.
- **Redis is down when the result should be mirrored.** The task is marked `completed_unpersisted` and nexus retries the mirror — but SENTINEL must not treat an unpersisted result as sufficient to approve. A verification that only lived in a message queue and never made it to durable storage is not yet a fact.

None of these are exotic. They're the ordinary failure modes of any distributed request/response system, and the design's answer to all of them is the same instinct: **when in doubt, don't approve.** That instinct is also, not coincidentally, the fix for the actual incident that motivated this work in the first place.

## The other half of the fix: refusing to approve blind

This capability exists because of a real failure: a SENTINEL instance was asked to approve a planning gate for ticket T-048 and discovered it had no way to read `PLAN.md` or reach the ticketing tool at all — wrong environment, missing binary, empty workspace. Faced with that, the correct move is to say so and stop. `verify` delegation is the mechanism for one specific gap in that story (verifying *code*), but the general principle it's built alongside is broader:

> **SENTINEL must hard-fail — never approve — when a required artifact is missing or unreadable.**

This applies whether the missing thing is `PLAN.md`, a PR diff, or a `verify` result that never got persisted to Redis. The gate isn't "approve unless something looks wrong." It's "approve only once the required evidence is actually in hand." Everything else, including this whole A2A relay, exists to make sure that evidence can actually reach SENTINEL when it's needed — not to give SENTINEL an excuse to approve without it.

## What this deliberately doesn't do yet

- **No dedicated `verifier` role.** FORGE is the executor in v1. The `verify` task shape is already executor-agnostic (`executor.role` is a field, not an assumption baked into the schema), so introducing a separate verifier workspace later is a nexus routing change, not a protocol change. It's deferred until there's real usage data on how often FORGE is too busy with implementation work to also serve as its own verifier.
- **No arbitrary command execution.** The allowlist is static and small on purpose. Widening it is a deliberate, reviewable change to nexus, not a runtime configuration SENTINEL can influence.
- **No cross-pair verification.** A SENTINEL (or a future LORE) auditing multiple pairs at once isn't supported — the routing table is keyed strictly on `pair_id`, matching the isolation guarantee this whole design exists to protect.

## Where this fits in the bigger picture

Nothing about this changes the shape of the existing gated-phase state machine in `openflows-harness` (`planning` → `building` → `testing` → `review_ready`, with SENTINEL's `gate approve` consuming a single-use approval token via Redis `GETDEL`). Delegated verification is additive: one more thing SENTINEL can *ask for* before it decides whether to call `gate approve`. The gate itself still lives entirely in Redis, still requires the SENTINEL role, and still can't be bypassed by FORGE approving its own plan.

What changes is what "evidence" can mean. Before this, a SENTINEL review was necessarily an act of reading — a plan, a diff — and reasoning from prose. Now it can also be an act of *asking the code to speak for itself*, through a narrow, audited, allowlisted channel that never requires SENTINEL to leave the boundary of its own workspace.

---

## Implementation Status (Issue #143)

This feature is being implemented across 8 tasks tracked in `.kilo/plans/1785948146715-sentinel-forge-a2a-verify.md`:

### ✅ Completed

1. **Task 1: a2a-protocol crate** — Serde types, Redis key helpers, command allowlist validation (9 tests passing)
2. **Task 2: Nexus A2A relay server** — JSON-RPC HTTP server with Axum, pair-scoped routing, idempotency dedup, result mirroring
3. **Task 3: Harness verify subcommands** — CLI skeleton for `verify request`, `verify serve`, `verify list`
4. **Task 4: Sentinel gate refusal** — Hard-fail when `pair:{id}:plan` is missing, preventing blind approvals (issue #143 root cause fix)
5. **Task 5.1: A2A client** — HTTP client library for harness workers (health check, submit_verify_request, polling)

### 🚧 In Progress / Deferred

6. **Task 5.2-5.3: Executor sandbox + mirroring** — Full sandbox execution (process group + timeout), stdout/stderr streaming, result persistence
7. **Task 6: Worker template wiring** — Updated Forge template to spawn `verify serve` daemon on startup
8. **Task 7: Documentation updates** — This document; pending updates to orchestration protocol docs
9. **Task 8: Integration & E2E tests** — Comprehensive test suite for A2A relay, sandbox isolation, failure modes

### Next Steps

- **Phase 2a:** Complete executor sandbox implementation with proper stdout/stderr capture and streaming via SSE
- **Phase 2b:** Full integration testing covering: relay routing, executor availability checks, timeouts, disconnection/reconnection, idempotency validation
- **Phase 3:** Extend support to additional executor roles (dedicated `verifier` workspace) based on usage data

### How to Use (v1)

```bash
# Sentinel workspace: submit a verify request
openflows-harness verify request \
  --argv cargo test --package mylib \
  --timeout-secs 300 \
  --expect-exit 0

# Forge workspace: automatically started via worker template
# (the `verify serve` daemon subscribes to tasks from nexus relay)

# Human audit: view recent verification results
openflows-harness verify list --pair-id T-048
```

### References

- **Main tracking issue:** [The-AgenticFlow/openflows#143](https://github.com/The-AgenticFlow/openflows/issues/143)
- **Plan document:** `.kilo/plans/1785948146715-sentinel-forge-a2a-verify.md`
- **Implementation branches:** `feat/a2a-verify-v2` (current)
