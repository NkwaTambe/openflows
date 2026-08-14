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
3. Signal planning complete: `openflows-harness status set planning`
4. Begin Segment 1
5. Use `/segment-done` when each segment is complete

## Important

- Each segment must have a clear, measurable exit condition
- Plan conservatively - it's better to have more small segments than fewer large ones
- The plan can be adjusted after segments if discovery reveals new information
- Update WORKLOG.md as you work through segments