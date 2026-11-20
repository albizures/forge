# T10 — Transactional first recovery authorization

## Status: partial, internal policy foundation only

This increment adds application/store-owned first recovery lease issuance and
specialized claim consumption. It does **not** implement a concrete recovery host
verifier, consumer handshake, daemon recovery or writable Harness reopen. T10
remains pending; T11 must not advance. No production or public API calls this port.

## Implemented boundary

`crates/app/src/durable_execution/recovery/authorization.rs` defines:

- `RecoveryHost`: the trusted external boundary responsible for proving predecessor
  descendant execution safe, holding exclusive canonical ownership through commit/
  delivery, inspecting the original pending runtime input and all committed effects,
  and validating applicable freshness/retained-work policy without adopting or
  repairing dirty progress. Before returning it must persist protected external
  evidence keyed by the exact claim operation/binding. There is deliberately no
  production implementation yet; a flock/PID/expired lease is insufficient proof.
- `Prepared::verify`: snapshots the original typed candidate and claim, verifies
  their content commitment, calls that boundary, then constructs a private,
  non-deserializable prepared transition. Retained JSON audits cannot reconstruct
  this object or substitute for external verification.
- A new first lease keeps Launch, attempt/generation and conversation identity,
  increments the Assignment-wide sequence (including prior renewals), and binds a
  newly verified waiting host/worker and the claim's distinct daemon epoch. It uses
  the existing 30-second bound. Expired old deadlines are not renewed. Backward
  issuance in the same boot/time coordinates rejects; changed boot coordinates
  are not compared as if they shared a clock. A changed boot ID alone is not proof
  of a reboot: the trusted verifier still owes actual safety evidence.
- The immutable `Grant` audit retains only claim, new lease and external evidence
  SHA-256. The digest links protected proof; it is not itself process-safety proof,
  a copied receipt or execution authority. Task bodies/handoffs/credentials stay out.

`crates/store/src/durable_execution/recovery_authorization.rs` implements the trusted
internal `grant_recovery` port. An immediate transaction authorizes scope and exact
binding, validates the bounded proposal, requires the exact retained claim, and
re-runs the shared candidate gate. Only that exact claim is exempted from resource
blocking. Current Task mode/status, open attempt/generation, Spec authorization,
dependencies, worktree binding, accepted admission, retained grant, competing
Assignments/resources, retirement, holds and effects remain gates. History sequence
columns must agree with their serialized lease values; mismatches fail closed.

Acceptance commits all three effects together:

1. The new lease in immutable Assignment-wide grant history.
2. An already-retired `recovery-grant` audit resource with the original acceptance
   timestamp and bounded identities/evidence digest.
3. Retirement of the exact active `recovery-claim` fence at that same timestamp.

No attempt, Outcome, runtime admission, context, hold, evidence or worktree is
changed. The runtime reservation continues to reserve capacity. Insert/update
failure rolls everything back. Generic resource insertion/retirement cannot forge
or consume either recovery resource kind.

An exact retry can return the identical committed grant only while live current
Forge authority still permits its delivery, its original bounded deadline is live,
and it is still the latest grant. It revalidates original context and the retained
predecessor history. Changed owner/deadline/proof, cancellation, revocation, expiry,
stale bindings or later renewal reject without rewriting the historical audit.
This follows lease-delivery semantics, not historical-result-as-authority replay:
the original audit remains inspectable in Assignment resources after delivery is
no longer authorized. A failed/uncertain return must not be delivered as a grant.

## Compatibility and deliberate limitations

No DDL/schema revision (schema 22), dependency, runner protocol or public CLI/RPC
change. `recovery-grant` is an additive opaque, retired resource kind. Older normal
issuers cannot adopt the new owner because history rejects a restarted sequence
one; old-owner renewals conflict with the new owner/epoch. Ordinary renewals can
follow the committed recovery lease, but no current controller is seeded to do so.

Existing fresh watchdog/runner consumers deliberately reject a first lease above
sequence one. A later specialized, owner-verified recovery handshake must authorize
that transition; this increment does not reset watchdogs or weaken first-grant
validation. No new model/tool admission or writable runtime open is possible through
this persistence port alone. The current single retained recovery claim/audit
conservatively rejects additional recovery operations; safe repeated recovery
must be composed explicitly rather than transferring a fence on timeout/restart.

