# T12 — Deliver auditable idempotent operator follow-ups

## Delivered outcome

An operator can send recorded input to an authorized durable Assignment through CLI/RPC, including a missing-Outcome conversation, without rebinding its work.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC5, AC13, AC15. Build on T11. Reuse stable operation identities, authorization/lease gates and read-only observation; do not create a second message lifecycle or grant operators' authority through agent capability endpoints.

Record/deliver input durably with exact retry behavior and explicit unknown admission. Preserve Check, profile, attempt, generation and evidence binding. Requests that imply a scope/evidence change cannot silently reconfigure the conversation; ordinary invalidation/replacement remains necessary. Reject revoked or incompatible targets, and respect interrupted-effect/recovery blockers rather than allowing a message to resume unsafe work.

## Acceptance checks

- A dropped admission reply followed by retry creates one intended follow-up.
- Follow-up is visible in committed transcript/history with its operation identity.
- Revoked Assignments reject; uncertain execution does not bypass safety gates.
- A reviewer receiving 'use another commit' retains its original evidence binding.
- A missing-Outcome Assignment can receive explicit safe intervention without synthesized completion.

## Verification and evidence

Exercise CLI/RPC delivery, duplicates, conflicting reuse, reconnect/restart, revocation between authorization and admission, and input queued while an effect is unresolved. Assert immutable bound context and absence of unauthorized tool/model calls. Retain observable request/result examples and focused results.

## Dependency

Depends on T11. Code Stack predecessor: T11.
