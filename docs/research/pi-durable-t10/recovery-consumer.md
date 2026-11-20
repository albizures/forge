# T10 — Exact-grant recovery lease consumer

## Status: partial; internal consumer only

Adds a specialized recovery initialization path in `runner/src/authorization.ts`.
It is not wired into an executable, supervisor, watchdog or daemon and does not
open runtime storage. T10 remains pending and T11 remains blocked. Production
and automatic recovery remain disabled.

## Behavior and trust boundary

- `Authorization.forRecovery` snapshots an expected claim/lease/evidence envelope
  supplied only by trusted fresh host composition. It checks claim identity and
  digest representation, exact Launch/generation/new-owner/epoch binding and a
  bounded recovery sequence greater than one. Preparation grants no permission.
- `acceptRecovery` requires the identical envelope after the application's commit.
  It uses the existing lease validation for safe integers, exact owner/bindings,
  30-second bounds and monotonic boottime. Invalid delivery permanently fences.
- Ordinary `accept` cannot initialize this waiting consumer. Ordinary consumers
  still reject initial sequences greater than one, including retained recovery
  leases. There is no ordinary sequence-reset fallback.
- Exact envelope retries preserve deadlines and snapshot semantics. Subsequent
  ordinary renewals must increment the sequence in the same new epoch. An old
  recovery envelope after renewal is conflicting delivery, not permission to reset.
- Expiry, clock rollback, closure and invalid delivery preserve terminal abort;
  the same provider/tool admission wrappers and cancellation signals apply.

This is a defensive consumer boundary, **not authentication or authority issuance**.
An expected digest does not prove cleanup, freshness, the transaction or process
absence. The host must verify these obligations, keep exclusive ownership and
supply the expectation through trusted composition, never by deserializing a
retained audit or forwarding an agent/operator request. The existing executable
and supervisor protocol still accept only initial sequence one. No recovery
permission is deliverable through them yet.

## Compatibility

No dependency, schema, public CLI/RPC, host control or Management Protocol change.
The new internal envelope follows the existing app `recovery::authorization::Grant`
shape. Tests construct that shape in JavaScript; they do not demonstrate actual
Rust-to-Node recovery delivery. The already-hashed authorization source changes
the matched component digest. No retained binding is rewritten.

## Verification

Commands actually run from the Forge repository root:

```sh
(cd runner && npm run check && node --test test/recovery-authorization.test.mjs test/authorization.test.mjs)
(cd runner && npm test)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

All passed. Runner typecheck and 76 Node tests pass, including six new recovery
consumer tests and all existing real-process/runtime tests. The Rust default-
parallel workspace suite passes (740 Rust harness tests plus seven supervised
scenarios, one pre-existing opt-in ignored). No transient failure occurred in
these runs; historical transient failures are not claimed fixed.

New tests cover exact-envelope delivery/retry/renewal, no permission before
handshake, immutable caller snapshots, ordinary/recovery admission separation,
claim/evidence/binding mismatch, malformed expectation and delivered lease,
expired first delivery, rollback, closure, superseded retries and guarded
provider/tool effects and cancellation. These are fake-clock consumer and wrapper
checks, not a recovery host, supervised resume or real-provider demonstration.
No live Assignment, configuration, credentials or worktree was changed.

## Implementation review

T10 asks to “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates”: **partial**.
This increment supplies only the specialized consumer prerequisite. Concrete host
verification, protected claim-bound proof, ownership handoff, independent watchdog
recovery and daemon/writable-resume composition remain missing.

“Safe restart resumes the same attempt/conversation with no duplicate admission”
and the complete restart-order matrix remain undemonstrated. Continuation/Review
successor demonstrations and glossary/ADR updates also remain outstanding.
No scope creep or incorrect claim of execution authority was found.

Summary: T10 remains incomplete; authorized writable recovery is still the main gap.

## Coding-standards review

Reviewed the changed authorization source, isolated tests and documentation
against `CODING_STANDARDS.md` and the surrounding consumer/provider/tool contracts.
Domain grant issuance remains in app/store; this change only adds defensive
consumer enforcement. Snapshots prevent mutable input from extending permission;
unknown fields, mismatches and late delivery fail closed. No external call enters
a database transaction, no persisted grant is adopted, and no new executable
control, secret exposure or worktree repair is introduced. Existing ordinary
initialization and renewal regressions pass.

Summary: no introduced coding-standards violations found. Self-review only; not
independent T18 review or T10/release completion.
