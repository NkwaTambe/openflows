# OpenFlows × Coder: Integrated Architecture

**Design Document v1.0**  
**Status:** Draft

---

## 1. Core Thesis

**Coder governs WHERE agents run. OpenFlows governs HOW agents coordinate.** Together they form a complete platform.

This is a platform layer (**Coder**) + application layer (**OpenFlows**) relationship. Coder provides secure, governed workspace infrastructure with identity, audit, and network isolation. OpenFlows provides architectural intelligence — flow graphs, typed contracts, multi-agent coordination, and self-healing reconciliation. Neither duplicates the other’s core competency.

| Layer | Coder | OpenFlows |
| :--- | :--- | :--- |
| **What it solves** | Where agents run safely | How agents coordinate intelligently |
| **Core primitive** | Terraform workspace templates | PocketFlow flow graph + Node trait |
| **Governance model** | Infrastructure-first — govern execution environment | Architecture-first — plan before execute |
| **Failure handling** | Workspace isolation + identity tracing | NEXUS reconcile() + flow recovery |
| **Agent model** | Single agent per workspace (spawn_agent for sub-tasks) | Differentiated agents: NEXUS, FORGE, SENTINEL, VESSEL, LORE |
| **State management** | Chat persistence in database | SharedStore (Redis/in-memory) |
| **Orchestration** | Sequential agent loop with tool calls | Multi-agent coordinated via action-routing flow graph |

---

## 2. Architecture Overview

The architecture consists of the **Coder Control Plane** managing LLM providers and infrastructure, and the **OpenFlows Orchestration Engine** managing the agent coordination flow.

### Coder Control Plane
*   **LLM Providers:** Anthropic, OpenAI, Google, Azure, AWS Bedrock, Custom.
*   **OpenFlows Orchestration Engine:** 
    *   **NEXUS (mind)** → **PocketFlow (routing table)** → **VESSEL (merge)**.
    *   **FORGE-SENTINEL Pairs** (Pair-1, Pair-2, Pair-N) connected to a **SharedStore (Redis)** containing tickets, workers, PRs, and events.
*   **Coder Infrastructure Layer:** Template Registry, Identity (SSO), Audit Log, MCP Config, Git Auth, Model Governance, Usage Analytics, Cost Controls.
*   **Coder Tailnet:** (DERP relay / P2P) and **Coder Workspace Daemon** (file I/O, shell, processes).

### Coder Workspaces (Network Isolated)
Each pair (e.g., Forge-1 + Sentinel) runs in an isolated workspace with:
*   Git checkout (/src, /tests).
*   **No API keys** and **No agent software** inside the workspace.
*   **Egress:** Restricted to git provider + control plane.

---

## 3. Integration Layers

The integration happens at five distinct layers, each preserving the independence of both systems.

### Layer 1: Workspace as Execution Substrate
OpenFlows pair harness provisions Coder workspaces instead of local git worktrees.

| OpenFlows Concept | Coder Equivalent |
| :--- | :--- |
| Git worktree `worktrees/pair-N/` | Coder workspace (template-based) |
| `git worktree add` | `create_workspace` (Coder API) |
| Local process spawn (CLI) | Agent loop → workspace daemon tool calls |
| `pair-N/shared/STATUS.json` | SharedStore (Redis) or workspace file via `write_file` |
| File lock directory | Coder workspace isolation (pair-1 can’t access pair-2) |

**Key benefit:** Network isolation per workspace. Agent workspaces can be locked down to only reach the git provider. No LLM API keys ever enter the workspace.

### Layer 2: PocketFlow-in-Coder (Agent Loop Replacement)
The OpenFlows NEXUS agent loop replaces the Coder Agents agent loop as the routing engine, while using Coder’s workspace connection infrastructure.

*   **PocketFlow Node trait** (Rust): Defines `name`, `prep`, `exec`, and `post` methods.
*   **CoderTransport**: Executes tool calls (read/write file, edit, execute, workspace management) via Coder workspace daemon.

### Layer 3: Identity and Governance Bridge
Every OpenFlows agent action inherits the submitting user’s Coder identity.

| Property | OpenFlows Standalone | OpenFlows + Coder |
| :--- | :--- | :--- |
| **API key management** | Per-agent env vars, in worktree | Control plane only, zero workspace exposure |
| **User identity** | GitHub PAT (shared) | Coder SSO identity per action |
| **Network isolation** | None (agents need network) | Workspace egress restricted to git provider + control plane |
| **Audit logging** | Event ring buffer in SharedStore | Coder audit log + SharedStore events |
| **Model governance** | Per-agent .env config | Centralized Coder admin panel |
| **Template governance** | N/A | Admin-controlled workspace templates |

### Layer 4: MCP Tool Bridge
Coder’s workspace management becomes MCP tools available to OpenFlows agents. New tools include `coder_create_workspace`, `coder_start_workspace`, `coder_stop_workspace`, `coder_read_file`, `coder_write_file`, `coder_execute`, and `coder_list_templates`.

### Layer 5: Hybrid Deployment Model

