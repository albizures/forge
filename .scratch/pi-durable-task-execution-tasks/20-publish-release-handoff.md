# T20 — Deliver the verified rollout and rollback handoff

## Delivered outcome

Operators receive a revision-specific readiness decision and release handoff that enables deliberate adoption without migrating live Assignments or assuming production authority.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC19–23. Build on T19's complete verification evidence. This is a handoff/release-evidence Task, not implementation, live rollout, profile registration or dispatch administration.

Assemble matched installation artifacts/manifest references, compatibility matrix, required explicit profile/template/pin selection steps, expected operator diagnostics and the release-gate evidence. Document a staged new-Assignment rollout and rollback procedure: old Herdr Assignments retain Herdr cleanup, Grilling stays on Herdr, and old durable Assignments retain durable recovery/retirement even when new launches are disabled. Include retained runtime/storage compatibility and transcript retention/deletion limits.

State exactly which Revision/dependency set passed, which operator actions remain required and what conditions revoke readiness. Do not describe registration as selection, selection as dispatchability or publication as automatic start. No Task execution, PR merge, release publication or live configuration change is authorized by the filesystem plan itself.

## Acceptance checks

- Handoff points to the reviewed/tested snapshot, packaged components and per-AC report.
- Any failed/unrun release gate produces a withheld-readiness result, not conditional success hidden in prose.
- Rollout/rollback instructions never redirect a live/uncertain Assignment or silently reinterpret pins.
- Renewed verification requirements for later implementation/dependency changes are explicit.

## Verification and evidence

Check every handoff command/reference against T17 documentation and T19 evidence. If handoff preparation changes implementation, parser behavior or packaged runtime contents, return to the applicable review/verification gates rather than reuse stale evidence. Deliver a concise operator checklist and readiness statement; stop without performing rollout.

## Dependency

Depends on T19. Non-code handoff Task; ordinary dependency only. This is the final Task in the filesystem plan.
