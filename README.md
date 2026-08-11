# OpenFlows — Autonomous AI Development Team on Coder
<img src="image-1.png" alt="OpenFlows demo" style="width: 100%; max-width: 1200px; height: auto; display: block; margin: 0 auto;">

> Official site: [openflows.dev](https://openflows.dev)

**OpenFlows is an autonomous software development team orchestrator that runs on your self-hosted Coder deployment.**

Give it a GitHub repo and some issues, and OpenFlows orchestrates a team of coordinated AI agents that plan the work, write the code, review it adversarially, and ship reviewed PRs — without you writing a single line of code. Each agent runs as a **Coder Agent** (control-plane AI loop) operating on an ephemeral, governed Coder workspace, with LLM keys kept in the Coder control plane and every action tied to your identity.

> **Getting started?** All setup, startup, and troubleshooting steps live in [**QUICK_START.md**](QUICK_START.md). The rest of this README is an overview of what the project is, how it works, how far it has come, and what's left.

## Why architecture-first

AI can generate code against a spec, but it can't write the spec. As models make boilerplate cheap, the real difficulty shifts *up the stack* — into architectural thinking, product judgment, and security awareness. OpenFlows encodes that discipline: a declared flow graph (PocketFlow), typed SharedStore state contracts, an explicit routing table, and recovery built into every step. **Engineering goes in, software comes out.** See [`docs/architecture/OpenFlows_Coder_Integrated_Architecture.md`](docs/architecture/OpenFlows_Coder_Integrated_Architecture.md) for the full design.

## How It Works

OpenFlows runs a team of AI agents that collaborate just like a real engineering team:

```
You create a GitHub issue → NEXUS picks it up → FORGE writes code → SENTINEL reviews adversarially
→ VESSEL merges green PRs → LORE documents → you get a merged PR
```

You stay in the loop only when needed — security concerns, ambiguous specs, or major decisions. Otherwise, the team runs autonomously, with NEXUS's `reconcile()` detecting orphans, stale workers, and unmerged PRs and recovering automatically.

### Gated Planning Approval

OpenFlows enforces a critical checkpoint before FORGE begins implementation:

```
FORGE writes plan (PLAN.md) → Sets status to 'planning' → HALTS
                             ↓
                    SENTINEL reviews plan
                             ↓
           SENTINEL approves: openflows gate approve --phase planning
                             ↓
        Gate unlock recorded in Redis → FORGE receives approval
                             ↓
        FORGE transitions status to 'building' → Implementation begins
```

**Key behaviors:**
- When FORGE attempts to transition from `planning` to `building`, the system checks for gate approval
- Without SENTINEL's explicit approval, FORGE receives error: `"Cannot transition from 'planning' to 'building' without SENTINEL approval"`
- Gate approval is stored with timestamp and approver role, enabling audit trails
- Only the `planning` → `building` transition is gated; subsequent phases are unconstrained

This ensures every issue is carefully planned before coding begins, catching scope creep and spec mismatches early.

### Coder governs *where* agents run — OpenFlows governs *how* they coordinate

The integration is deliberate and asymmetrical:
- **Coder** provides the governed environment: ephemeral workspaces, control-plane AI agents, model governance, identity, audit logging, cost tracking. The workspace has zero AI software and zero LLM keys.
- **OpenFlows** provides the brain: the flow graph, typed SharedStore contracts, the Node trait's `prep → exec → post` separation, and the FORGE↔SENTINEL planning cycle.

Coder Agents run in the **control plane** (not in workspaces). They execute tool calls by connecting to workspaces over the same secure tunnel as IDEs. You watch agents coding live in the Coder Agents chat UI with diffs, status, and message streaming.

### The `openflows-harness` CLI

Each worker workspace gets a small `openflows-harness` binary. The Coder Agent invokes it via shell (guided by skills) to read/write the Redis SharedStore with typed, validated schemas. Agents never run `redis-cli` directly — the harness is the only Redis client in a workspace.

## The Team

| Agent | Role | Plan mode | What it does |
|-------|------|-----------|--------------|
| **NEXUS** | Orchestrator | yes | Assigns issues, coordinates the team, owns `reconcile()` failure recovery, notifies you when needed |
| **FORGE** | Builder | no | Writes code against an agreed `CONTRACT.md`, creates branches, opens PRs |
| **SENTINEL** | Reviewer | yes | Adversarially reviews code for security, quality, and test coverage against the contract |
| **VESSEL** | DevOps | no | Monitors CI, handles merge conflicts, squash-merges green PRs, tears down workspaces on merge |
| **LORE** | Writer | no | Documents decisions, updates changelogs, maintains project history *(disabled by default — enable in the registry)* |

## Multi-Tenancy

One Coder server serves many teams. Each tenant = a real Coder user + a repo binding + an `openflows-nexus` workspace. Tenants are isolated by Coder RBAC and per-tenant Redis keyspace prefixes (`ns:{tenant}:...`).

Configure multiple tenants via environment variables or the control plane API (documented in `docs/`).

## Project Status

OpenFlows is an actively developed, functioning system that already ships merged PRs end-to-end. Current state (v1.2.x):

**Working today:**
- A full agent team (NEXUS / FORGE / SENTINEL / VESSEL / LORE) running as Coder Agents on ephemeral, governed workspaces.
- End-to-end flow: GitHub issue → planning gate → FORGE implementation → SENTINEL adversarial review → VESSEL merge → merged PR.
- Gated planning approval with audit-trailed gate records in Redis.
- `reconcile()` failure recovery: orphan / stale worker detection, retry with backoff, and unmerged-PR resume.
- Typed SharedStore contracts and the `openflows-harness` CLI as the only Redis client inside workspaces.
- Multi-tenancy via per-tenant Redis keyspace prefixes and Coder RBAC.
- Production controller deployment inside a Nexus workspace (auto-start via startup script).
- A pluggable skill / MCP / model registry.

See [QUICK_START.md](QUICK_START.md) to run it, and the planning-gate / architecture docs for the intended end state.

## Plug-and-Play Extension

- **Add a skill**: Drop a directory in `orchestration/plugin/skills/` with a `SKILL.md`, list it in `registry.json` under the role's `skills` array. No code change.
- **Add an MCP server**: Add it to the role's `mcp` object in `registry.json`, or register it centrally in the Coder dashboard (AI Settings → MCP Servers). Both coexist.
- **Enable a new model**: Configure it in the Coder dashboard (AI Settings → Coder Agents → Models). Reference it in `registry.json` via the `model` field.

See [`docs/extending.md`](docs/extending.md) for details.

## Documentation

| Guide | What it covers |
|-------|---------------|
| [QUICK_START.md](QUICK_START.md) | Complete setup, startup, and troubleshooting |
| [TOKEN_GUIDE.md](TOKEN_GUIDE.md) | Token acquisition step-by-step |
| [TESTING_QUICK_START.md](TESTING_QUICK_START.md) | Testing & debugging walkthrough |
| [docs/coder-compatibility.md](docs/coder-compatibility.md) | Coder version compatibility and verification |
| [docs/tenancy.md](docs/tenancy.md) | Multi-tenant model and Redis namespacing |
| [docs/governance.md](docs/governance.md) | AI governance controls and network policy |
| [docs/extending.md](docs/extending.md) | Adding skills, MCP servers, and models |

## License

MIT
