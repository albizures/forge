# T16 — Gate mixed-backend scheduling and safe rollback

## Delivered outcome

The completed backend can advance controlled Task Check sequences through normal scheduling while rollout/rollback preserves original Assignment ownership and Herdr Grilling operation.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC1, AC2, AC4, AC7, AC11–13, AC20–22. Build on T15. Implement release/default eligibility and new-launch controls without treating a local demo or a runtime done event as rollout authorization.

Exercise explicit and automatic eligible Task dispatch, accepted Implementation output to independent Review, review handback, deliberate Continuation and stacked publication/unchanged-result rules. Every successor waits for accepted Forge evidence and safe retirement/reconciliation; original runtime identity remains authoritative for cleanup.

Keep durable default dispatch release-gated until T19's retained verification and T20 handoff. Provide tested controls to disable new durable admissions without moving existing Assignments to Herdr, rewriting pins or disabling their authorized inspection/recovery/retirement. Existing Herdr work and Grilling retain their prerequisites/guards. Same-backend safe recovery of an existing Assignment is not a new cross-backend fallback.

## Acceptance checks

- Controlled full Check sequencing respects exact evidence, freshness, Findings, capacity and retirement.
- Missing Outcome or incomplete retirement prevents the next Check.
- Mixed-backend configuration changes do not redirect existing Assignments.
- Rollback prevents new durable launches and leaves historical operations usable.
- Default eligibility remains withheld pending verification evidence.

## Verification and evidence

Use deterministic end-to-end fixtures for automatic scheduling, review handback, Continuation, unchanged publication and dependency/Spec gates. Toggle defaults/launch controls with live and closed-uncertain Assignments. Run retained Herdr/Grilling regressions and store restart tests; record explicit no-fallback evidence.

## Dependency

Depends on T15. Code Stack predecessor: T15.
