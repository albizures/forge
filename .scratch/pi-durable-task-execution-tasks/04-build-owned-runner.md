# T04 — Build a single-owner persistent runner process

## Delivered outcome

A real TypeScript runner can own one protected SQLite store and one conversation, admit an idempotent test submission and expose committed state without a Pi CLI process.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC4–6, AC14. Build on T03 and use T01's exact upstream/runtime contract. Keep production dispatch disabled and use controlled fake model/tools until authorization and profile integration land.

Implement version negotiation, stable admission identities, process identity and ownership handshake. Establish exclusive ownership before writable storage open or scheduler startup; a second process cannot repair or execute another owner's store. Place storage outside the Managed Worktree with owner-only access. Associate one runner, conversation and store with one Assignment. Expose committed read-only observations without scheduling; full CLI pagination comes later.

## Acceptance checks

- Concurrent starts produce one writable owner and one intended submission.
- Abrupt restart retains conversation identity and committed content without creating another Assignment.
- Wrong ownership, unknown protocol and conflicting operation IDs fail explicitly.
- A fake model run in a second Assignment survives the first runner crashing.
- No process is adopted or terminated merely by matching a PID or label.

## Verification and evidence

Add real-process integration tests with isolated directories and explicit synchronization: start races, interrupted handshake, stale ownership evidence, read-only observation beside a writer and independent runner failure. Capture runtime/dependency identity and commands. Tests must not require developer Pi configuration or real credentials.

## Dependency

Depends on T03. Code Stack predecessor: T03.
