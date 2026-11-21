# T09 — Retire durable Assignments with conservative process cleanup

## Delivered outcome

The existing operator retirement path revokes a durable Assignment before stopping its runner and supervised children, releasing capacity only after safe reconciliation.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC10–12, AC21. Build on T08. Integrate app/store retirement with runner cancellation and host process supervision. Follow SAFE-1 through SAFE-4 and retain original backend provenance for every cleanup request.

Durably revoke before external cleanup; verify retained ownership before destructive requests. Stop admission, cancel runtime execution, terminate the supervised process group and verify relevant cleanup. An escaped child, unknown launch result, unavailable runner, identity mismatch or uncertain cancellation keeps evidence and capacity. Repeating retirement of an already closed Assignment reconciles the original operation; it cannot adopt similarly named processes or bypass unknown-resource confirmation rules.

Complete the retirement portions of proposed ADR-0013. Do not delete worktrees, runtime history or an operator-owned Herdr Workspace.

## Acceptance checks

- Revocation precedes every termination request and is retained if cleanup fails.
- Successful mutation/abort is not treated as verified absence.
- Closed uncertain Assignments reserve capacity across restart.
- Repeated retirement is bounded, identity-safe and auditable.
- Herdr cleanup and Grilling routing continue through their original adapters.

## Verification and evidence

Run real-process tests for long-running children, delayed shutdown, surviving/escaped child evidence and process-identity mismatch. Inject protocol/storage failures between revocation, cancellation, termination and final reconciliation. Assert no replacement while unsafe and unchanged Git/runtime content. Record known supervision limits and focused results.

## Dependency

Depends on T08. Code Stack predecessor: T08.
