# T18 — Review the complete implementation against the Spec and coding standards

## Delivered outcome

An independent review report traces delivered behavior to all Spec criteria and identifies any blockers before release verification.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC1–23. Build on T17's completed implementation/documentation, not an earlier happy-path snapshot. Use `review-implementation` and `review-coding-standards`; this is a review deliverable, not a new feature slice.

Inspect domain ownership, exact retries/atomic history, host ownership before storage open, every model/tool authorization path, unsafe-effect blocking before resumed generation, process identity/termination ordering, closed-uncertain capacity, read-only observation, effective-pin history, matched runtime distribution and rollback. Audit supported authentication claims, secret exposure paths and the actual scope of subprocess guarantees. Reconcile all proposed domain updates against delivered documents.

## Acceptance checks

- Report maps every AC to concrete implementation/tests and identifies missing coverage.
- Standards findings cite rule, code location and concrete consequence.
- Review distinguishes mock tests, real-process tests and still-unrun real-provider gates.
- No unresolved safety/contract blocker is described as acceptable release evidence.

## Verification and evidence

Deliver a dated, revision-identified review report with findings, severity, required remediation and the reviewed diff. Do not dispatch work, publish PRs or alter operator configuration. If defects require code changes, stop advancement until corrections satisfy the responsible implementation deliverable and the changed revision is reviewed again; the final verification must identify that corrected snapshot. Do not silently add Tasks or redesign accepted policy.

## Dependency

Depends on T26. The first review pass (2026-11-20, snapshot `1fc65a5`) reviewed T17 and found blockers; the corrective Tasks T21–T24 fixed them. The second pass (2026-11-21, snapshot `646abd9`) reviewed T24 and found blockers B3 and B4; the corrective Tasks T25–T26 address them. This Task now re-reviews the corrected snapshot after T26 (third pass). Non-code review Task; ordinary dependency only, not a new code Stack member. All downstream work consumes the reviewed final implementation snapshot.