| Mode | Description | Use Case |
| :--- | :--- | :--- |
| **OpenFlows Standalone** | Local worktrees, local agents, SharedStore | Individual developers, small teams, OS contributors |
| **Coder + OpenFlows Integrated** | Coder workspaces + OpenFlows orchestration | Enterprises, regulated industries, teams needing governance |
| **Coder Only** | Coder Agents without OpenFlows orchestration | Teams wanting workspace governance without multi-agent flows |

---

## 4. Detailed Component Mapping

### 4.1 NEXUS (Orchestrator)
NEXUS runs inside the Coder control plane as part of the OpenFlows orchestration engine. It gains `list_templates` and `read_template` capabilities to select workspace templates.

---

## 6. Security and Isolation

| Threat | OpenFlows Only | OpenFlows + Coder |
| :--- | :--- | :--- |
| **API key exfiltration** | PAT in workspace (extractable) | Keys in control plane only |
| **Cross-pair access** | File locks (flock) | Workspace isolation (kernel-level) |
| **Unauthorized commands** | Hook scripts (bypassable) | Coder template policy + hooks |
| **Network exfiltration** | No restriction | Egress firewall per template |
| **Audit trail** | SharedStore events (volatile) | Coder audit log + SharedStore events |
| **User attribution** | GitHub PAT per bot | Per-user Coder SSO identity |
| **Secret scanning** | Git hooks on push | Coder template policy + git hooks |

---

## 7. Flow Graph (Updated for Coder)

The PocketFlow flow graph remains the core routing mechanism. Workspace lifecycle actions (create, stop, destroy) are now Coder API calls made during the Node’s `exec()` phase.

---

## 8. Context Compaction and Chat Persistence

*   **Existing HANDOFF.md**: Continues to work for hard resets (session kills, process restarts).
*   **Coder Auto-compaction**: Serves as a softer, more granular context management layer.
*   **Chat Persistence**: Full conversation history survives workspace stops and rebuilds.

---

## 9. Plan Mode Integration

Coder Agents' **Plan Mode** maps directly to the existing FORGE-SENTINEL plan review cycle:
*   **FORGE writes PLAN.md** ↔ `propose_plan`
*   **FORGE asks SENTINEL** ↔ `ask_user_question`
*   **SENTINEL writes CONTRACT.md** ↔ Plan review
*   **FORGE begins implementation** ↔ "Implement plan"

---

## 10. Implementation Roadmap

1.  **Phase 1: CoderTransport (Non-Breaking)**: Add `CoderTransport` abstraction to execute workspace operations via Coder's API.
2.  **Phase 2: SharedStore Migration for Pair State**: Move pair communication artifacts from filesystem (`shared/`) to SharedStore (Redis).
3.  **Phase 3: Coder Provisioner**: Full Coder workspace lifecycle management.
4.  **Phase 4: Governance Integration**: Coder’s admin panel governs OpenFlows agent behavior (LLM providers, system prompts, audit, costs).
5.  **Phase 5: MCP Bridge**: Register OpenFlows orchestration tools as Coder MCP servers.

---

## 11. Failure Recovery Under Coder

Coder significantly improves recovery. If a workspace crashes, NEXUS can call `start_workspace` to resume or create a new workspace and continue from the last checkpoint stored in SharedStore.

---

## 12. Comparison: Standalone vs Integrated

| Feature | OpenFlows Standalone | OpenFlows + Coder |
| :--- | :--- | :--- |
| **Provisioning** | Local git worktrees | Coder workspace templates |
| **Agent execution** | CLI processes | Coder workspace daemon tool calls |
| **Isolation** | File locking (flock) | Kernel-level workspace isolation |
| **User identity** | Shared GitHub PAT | Per-user Coder SSO |
| **Scalability** | Limited by local resources | Terraform-defined compute per workspace |

---

## 13. Decision Record

*   **DR-001**: SharedStore over filesystem for pair state to enable cross-workspace access.
*   **DR-002**: `CoderTransport` as an abstraction (trait) to ensure zero breaking changes.
*   **DR-003**: Workspace lifecycle managed in NEXUS for natural flow orchestration.
*   **DR-004**: Keep PocketFlow as the routing engine to retain multi-agent coordination advantages.

---

## 14. Open Questions

1.  **MCP tool registration**: Should integration be purely API or include MCP? (Leaning: both).
2.  **Chat model for FORGE/SENTINEL**: Should they use Coder's built-in loop? (Leaning: OpenFlows drives prompts, Coder executes tools).
3.  **Billing**: How to attribute costs for multi-agent flows? (Leaning: Per-flow cost).
4.  **Template selection**: User-chosen or NEXUS-selected? (Leaning: NEXUS selected with user override).
5.  **Multi-tenant isolation**: Isolated SharedStore namespaces? (Leaning: Yes).

---

## 15. Summary

OpenFlows and Coder are complementary. Coder governs **where** agents run, while OpenFlows governs **how** they coordinate. Together, they form a complete enterprise AI development platform combining orchestration intelligence with robust infrastructure governance.
