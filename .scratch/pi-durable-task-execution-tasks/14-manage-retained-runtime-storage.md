# T14 — Retain protected runtime history and delete it only after safe cleanup

## Delivered outcome

Retirement keeps readable protected history; an explicit CLI/RPC operation can delete eligible runtime storage without erasing Forge's audit or recreating execution.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC6, AC10, AC14, AC16. Build on T13. Runtime files contain sensitive repository content even if credentials are redacted. They are not worktrees and are not Forge's accepted Outcome/audit database.

Enforce owner-only runtime directory/file access and preserve stores by default on retirement. Add explicit deletion with scope/identity checks, canonical path containment and exclusive ownership. Reject live or unresolved execution, uncertain cleanup, redirected paths and incompatible ownership. Handle SQLite-associated files and partial deletion conservatively; retain a diagnostic rather than pretending success. Deletion must not start a runner, synthesize an empty replacement store or make historical operation replay executable again.

## Acceptance checks

- Retired history remains inspectable with protected permissions.
- Active/unresolved or wrong-owner deletion is rejected before destructive work.
- Eligible deletion removes only the bound runtime history.
- Assignment, Outcomes, Assessments, migration/recovery audit and accepted operation facts remain intact.
- Inspection after deletion reports unavailable/purged runtime history without resurrection.

## Verification and evidence

Test stopped and active stores, symlink/path escapes, wrong identity, concurrent reader/writer races, partial filesystem failure and exact retry. Compare Forge audit records before/after deletion and verify unchanged Managed Worktree contents. Document the retention/deletion boundary and record commands/results.

## Dependency

Depends on T13. Code Stack predecessor: T13.
