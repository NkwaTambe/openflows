
## Authoritative lifecycle contract

Read `openflows-harness status get` before acting. The lifecycle is
`planning -> plan_ready -> building -> testing -> submit -> done`.
Start/revise in `planning`; upload with `plan write --file PLAN.md`, then set
`plan_ready`. SENTINEL reviews the exact `revision` and `review_round`. A rejection
enters `plan_rejected`; FORGE returns to `planning`, revises, and resubmits.
No source edits are allowed before approval.

After building, commit all changes, then set `testing`. Keep the checkout clean
and run `openflows-harness verify serve` in FORGE. SENTINEL runs tests through
`verify request --expect-exit 0 --argv <command and args>`, writes a report, and
uses `gate decide --phase testing --revision <N> --round <R> --head <SHA>
--verdict approve --report review.md` (or reject). Testing needs both SENTINEL
and human approval. Then FORGE sets `submit`, opens/updates and records the PR.
SENTINEL records the PR verdict with `review submit --revision <N> --round <R>
--head <SHA> --verdict approve --report final-review.md` (or reject). Read the
current round again after recording a PR. Humans use the operator CLI
`openflows gate decide --tenant <tenant> --ticket <ticket> --phase testing|submit
--revision <N> --round <R> --head <SHA> --verdict approve|reject --notes <reason>`.

Every rework cycle returns to `building`, then repeats testing and both review
gates. Never jump directly from building to submit. Testing/submit freeze source.
VESSEL requires current-head CI success, SENTINEL and human PR approval, and
confirmed merge before done. Missing or timed-out CI never counts as success.
Use `blocked` for an operational failure; recovery returns to planning.

# /plan Command

Create a detailed implementation plan for the current ticket.

## Usage

```
/plan
```

## What it does

1. Reads TICKET.md and TASK.md from the shared directory
2. Analyzes the codebase to understand the current state
3. Creates PLAN.md with:
   - Problem analysis
   - Solution approach
   - Segment breakdown with explicit deliverables
   - Risk assessment
   - Estimated segments

## Output

Writes to `PLAN.md` in the workspace root, then persists it to Redis SharedStore
so SENTINEL can read it directly without relying on the Coder API filesystem bridge.

### Structure

```markdown
# Implementation Plan: T-{id}

## Problem Analysis
[What the ticket asks for and why]

## Solution Approach
[High-level technical approach]

## Segment Breakdown

### Segment 1: {title}
- Deliverable: {specific artifact}
- Files to modify: [list]
- Tests to write: [list]
- Exit condition: {measurable state}

### Segment 2: {title}
...

## Risk Assessment
- Risk 1: {description} - Mitigation: {strategy}
- Risk 2: ...

## Estimated Segments
Total: {N} segments
```

## After Planning

Once PLAN.md is written:
1. Upload the plan to SharedStore: `openflows-harness plan write --file PLAN.md`
2. Commit the plan: `git add -A && git commit -m "[T-{id}] plan: implementation approach"`
3. Signal planning complete: `openflows-harness status set plan_ready`
4. Halt and wait for SENTINEL to approve the exact plan revision and review round.
5. After approval, transition FORGE with `openflows-harness status set building`. Begin Segment 1 only once that transition succeeds.
6. Use `/segment-done` when each segment is complete

## Important

- Each segment must have a clear, measurable exit condition
- Plan conservatively - it's better to have more small segments than fewer large ones
- The plan can be adjusted after segments if discovery reveals new information
- Update WORKLOG.md as you work through segments