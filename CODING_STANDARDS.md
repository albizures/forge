# Coding standards

These rules govern new and modified Rust code, tests, CLI/RPC contracts, and
shipped skills. Existing violations are not precedent; unrelated cleanup is not
required. Rule IDs are stable references for review findings.

For each change, apply every relevant rule below. A review finding must identify
the rule, affected code, and concrete consequence. An exception requires an
explicit rationale and verification of the alternative in the change description;
changes to safety policy require separate, explicit approval before implementation.

## Architecture and code structure

- **ARCH-1 — Policy ownership.** Keep domain types, policies, and application
  interfaces in `app`; SQLite persistence and transactional enforcement in `store`;
  wire contracts in `rpc`; request adaptation and dependency wiring in `server`;
  command parsing and presentation in `cli`. Adapters delegate business policy
  rather than independently redefining it. Defensive validation may repeat at
  distinct trust or consistency boundaries.
- **ARCH-2 — Integration boundaries.** Isolate Git, filesystem, process, and
  provider operations behind explicit interfaces so policy can be tested without
  those external systems. Keep Forgeup Revision selection and Managed Daemon
  lifecycle in `forgeup`, separate from Record execution policy.
- **ARCH-3 — Cohesion.** Place changes in a capability-focused module. Extract a
  meaningful capability when extending `lib.rs` would mix responsibilities;
  file length alone does not require extraction. Introduce abstractions for an
  actual boundary or demonstrated reuse, not hypothetical future consumers.
- **ARCH-4 — Dependencies.** Explain each added dependency's purpose and why
  existing workspace dependencies or standard-library facilities are insufficient.
  Keep unrelated refactors out of behavior changes.
- **ARCH-5 — Vocabulary.** Use the canonical terms in the contexts linked from
  [GLOSSARY-MAP.md](GLOSSARY-MAP.md). Resolve conflicting terminology explicitly;
  keep domain definitions in their context rather than copying them here.

## Rust and errors

- **RUST-1 — Explicit state.** Represent domain alternatives with typed values
  and exhaustive enums. Parse wire and stored strings at their boundaries; keep
  unknown or invalid values distinguishable from valid defaults.
- **RUST-2 — Operational errors.** Return errors for expected input, storage,
  network, process, and filesystem failures. Reserve production panics, including
  `unwrap` and `expect`, for established invariants with a locally evident reason.
  Test assertions may panic. `clone` and dynamic dispatch require no blanket ban.
- **RUST-3 — Error handling.** Propagate failures or explicitly handle their
  consequences. Best-effort operations must retain diagnostics and preserve safety
  state when they fail. Scope lint suppressions narrowly and explain their reason.

## Transactions and concurrency

- **TX-1 — Atomic preconditions.** Validate mutable preconditions and commit
  dependent database effects within one transaction. Enforce exclusive ownership
  with database constraints or atomic claims, rather than separate check/write
  operations.
- **TX-2 — Atomic history.** Commit state mutations, required history, evidence,
  and accepted-result records together. A failed transaction leaves none of its
  effects accepted.
- **TX-3 — Replay.** For retryable outcome-acceptance operations, authorize scope
  before replay, return the originally accepted result for an exact retry, and
  reject conflicting retries. Later live state must not rewrite accepted results.
- **TX-4 — External calls.** Keep Git, filesystem, process, and provider calls
  outside database transactions. Revalidate or atomically claim relevant state
  when committing effects that depend on observations made outside a transaction.

## Recovery and safety

- **SAFE-1 — Partial failure.** For external effects whose interruption could
  duplicate work or make cleanup unsafe, persist sufficient intent and resource
  identity to reconcile after restart. Retry only when duplication is prevented
  or the operation is explicitly safe to repeat.
- **SAFE-2 — Uncertainty.** Preserve unknown outcomes as unknown. Missing
  heartbeats, timeouts, or failed lookups alone do not establish that an Agent has
  exited or a resource is absent. Retain capacity reservations when ownership
  cannot safely be reconciled.