## Verification

Ten new fake-host/application/SQLite policy tests cover:

- atomic lease/audit/fence acceptance with all other tables unchanged, same bound
  attempt/admission/context, retained capacity and exact retries across connections;
- failed external verification and wrong candidate commitments (no host calls for
  wrong content);
- old host/worker/epoch reuse, invalid predecessor grants, sequence/clock overflow,
  backward same-coordinate issuance and malformed evidence digest rejection;
- current mode/status, revocation, generations, context, worktree, admission,
  malformed sequence history and all unrelated resource blockers revalidated;
- foreign scope, conflicting operation/proof, before-issue/exact-expiry deadlines
  and post-commit revocation rejecting without writes;
- accepted audits not bypassing later holds/context/effects or malformed history;
- continued global sequence after predecessor renewals;
- lease insert, audit insert and fence update failures rolling back together;
- independent-connection exact races and conflicting owner/deadline races;
- generic audit mutation denial, old/restarted issuer rejection and monotonic
  renewal after a specialized commit, with no old grant delivery after renewal.

Commands actually run:

```sh
cargo test -p forge-store durable_execution::tests::recovery_tests
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

The focused recovery suite passed (30 tests, ten new); the final workspace run
passed **735 Rust harness tests plus seven custom-harness supervised scenarios**,
with one pre-existing opt-in test ignored. Runner typecheck and **64 Node tests**
passed. Formatting, Clippy and diff checks passed on the final snapshot.

The first full workspace run failed the existing
`interrupted_external_mutation_retains_forge_blocker_across_daemon_restart` test at
`crates/cli/tests/durable_check_bootstrap.rs:669`: inspection-lock reacquisition
returned `WouldBlock`. The complete five-test target then passed, followed by a
complete **default-parallel** workspace rerun. No serial fallback, production fix
or diagnosis/resolution of that transient lock failure is claimed. It remains
recorded evidence, not release readiness. Initial focused fixture issues (empty
body redaction sentinel and SQLite SELECT/UPSERT syntax) and a test-only Clippy
semicolon finding were corrected before the final runs.

New tests use a fake `RecoveryHost`, not kernel/process cleanup proof, real Git
freshness inspection, recovery control transport or runtime execution. Existing
supervised and runner tests were rerun unchanged; they do not demonstrate successful
same-Assignment recovery. No new packaged-component, real-provider or physical
power-loss verification ran. No live Assignment, configuration or worktree changed.

## Implementation review

T10 asks to “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates”: **partial**.
The first-grant application/store transition is implemented, but no concrete host
verifier, recovery delivery/consumer handshake or daemon/writable-open path exists.

“Safe restart resumes the same attempt/conversation with no duplicate admission”
and “Daemon and runner restart order cannot bypass gates” still require integrated
successful recovery and the full restart matrix, including repeat recovery. These
new tests establish policy/persistence only. “Continuation and independent Review
successors receive fresh conversations and exact approved context” still lacks
the new durable recovery-to-successor demonstration; glossary/ADR recovery updates
remain outstanding.

Summary: T10 remains incomplete; the main gaps are concrete safety verification,
recovery handshake/controller, writable reopen and integrated verification. No
scope creep or incorrect execution-readiness claim found.

## Coding-standards review

Reviewed changed app policy/ports, store transaction/shared candidate gate,
generic resource guards, all pending cancellation/authorization tests and updated
docs against `CODING_STANDARDS.md`. Domain policy stays in app, transactional
revalidation and history in store, external I/O behind a trusted host interface.
The new prepared value cannot be decoded from retained audit JSON. Scope and
current permission precede delivery replay; failures preserve audit/fence/grants
atomically. Tests use isolated stores/connections and fake clocks, cover denied
scope, conflicting races, rollback, stale evidence and expiry, and distinguish
fake verification from process evidence. No secret exposure, worktree repair,
wire/schema break, dependency or ambient fallback is introduced.

Summary: no introduced coding-standards violations found. This is a self-review,
not independent T18 or a claim of T10/release completion.
