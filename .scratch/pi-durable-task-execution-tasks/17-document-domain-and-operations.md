# T17 — Complete domain records and executable operational documentation

## Delivered outcome

The shipped documentation explains setup, migration, supervision/recovery limits and all operator controls accurately enough to verify against the finished CLI/RPC.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC17–23 and Domain changes. Build on T16. Apply proposed ADR-0012 and remaining Forge Workspace/Forge Dispatch Workspace glossary changes in the actual runtime context. Review previously applied lease/recovery, Check/Profile and ADR-0013/0014 changes for consistency; check current ADR numbering before creating files.

Update dispatch, Assignment recovery/context, profile policy, workspaces, storage structure and Revision installation/development documentation. Include supported authentication/resource matrix, explicit pin migration, read-only transcript pagination, follow-ups, missing Outcome, lease expiry/outage behavior, interrupted-effect inspection, retirement, runtime retention/deletion and rollout/rollback. Clearly distinguish configuration readiness, authorization, liveness and verified absence.

Document the trusted-local-host limit: same-user shell tools are not adversarially contained, escaped processes may require investigation and persistence/leases do not establish exactly-once effects or death. Remove Herdr-only Task prerequisites only where durable-specific behavior replaces them, preserving Herdr/Grilling guidance.

## Acceptance checks

- Every proposed domain block/ADR is applied once or explicitly reconciled with a documented reason; no competing root glossary appears.
- Examples match actual parser/RPC contracts and redact sensitive values.
- No instruction uses hold clearance, transcript inspection or revocation as proof of safe replacement.
- Runtime/dependency installation, support limits and release verification requirements are explicit.

## Verification and evidence

Run documented-command/parser tests and execute examples against isolated controlled fixtures. If shipped skills change, apply `writing-for-agents` and run documented-skill/portability gates. Deliver a documentation-to-command checklist with observed results; no live profile migration or rollout.

## Dependency

Depends on T16. Code Stack predecessor: T16. This is the final planned implementation/documentation Stack member before review and integration verification.
