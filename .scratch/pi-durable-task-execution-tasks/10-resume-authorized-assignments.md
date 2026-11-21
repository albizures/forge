# T10 — Recover authorized execution without creating new attempts

## Delivered outcome

Forge can automatically resume a safely interrupted durable Assignment while rejecting revoked, stale, competing or effect-uncertain recovery.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC2, AC4–10, AC12, AC13. Build on T09's proven retirement/ownership behavior before enabling any automatic recovery path.

Wire daemon reconciliation and runner reopen through authorization, exclusive ownership, lease issuance and interrupted-effect gates. Preserve Assignment, Check attempt, profile, evidence and conversation identity for same-Assignment recovery. Current worktree/profile observations must follow existing freshness/retained-work rules without treating expected implementation progress as automatically adopted completion evidence. Source-profile preflight is not a historical cleanup prerequisite.

Demonstrate a deliberate accepted Implementation Continuation creating a fresh successor conversation only after retirement/reconciliation, with Forge-approved evidence and handoff. A missing Outcome remains an intervention state rather than a recovery-triggered successor.

Apply the Assignment Execution Recovery glossary proposal and complete ADR-0013's recovery distinction.

## Acceptance checks

- Safe restart resumes the same attempt/conversation with no duplicate admission.
- Revocation, expired authority, competing ownership, stale bindings and uncertain effects block all scheduling.
- Daemon and runner restart order cannot bypass gates.
- Continuation and independent Review successors receive fresh conversations and exact approved context.
- No dirty work is silently repaired, adopted or discarded.

## Verification and evidence

Exercise fake-clock/fake-model restart matrices plus real owned process death. Verify attempt counters, histories, conversation IDs, capacity and model/tool admissions. Run retained-work, Continuation and Check-attempt regressions. Save a reproducible same-Assignment recovery demo and blocked counterexamples.

## Dependency

Depends on T09. Code Stack predecessor: T09.
