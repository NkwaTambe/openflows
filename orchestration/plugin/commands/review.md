---
name: review
description: Record a SENTINEL decision for a specific lifecycle review round
---

# /review

Read `openflows-harness status get` and `plan read`. Use the returned `revision`,
`review_round` and `head` from the artifact you actually review, not from a later
snapshot obtained merely to make an outdated command succeed. Write a report.

For a submitted plan:
```bash
openflows-harness gate decide --phase plan_ready --revision <N> --round <R> --verdict approve --report review.md
```
For testing, verify implementation against the approved plan and run tests through
A2A in FORGE's clean checkout (FORGE runs `verify serve`):
```bash
openflows-harness verify request --expect-exit 0 --argv <program> --argv <argument>
openflows-harness gate decide --phase testing --revision <N> --round <R> --head <SHA> --verdict approve --report review.md
```
For a recorded PR in submit:
```bash
openflows-harness review submit --revision <N> --round <R> --head <SHA> --verdict approve --report final-review.md
```
Use `--verdict reject` with actionable feedback for any failed review. Rejected
plans return to plan_rejected; testing/PR rejection returns to building. The
controller retries delivery of PR decisions to GitHub from a persisted queue.
Do not substitute a GitHub-only review for the lifecycle command.

SENTINEL does not grant human approval. Humans independently approve testing and
submit through `openflows gate decide`; CI must succeed for the submitted head.
