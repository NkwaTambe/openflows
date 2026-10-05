---
name: status
description: Read or advance the authoritative ticket lifecycle
---

# /status

Read `openflows-harness status get` before acting. It returns the phase, version,
plan revision, review round, candidate head, decisions, feedback and history.

## Lifecycle graph (authoritative)

```
                 ┌──────────────────────────────────────────────┐
                 │                                              │
   planning ──▶ plan_ready ──approve──▶ building ──▶ testing ──▶ submit ──▶ done
      ▲             │ reject               │  ▲       │  ▲        │   │
      │             ▼                      │  │       │  │        │   │
      │       plan_rejected                │  └──reject┘  │        │   │
      │             │                      │             │        │   │
      └─────────────┘                      └──reject──────┘        │   │
                                                                   │   │
   blocked ──────────────▶ planning ──────(recover)────────────────┘   │
      (any phase may enter blocked)            reject ──▶ building     │
                                                      (rework loop)    │
   done is terminal; no exit.
```

- **Entry into `blocked`**: any phase may move to `blocked` (`(_, blocked)` is
  always permitted). FORGE uses it for an external prerequisite it cannot
  resolve, with an exact, answerable unblock question. SENTINEL may set it from
  `testing` only for a genuinely external prerequisite (missing human approval,
  unreachable external service, credentials/secret unavailable to the project)
  — **not** for a repairable code/command/setup/toolchain failure. Workspaces
  are provisioned fresh and empty, so a missing executor toolchain (cargo,
  python, yaml, …) is FORGE's build environment to install/repair in `building`
  and must **reject into `building`**, not `blocked`.
- **Exit from `blocked`**: the only legal transition is `blocked → planning`,
  and only FORGE may make it. NEXUS re-awakens FORGE when a ticket is blocked so
  it can resume once the blocker clears. There is **no** direct `blocked →
  building`; recovery always returns through planning and a fresh plan approval.
- **Rework**: `testing → building` and `submit → building` are always permitted;
  rejections land the worker in `building` to fix under the approved plan, then
  repeat `testing` and both review gates. Never jump from `building` straight to
  `submit`.

## Phases

| Phase | Entry / exit |
|---|---|
| planning | Write the plan at the current chat-specific path; upload with `plan write --file <absolute-plan-path>` |
| plan_ready | Submit the uploaded plan; wait for SENTINEL approval |
| plan_rejected | Read feedback, set planning, revise/upload and resubmit |
| building | Implement the approved plan, commit changes |
| testing | Set with a clean checkout; run verify serve, await SENTINEL + human approval |
| submit | Set after testing approval; open/record PR, await SENTINEL + human approval + CI |
| done | Controller-only, confirmed merge; terminal |
| blocked | Record blocker; recover through planning (only FORGE may exit it) |

Use `openflows-harness status set <phase>` for permitted worker transitions.
A draft plan is not permission to edit source. Testing and submit freeze source;
return to building for fixes, then repeat testing and review. A plan change
requires returning to planning and getting a new approval. Skipped stages and
stale decisions are rejected. `review_ready` is no longer a phase.
