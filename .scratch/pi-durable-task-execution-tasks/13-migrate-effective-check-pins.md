# T13 — Migrate eligible Check profile selections explicitly and auditably

## Delivered outcome

An operator can explicitly select durable-compatible profiles for eligible existing Checks without rewriting historical pins, attempts or evidence.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC2, AC17, AC20, AC21. Build on T12. Current Checks are documented as immutable; this Task implements the Spec's narrow migration exception, not generic Check editing.

Add app policy, transactional store history/effective selection and CLI/RPC operation. Require no active ownership, fully reconciled known/unknown resources, compatible profile scope/type and safe current Task state. Revalidate external observations when committing. Atomically record prior/new effective pins, migration identity and freshness invalidation; accepted replay returns the original result without advancing generation again. Conflicting reuse rejects. Preserve Check identity, description/order, historical original pins, existing attempt bindings and Assessments as history.

New attempts use effective explicit selections; changing a template or Workspace default alone does not migrate old Checks. Do not register profiles or migrate live operator Tasks as part of implementation.

Apply the Spec's Check and Agent Profile glossary replacements and proposed ADR-0014; update affected immutable-pin documentation and schema contracts.

## Acceptance checks

- Eligible migration yields a new durable next attempt only through ordinary gates.
- Active, uncertain, stale, cross-scope and incompatible migration reject atomically.
- Exact retry preserves accepted history/generation; conflicting retry rejects.
- Historical evidence and Assignment backend/profile identity remain unchanged.

## Verification and evidence

Test concurrency with claim/retirement, transaction failure, restart/replay, stale evidence and mixed profiles. Exercise CLI/RPC plus historical inspection and a controlled successor dispatch. Retain migration before/after evidence, schema compatibility results and documented command shapes.

## Dependency

Depends on T12. Code Stack predecessor: T12.
