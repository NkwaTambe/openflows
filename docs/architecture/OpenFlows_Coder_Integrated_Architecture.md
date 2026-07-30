# OpenFlows × Coder: Integrated Architecture

**Design Document v2.0**  
**Status:** Implemented  
**Last Updated:** 2026-07-29

---

## Executive Summary

**Coder governs WHERE agents run. OpenFlows governs HOW agents coordinate.** Together they form a complete enterprise AI development platform.

This document describes how OpenFlows multi-agent orchestration integrates with Coder workspace infrastructure to provide secure, governed, autonomous software development.

---

## Table of Contents

1. [Core Thesis](#1-core-thesis)
2. [Architecture Overview](#2-architecture-overview)
3. [Agent Roles and Responsibilities](#3-agent-roles-and-responsibilities)
4. [Integration Layers](#4-integration-layers)
5. [Flow Graph and Routing](#5-flow-graph-and-routing)
6. [Planning Gate Workflow](#6-planning-gate-workflow)
7. [State Management](#7-state-management)
8. [Harness CLI](#8-harness-cli)
9. [Security Model](#9-security-model)
10. [Failure Recovery](#10-failure-recovery)
11. [Implementation Roadmap](#11-implementation-roadmap)
12. [Decision Records](#12-decision-records)

---

## 1. Core Thesis

This is a **platform layer** (Coder) + **application layer** (OpenFlows) relationship. Coder provides secure, governed workspace infrastructure with identity, audit, and network isolation. OpenFlows provides architectural intelligence — flow graphs, typed contracts, multi-agent coordination, and self-healing reconciliation. Neither duplicates the other's core competency.

| Layer | Coder | OpenFlows |
|-------|-------|-----------|
| **What it solves** | Where agents run safely | How agents coordinate intelligently |
| **Core primitive** | Terraform workspace templates | PocketFlow flow graph + Node trait |
| **Governance model** | Infrastructure-first — govern execution environment | Architecture-first — plan before execute |
| **Failure handling** | Workspace isolation + identity tracing | NEXUS `reconcile()` + flow recovery |
| **Agent model** | Single agent per workspace (`spawn_agent` for sub-tasks) | Differentiated agents: NEXUS, FORGE, SENTINEL, VESSEL, LORE |
| **State management** | Chat persistence in database | SharedStore (Redis) with typed schemas |
| **Orchestration** | Sequential agent loop with tool calls | Multi-agent coordinated via action-routing flow graph |

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        CODER CONTROL PLANE                                   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                     LLM PROVIDERS                                     │   │
│  │   Anthropic · OpenAI · Google · Azure · AWS Bedrock · Custom        │   │
│  └───────────────────────────────┬─────────────────────────────────────┘   │
│                                  │ API calls only                            │
│  ┌───────────────────────────────▼─────────────────────────────────────┐   │
│  │                OPENFLOWS ORCHESTRATION ENGINE                         │   │
│  │                                                                     │   │
│  │   ┌──────────┐    ┌───────────────────┐    ┌──────────┐              │   │
│  │   │  NEXUS   │───▶│   PocketFlow      │───▶│  VESSEL  │              │   │
│  │   │ (orchestrator)│  (routing table)   │    │ (merge) │              │   │
│  │   └──────────┘    └───────────────────┘    └──────────┘              │   │
│  │                           │                                         │   │
│  │          ┌────────────────┼────────────────┐                        │   │
│  │          ▼                ▼                ▼                        │   │
│  │   ┌────────────┐   ┌────────────┐   ┌────────────┐                  │   │
│  │   │ FORGE-     │   │ FORGE-     │   │ FORGE-     │                  │   │
│  │   │ SENTINEL   │   │ SENTINEL   │   │ SENTINEL   │                  │   │
│  │   │  Pair-1    │   │  Pair-2    │   │  Pair-N    │                  │   │
│  │   └─────┬──────┘   └─────┬──────┘   └─────┬──────┘                  │   │
│  │         │                 │                 │                        │   │
│  │   ┌─────▼──────────────────▼─────────────────▼──────┐                │   │
│  │   │              SharedStore (Redis)                │                │   │
│  │   │   tickets · workers · PRs · gates · events      │                │   │
│  │   └─────────────────────────────────────────────────┘                │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                CODER INFRASTRUCTURE LAYER                           │   │
│  │   Template Registry · Identity (SSO) · Audit Log · MCP Config      │   │
│  │   Git Auth · Model Governance · Usage Analytics · Cost Controls   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────┐   ┌─────────────────────────────────────┐        │
│  │  CODER TAILNET     │   │  Coder Workspace Daemon            │        │
│  │  (DERP relay/P2P) │   │  (file I/O, shell, processes)       │        │
│  └──────────┬──────────┘   └─────────────────┬───────────────────┘        │
│             │                                │                            │
└─────────────┼────────────────────────────────┼────────────────────────────┘
              │                                │
  ┌───────────▼────────────────────────────────▼──────────────┐
  │              CODER WORKSPACES (Network Isolated)          │
  │                                                           │
  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │
  │  │ Workspace-1  │  │ Workspace-2  │  │ Workspace-N  │   │
  │  │ (forge-1     │  │ (forge-2     │  │ (forge-N     │   │
  │  │  + sentinel) │  │  + sentinel) │  │  + sentinel) │   │
  │  │              │  │              │  │              │   │
  │  │  git checkout│  │  git checkout│  │  git checkout│   │
  │  │  /src /tests │  │  /src /tests │  │  /src /tests │   │
  │  │  No API keys │  │  No API keys │  │  No API keys │   │
  │  │  No agent SW │  │  No agent SW │  │  No agent SW │   │
  │  └──────────────┘  └──────────────┘  └──────────────┘   │
  │                                                           │
  │  Egress: only git provider + control plane                │
  └───────────────────────────────────────────────────────────┘
```

---

## 3. Agent Roles and Responsibilities

### NEXUS (Orchestrator)

**Role:** The brain of the entire pipeline. Assigns work, detects broken states, resumes stalled workflows, and routes work to correct agents.

**Key Responsibilities:**
- **Sprint orchestration:** Sync issues from GitHub, assign tickets to available workers
- **Flow recovery:** Detects and recovers orphaned tickets, unmerged PRs, crashed workspaces
- **SENTINEL spawning:** When FORGE enters `planning` phase or `review_ready` phase, NEXUS provisions a SENTINEL workspace and chat for review
- **Command Gate:** Approves/rejects dangerous command proposals from workers
- **Worker slot management:** Tracks worker availability, provisions Coder workspaces

**State Tracking:**
- Reads `KEY_TICKETS`, `KEY_WORKER_SLOTS`, `KEY_PENDING_PRS`
- Writes dispatch payloads, workspace IDs, worker assignments

### FORGE (Builder)

**Role:** Implements tickets. Writes code, runs tests, opens PRs.

**Key Responsibilities:**
- **Planning phase:** Writes `PLAN.md`, sets `status set planning`, waits for SENTINEL gate approval
- **Implementation:** After gate approval, implements changes
- **Testing:** Runs tests, fixes failures
- **PR creation:** Opens PR, sets `status set review_ready`

**Gated Workflow:**
```
planning ──[SENTINEL approves]──> building ──> testing ──> review_ready
    │                                                           │
    └── HALT until gate approved                               └── PR opened
```

### SENTINEL (Reviewer)

**Role:** Adversarial code reviewer. The last line of defense before merge.

**Key Responsibilities:**
- **Plan review:** Reviews `PLAN.md` during planning gate, runs `openflows-harness gate approve --phase planning`
- **PR review:** Reviews code changes, posts inline comments on GitHub
- **Spec verification:** Ensures implementation matches ticket requirements

**Review Types:**
1. **Planning gate review:** Triggered when FORGE enters `planning` phase
2. **PR review:** Triggered when FORGE enters `review_ready` phase

### VESSEL (Merge Gatekeeper)

**Role:** CI watchdog and merge executor. Polls GitHub CI, handles conflicts, performs merges.

**Key Responsibilities:**
- CI polling and timeout handling
- Merge conflict detection and escalation
- Squash merges after CI passes

### LORE (Documentarian)

**Role:** Generates documentation, changelogs, and ADRs after merge.

---

## 4. Integration Layers

### Layer 1: Workspace as Execution Substrate

OpenFlows provisions Coder workspaces instead of local git worktrees.

| OpenFlows Concept | Coder Equivalent |
|-------------------|------------------|
| Git worktree `worktrees/pair-N/` | Coder workspace (template-based) |
| `git worktree add` | `create_workspace` (Coder API) |
| Local process spawn (CLI) | Agent loop → workspace daemon tool calls |
| `pair-N/shared/STATUS.json` | SharedStore `pair:{id}:status` key |
| File lock directory | Coder workspace isolation (kernel-level) |

**Key benefit:** Each FORGE-SENTINEL pair gets complete workspace isolation. No API keys in workspace. Network egress restricted to git provider and control plane.

### Layer 2: PocketFlow-in-Coder (Agent Loop Replacement)

OpenFlows uses PocketFlow for orchestration routing, not Coder's agent loop:

```rust
// PocketFlow Node trait
#[async_trait]
pub trait Node: Send + Sync {
    fn name(&self) -> &str;
    async fn prep(&self, store: &SharedStore) -> Result<Value>;
    async fn exec(&self, prep_result: Value) -> Result<Value>;
    async fn post(&self, store: &SharedStore, exec_result: Value) -> Result<Action>;
}
```

**Flow execution:**
1. `prep()` — Read from SharedStore, no side effects
2. `exec()` — Do the work (LLM calls, GitHub API, workspace operations)
3. `post()` — Write results to store, return next routing Action

**Action strings** drive routing: `work_assigned`, `pr_opened`, `merge_prs`, `planning_gate`, etc.

### Layer 3: Identity and Governance Bridge

Every OpenFlows agent action inherits the submitting user's Coder identity.

| Property | OpenFlows Standalone | OpenFlows + Coder |
|----------|---------------------|-------------------|
| API key management | Per-agent env vars, in worktree | Control plane only, zero workspace exposure |
| User identity | GitHub PAT (shared) | Per-user Coder SSO identity |
| Network isolation | None (agents need network) | Workspace egress restricted |
| Audit logging | SharedStore events (volatile) | Coder audit log + SharedStore events |
| Model governance | Per-agent .env config | Centralized Coder admin panel |
| Cost attribution | None | Per-flow cost attributed to user |

### Layer 4: MCP Tool Bridge

Coder workspace management becomes MCP tools:

| Tool | Purpose |
|------|---------|
| `coder_create_workspace` | Provision workspace from template |
| `coder_start_workspace` | Start stopped workspace |
| `coder_stop_workspace` | Stop running workspace |
| `coder_read_file` | Read file from workspace |
| `coder_write_file` | Write file to workspace |
| `coder_execute` | Run shell command in workspace |

### Layer 5: Hybrid Deployment Model

| Mode | Description | Use Case |
|------|-------------|----------|
| **OpenFlows Standalone** | Local worktrees, local agents, SharedStore | Individual developers, small teams, OS contributors |
| **Coder + OpenFlows Integrated** | Coder workspaces + OpenFlows orchestration | Enterprises, regulated industries, teams needing governance |
| **Coder Only** | Coder Agents without OpenFlows orchestration | Single-agent workflows, no multi-agent coordination |

---

## 5. Flow Graph and Routing

The flow graph is defined in `binary/src/bin/agentflow.rs`:

```rust
let mut flow = Flow::new("nexus")
    .add_node("nexus", nexus, vec![
        (ACTION_WORK_ASSIGNED, "forge_pair"),
        (ACTION_MERGE_PRS, "vessel"),
        ("approve_command", "forge_pair"),
        ("reject_command", "nexus"),
        ("sentinel_spawned", "sentinel"),
    ])
    .add_node("forge_pair", forge_pair, vec![
        (ACTION_PR_OPENED, "sentinel"),
        (ACTION_PLANNING_GATE, "nexus"),     // ← Planning gate → NEXUS spawns SENTINEL
        (ACTION_REVIEW_READY, "nexus"),       // ← Review ready → NEXUS spawns SENTINEL
        (ACTION_FAILED, "nexus"),
        (Action::NO_TICKETS, "nexus"),
        ("suspended", "nexus"),
    ])
    .add_node("sentinel", sentinel, vec![
        ("review_approve", "vessel"),
        ("review_reject", "forge_pair"),
        ("no_work", "nexus"),
    ])
    .add_node("vessel", vessel, vec![
        (ACTION_DEPLOYED, "nexus"),            // or "lore" if LORE active
        (ACTION_DEPLOY_FAILED, "nexus"),
        (ACTION_CI_FIX_NEEDED, "forge_pair"),
        ("merge_blocked", "nexus"),
        (ACTION_CONFLICTS_DETECTED, "forge_pair"),
        (Action::AWAITING_HUMAN, "nexus"),
        ("no_work", "nexus"),
    ]);
```

### Routing Table

| Action | Source | Target | Trigger |
|--------|--------|--------|---------|
| `work_assigned` | NEXUS | forge_pair | New ticket assigned |
| `pr_opened` | forge_pair | sentinel | FORGE opened PR |
| `planning_gate` | forge_pair | nexus | FORGE in planning phase |
| `review_ready` | forge_pair | nexus | FORGE ready for review |
| `sentinel_spawned` | nexus | sentinel | SENTINEL chat created |
| `review_approve` | sentinel | vessel | SENTINEL approved PR |
| `review_reject` | sentinel | forge_pair | SENTINEL requested changes |
| `merge_prs` | nexus | vessel | Unmerged PRs detected |
| `deployed` | vessel | nexus/lore | PR merged successfully |

---

## 6. Planning Gate Workflow

The planning gate is a **crucial checkpoint** in the FORGE-SENTINEL workflow. FORGE must obtain SENTINEL approval before proceeding to implementation.

### Why the Planning Gate Exists

1. **Catch misunderstandings early:** Spec mismatches caught at planning cost far less than after implementation
2. **Enforce design review:** Complex changes require architectural review
3. **Prevent scope creep:** SENTINEL verifies the plan matches the ticket requirements

### Complete Planning Gate Lifecycle

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         PLANNING GATE WORKFLOW                                │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  1. FORGE receives ticket assignment from NEXUS                              │
│     └── write dispatch payload to SharedStore                                │
│                                                                              │
│  2. FORGE analyzes ticket, writes PLAN.md                                    │
│                                                                              │
│  3. FORGE: openflows-harness status set planning                             │
│     └── Writes: ns:{tenant}:ticket:{id}:status = {"phase": "planning", ...}  │
│     └── Status key: {"phase": "planning", "role": "forge", "ts": ...}        │
│                                                                              │
│  4. ForgePairNode polls Redis status                                         │
│     └── Detects phase == "planning"                                          │
│     └── Emits ACTION_PLANNING_GATE → routes to NEXUS                         │
│                                                                              │
│  5. NEXUS: poll_harness_status_and_spawn_agents()                            │
│     └── Detects phase == "planning"                                          │
│     └── Checks: gate approval exists? (ns:{tenant}:ticket:{id}:gate:planning)│
│     └── If NO approval:                                                      │
│         ├── provisions SENTINEL workspace (if needed)                        │
│         ├── creates SENTINEL chat with plan review prompt                     │
│         ├── writes dispatch payload with {"review_type": "planning_gate"}      │
│         └── SENTINEL chat receives instructions to review PLAN.md             │
│                                                                              │
│  6. SENTINEL reads PLAN.md                                                   │
│     └── Reviews plan against ticket requirements                              │
│     └── If good: openflows-harness gate approve --phase planning              │
│         └── Writes: ns:{tenant}:ticket:{id}:gate:planning = {               │
│               "phase": "planning",                                           │
│               "approved_by": "sentinel",                                      │
│               "ts": ...,                                                      │
│               "notes": "Plan approved. Proceed with implementation."           │
│             }                                                                 │
│     └── If issues: Provides feedback, does NOT approve gate                   │
│                                                                              │
│  7. FORGE: openflows-harness status set building                             │
│     └── Harness checks gate approval (GETDEL - atomic consume)               │
│     └── If approved: transition succeeds, FORGE proceeds                     │
│     └── If NOT approved: transition REJECTED, FORGE must wait                │
│                                                                              │
│  8. FORGE implements, tests, opens PR                                         │
│     └── openflows-harness status set review_ready                            │
│                                                                              │
│  9. NEXUS detects review_ready → spawns SENTINEL for PR review               │
│                                                                              │
│ 10. SENTINEL reviews PR, approves/rejects                                    │
│                                                                              │
│ 11. VESSEL merges if approved                                                 │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Gate Approval Enforcement

The harness CLI enforces gated transitions in `openflows-harness/src/store.rs`:

```rust
/// Phases that require SENTINEL approval before transitioning FROM them.
const GATED_PHASES: &[&str] = &["planning"];

/// Phases a brand-new ticket may enter directly (can't skip planning gate).
const ENTRY_PHASES: &[&str] = &["planning", "blocked"];

pub async fn status_set(&self, ticket: &str, role: &str, phase: &str) -> Result<()> {
    // ... phase validation ...
    
    // Check if transitioning FROM a gated phase
    if let Some(source_phase) = gate_source_for_transition(current_phase, phase) {
        // Atomically GET-and-DELETE the approval (single-use guarantee)
        let gate_key = format!("ns:{}:ticket:{}:gate:{}", tenant, ticket, source_phase);
        let consumed_approval: Option<String> = self.client.getdel(&gate_key).await?;
        
        if consumed_approval.is_none() {
            bail!(
                "Cannot transition from '{}' to '{}' without SENTINEL approval.\n\
                 SENTINEL must run: openflows-harness gate approve --phase {}",
                source_phase, phase, source_phase
            );
        }
    }
    // ... write status ...
}
```

### Sentinel Gate Approval

Only SENTINEL can approve a gate:

```rust
pub fn authorize_gate_approver(role: &str) -> Result<()> {
    if !role.eq_ignore_ascii_case("sentinel") {
        bail!(
            "Gate approval rejected: approver role '{}' is not SENTINEL. \
             Only SENTINEL may approve a gated phase transition.",
            role
        );
    }
    Ok(())
}
```

---

## 7. State Management

### SharedStore Keys

All state lives in Redis with tenant namespacing: `ns:{tenant}:{key}`.

| Key Pattern | Type | Purpose |
|-------------|------|---------|
| `ns:{tenant}:tickets` | `Vec<Ticket>` | All known tickets |
| `ns:{tenant}:worker_slots` | `HashMap<String, WorkerSlot>` | Worker availability |
| `ns:{tenant}:pending_prs` | `Vec<Value>` | PRs awaiting merge |
| `ns:{tenant}:ticket:{id}:status` | `{phase, role, ts}` | Current harness phase |
| `ns:{tenant}:ticket:{id}:gate:{phase}` | `GateApproval` | Gate approval record |
| `ns:{tenant}:ticket:{id}:chat:{role}` | `String` | Coder chat ID |
| `ns:{tenant}:ticket:{id}:dispatch:{role}` | `DispatchPayload` | Task assignment |
| `ns:{tenant}:ticket:{id}:review:{role}` | `ReviewPayload` | Review verdict |

### Ticket Status Types

```rust
enum TicketStatus {
    Open,
    Assigned { worker_id: String },
    InProgress { worker_id: String },
    Completed { worker_id: String, outcome: String },
    Merged { worker_id: String, pr_number: u64 },
    Failed { worker_id: String, reason: String, attempts: u32 },
    Exhausted { worker_id: String, attempts: u32 },
    AwaitingHuman { worker_id: String, reason: String, attempts: u32 },
}
```

### Valid Harness Phases

| Phase | Meaning | Next Phase |
|-------|---------|------------|
| `planning` | Writing PLAN.md, awaiting gate approval | `building` (after gate) |
| `building` | Implementation in progress | `testing` |
| `testing` | Running tests | `review_ready` |
| `review_ready` | PR opened, awaiting SENTINEL review | `building` (if rejected) |
| `blocked` | Cannot proceed without help | Any (after unblock) |

---

## 8. Harness CLI

The `openflows-harness` CLI is the only component inside FORGE/SENTINEL workspaces that writes to Redis. All writes are validated against typed schemas.

### Key Commands

```bash
# Read task assignment
openflows-harness dispatch read

# Get/set current phase
openflows-harness status get
openflows-harness status set planning
openflows-harness status set building
openflows-harness status set testing
openflows-harness status set review_ready
openflows-harness status set blocked

# Gate approval (SENTINEL only)
openflows-harness gate approve --phase planning --notes "Plan approved"
openflows-harness gate status --phase planning

# Record PR
openflows-harness pr opened --pr 42 --branch feat/xyz --title "Feature"

# Handoff between agents
openflows-harness handoff write --contract CONTRACT.md --notes "Ready for review"

# Heartbeat (daemonized)
openflows-harness heartbeat start
openflows-harness heartbeat stop
```

### Phase Validation

The harness enforces:
1. First status must be `planning` or `blocked` (can't skip the gate)
2. Transitions from `planning` require a consumed gate approval
3. Gate approvals are single-use (GETDEL)

---

## 9. Security Model

### Threat Model Comparison

| Threat | OpenFlows Only | OpenFlows + Coder |
|--------|----------------|-------------------|
| API key exfiltration | PAT in workspace (extractable) | Keys in control plane only |
| Cross-pair access | File locks (flock) | Workspace isolation (kernel-level) |
| Unauthorized commands | Hook scripts (bypassable) | Coder template policy + hooks |
| Network exfiltration | No restriction | Egress firewall per template |
| Audit trail | SharedStore events (volatile) | Coder audit log + SharedStore events |
| User attribution | GitHub PAT per bot | Per-user Coder SSO identity |
| Secret scanning | Git hooks on push | Coder template policy + git hooks |

### Network Isolation Per Workspace

```
workspace-1 egress:
  ALLOW tcp/443 → github.com (git push/pull)
  ALLOW tcp/443 → coder-control-plane (workspace daemon heartbeat + LLM)
  DENY all other outbound
```

No workspace needs access to LLM providers. All model inference happens in the control plane. This eliminates entire classes of data exfiltration.

---

## 10. Failure Recovery

### Recovery Scenarios

| Scenario | OpenFlows Only | OpenFlows + Coder |
|----------|-----------------|-------------------|
| Workspace crash | Harness detects process exit | Coder detects workspace health, NEXUS restarts |
| Network failure mid-flow | SharedStore survives | Coder database persists chat history |
| Context exhaustion | HANDOFF.md hard reset | Coder auto-compaction + HANDOFF.md |
| Workspace unreachable | Manual cleanup | NEXUS calls `stop_workspace` + `create_workspace` |
| Merge conflict | VESSEL writes CONFLICT_RESOLUTION.md | VESSEL writes via Coder write_file |
| Stalled workspace | Watchdog timer in harness | Coder workspace timeout + harness watchdog |

### NEXUS Flow Recovery

NEXUS `reconcile()` runs on every loop iteration and detects:

1. **Unmerged PRs:** PRs in `pending_prs` not processed by VESSEL
2. **Orphaned tickets:** Tickets `Assigned`/`InProgress` but worker is `Idle` or missing
3. **Stale workers:** Workers `Assigned`/`Working`/`Suspended` but ticket no longer exists
4. **Completed without PR:** Tickets marked `Completed(pr_opened)` but no entry in `pending_prs`
5. **Crashed workspaces:** Workspace heartbeat stale for >90s
6. **Crashed chats:** Coder chat status is `Error`
7. **Missing gate approvals:** Tickets stuck in `planning` without SENTINEL chat spawned

---

## 11. Implementation Roadmap

### Phase 1: CoderTransport (Non-Breaking)
Add `CoderTransport` abstraction (`LocalTransport` + `CoderTransport`) and `SharedStore` migration from filesystem to Redis. Zero breaking changes to existing users.

### Phase 2: SharedStore Migration for Pair State
Move communication artifacts from `shared/` to SharedStore (`pair:{id}:{artifact}`) keys.

### Phase 3: Coder Provisioner
NEXUS gains `create_workspace`/`start_workspace`/`stop_workspace` capabilities. FORGE-SENTINEL pairs provision Coder workspaces instead of local worktrees.

### Phase 4: Governance Integration
Centralized model governance, audit logging, cost controls via Coder admin panel.

### Phase 5: MCP Tool Bridge
Register OpenFlows orchestration tools (`flow_status`, `list_workers`, `approve_command`) as Coder MCP servers for user visibility.

---

## 12. Decision Records

### DR-001: SharedStore over Filesystem for Pair State
**Context:** Coder workspaces don't share filesystem. Pair communication must be accessible across workspaces.

**Decision:** Migrate `shared/` artifacts to SharedStore (`pair:{id}:{artifact}`) keys. Keep filesystem-based `shared/` for standalone mode.

**Consequences:** Both modes work; Redis becomes required for Coder mode.

### DR-002: CoderTransport as Abstraction, Not Replacement
**Context:** Need both local and Coder modes.

**Decision:** `WorkspaceTransport` trait with `LocalTransport` and `CoderTransport` implementations.

**Consequences:** Zero breaking changes; new implementation is additive.

### DR-003: Workspace Lifecycle in NEXUS
**Context:** Need to create/destroy workspaces; could be separate agent or NEXUS.

**Decision:** Add workspace lifecycle to NEXUS `post()` method.

**Consequences:** No new agent; NEXUS gains `create_workspace` and `stop_workspace` tools.

### DR-004: Keep PocketFlow as Routing Engine
**Context:** Coder Agents has its own agent loop; could use that instead of PocketFlow.

**Decision:** OpenFlows uses PocketFlow for orchestration routing. Coder's agent loop handles workspace tool execution.

**Consequences:** OpenFlows retains multi-agent coordination advantage; Coder's single-agent loop handles workspace operations.

### DR-005: Planning Gate Implementation
**Context:** FORGE must wait for SENTINEL approval before implementation.

**Decision:** 
- Harness CLI enforces gate transitions (cannot skip `planning`)
- NEXUS spawns SENTINEL when detecting `planning` phase
- SENTINEL runs `openflows-harness gate approve --phase planning`
- Gate approval is atomic single-use (GETDEL)

**Consequences:** Guaranteed plan review before any implementation begins.

---

## 13. Open Questions

1. **MCP tool registration:** Should integration be purely API, or also include MCP servers? *(Leaning: both — Coder MCP for user-facing interactions, control plane API for agent-internal operations.)*

2. **Chat model for FORGE/SENTINEL:** Should they use Coder's built-in agent loop for workspace operations? *(Leaning: OpenFlows drives prompts via dispatch payload, Coder executes workspace tools.)*

3. **Billing and cost attribution:** How to attribute LLM costs for multi-agent flows? *(Leaning: Per-flow cost with breakdown per FORGE-SENTINEL pair, attributed to initiating user.)*

4. **Template selection:** User choice or NEXUS automatic? *(Leaning: NEXUS selects based on ticket type/labels, with user override.)*

5. **Multi-tenant isolation:** Isolated SharedStore namespaces? *(Leaning: Yes — `flow:{user_id}:{flow_id}:` prefix on all keys.)*

---

## 14. Summary

OpenFlows and Coder are complementary systems combining **orchestration intelligence** with **infrastructure governance**:

| What | Coder | OpenFlows |
|------|-------|-----------|
| **Where agents run** | ✅ Workspace templates, identity, network isolation | — |
| **How agents coordinate** | — | ✅ Flow graphs, typed contracts, multi-agent coordination |
| **What they do** | — | ✅ FORGE builds, SENTINEL reviews, VESSEL merges, LORE documents |
| **Failure recovery** | Workspace health monitoring | NEXUS reconciliation, gate approvals |

Together they form a complete enterprise AI development platform: **architecture-first orchestration running on governed, isolated, auditable infrastructure.**