- **SAFE-3 — Retained evidence.** Preserve recovery facts, history, retry counters,
  and safety diagnostics. Clear Dispatch Holds, reset budgets, or release retained
  resources only through their explicitly authorized operations, never as an
  incidental consequence of retry, completion, or ordinary restart.
- **SAFE-4 — Destructive operations.** Verify ownership and canonical filesystem
  boundaries before cleanup. Missing evidence does not grant permission. Preserve
  resources for explicit repair when ownership or removal safety is uncertain.

When changing dispatch or cleanup, consult [Dispatcher](docs/dispatch.md),
[Assignment recovery](docs/assignment-recovery.md), and
[Managed worktrees](docs/worktrees.md) for the applicable product rules.

## Security

- **SEC-1 — Authorization.** Authorize Assignment operations server-side against
  their bound identity, scope, and permitted action. Workspace selection is routing,
  not authentication; Assignment capabilities do not authenticate operator clients.
- **SEC-2 — Secrets.** Deliver capability secrets through the established
  owner-only secret-file mechanism. Keep secrets out of prompts, environment
  values, logs, errors, and ordinary output; test redaction on changed exposure paths.
- **SEC-3 — Processes.** Invoke programs with explicit argument arrays. Use shell
  execution only for intentional shell-command features, such as Project Setup
  Hooks, and preserve their documented trust boundary.

For capability delivery and bounded reads, consult
[Assignment context](docs/assignment-context.md). For listener exposure, consult
[Security](README.md#security).

## Storage and interface contracts

- **CONTRACT-1 — Storage compatibility.** Advance the schema revision for breaking
  storage-contract changes. Reject incompatible stores before initialization
  writes, without automatic conversion, reset, invented backfills, or marker
  rewrites. Update [Database structure](crates/store/README.md) with schema changes.
- **CONTRACT-2 — Wire compatibility.** Assess compatibility explicitly when
  changing CLI arguments, RPC methods, serialized fields, or machine-readable
  output. Make breaking changes intentional and tested; preserve structured output
  separately from diagnostics. Update affected help, examples, and contract tests.
- **CONTRACT-3 — Management Protocol.** Follow the compatibility and revision rules
  in [Management Protocol](crates/docs/management-protocol.md) when changing daemon
  attestation or lifecycle contracts. Treat storage and protocol revisions as
  separate contracts.

## Tests and shipped skills

- **TEST-1 — Behavior.** Test observable behavior and invariants at the boundary
  that owns them. Add integration tests when a change crosses contracts. Every
  bug fix includes a regression test that fails without the fix.
- **TEST-2 — Failure cases.** Cover the failure dimensions affected by the change:
  denied actions, stale evidence, conflicting retries, concurrent claims,
  interrupted effects, or restart recovery. A happy-path test alone does not
  verify a changed safety gate.
- **TEST-3 — Isolation.** Isolate databases, listeners, environment changes, and
  temporary directories so tests pass in parallel. Prefer controlled clocks and
  explicit synchronization over sleeps. A serial pass does not waive a failing
  default-parallel suite.
- **SKILL-1 — Portability.** Keep required references inside each shipped skill's
  directory; resolve other skills by name. Update affected examples and run
  documented-command parser tests and skill-portability tests when changing skills.
  See [Maintaining skills](docs/development.md#maintaining-skills) for fixture rules.

## Verification

- **VERIFY-1 — Required gates.** Run from the repository root:

  ```sh
  cargo fmt --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  git diff --check
  ```

  For shipped skill changes, also run:

  ```sh
  cargo test -p forge-cli --bin forge documented_skill
  python3 -m unittest discover -s tests -p 'test_skill_*.py'
  ```

- **VERIFY-2 — Truthful results.** Report checks actually run, their outcomes,
  and any unrun gates or unresolved failures. Distinguish recommendations from
  execution. Local checks do not establish release or recovery readiness; apply
  [Development verification](docs/development.md#verification) and the applicable
  release policy separately.
