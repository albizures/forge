# T03 — Persist durable execution identity and idempotent admission intent

## Delivered outcome

Forge can reserve and reconcile a durable Assignment's runtime identity without admitting duplicate work across interrupted protocol calls.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC4–7, AC12. Build on T02. Route through app Assignment/dispatch policy, store transactional enforcement and the versioned runner wire contract selected in T01.

Persist backend, runtime/conversation/store identity, launch intent and operation results sufficient to distinguish pending, accepted, rejected and unknown outcomes. Define typed operations and compatibility negotiation for launch, observation, recovery, follow-ups and retirement; later Tasks implement their behavior. Scope authorization before replay, return exact accepted results and reject conflicting operation reuse. Keep external process/network calls outside writer transactions and revalidate dependent state.

Follow the repository's actual schema policy: advance incompatible storage contracts and reject incompatible stores before initialization writes; do not invent legacy runtime identities or silently convert stores.

## Acceptance checks

- Duplicate admission requests identify one Assignment runtime; conflicting retries reject.
- Launch uncertainty and closed unresolved Assignments reserve capacity across reopen.
- Historical backend identities remain unchanged by current configuration.
- Store/wire versions and structured diagnostics are explicit.

## Verification and evidence

Use fake transport responses and failures around intent commit, request send, remote admission and local result commit. Assert persisted state after restart, concurrent claims, replay results and incompatible-store rejection. Update store contract documentation. No real production runner launch is required.

## Implementation evidence

Completed locally. See [T03 implementation and verification](../../docs/research/pi-durable-t03/README.md)
for concrete interfaces, failure/restart/concurrency tests, schema 20 and wire v1
compatibility, and successor boundaries. All four repository gates pass: 577 tests
passed, 1 opt-in probe ignored. Durable launching remains disabled; no live Forge
Records, production runner or release readiness is claimed.

## Dependency

Depends on T02. Code Stack predecessor: T02.
