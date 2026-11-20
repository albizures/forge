# T10 — Recovery grant delivery coordinator and watchdog policy

## Status: partial; trusted application composition only

Adds application-owned commit-before-delivery recovery coordination and an exact-
envelope recovery initializer for the independent watchdog policy. No executable
host, pipe transport or daemon invokes these new ports. This does not implement
predecessor cleanup verification, protected host proof, ownership handoff or
writable runtime resume. T10 remains pending; T11, production and automatic
recovery remain blocked.

## Implementation

`crates/app/src/durable_execution/recovery/session.rs::Session` consumes `Prepared`,
the opaque non-serializable result of the existing trusted external verification
boundary. It cannot be constructed from a retained grant/audit. Its first tick:

1. Observes the newly waiting owner and trusted boottime outside a transaction.
2. Requires the exact verified owner and a currently valid prepared lease; a clock
   earlier than verification, unavailable observation or expired deadline denies.
3. Calls the existing transactional recovery repository to revalidate the claim,
   original binding and all current Forge authority/freshness/resource gates, then
   commit the lease/audit/claim consumption atomically.
4. Requires an identical successful transaction result and reobserves owner/time
   afterward. Ownership drift, clock rollback or expiry withholds delivery, even
   though the transaction may have committed. Storage latency never shifts time.
5. Sends the full exact claim/lease/evidence envelope through a specialized bounded
   `Delivery` port. Only after success does it activate ordinary lease renewal.

`authorization_lease::session::Session::after_recovery` is crate-private and seeds
renewal only from the opaque verified object and identical live transaction result.
It is not a public retained-lease constructor. Subsequent ticks reuse the existing
host/store reauthorization, 10-second cadence, post-commit observation and bounded
ordinary lease delivery. Global sequences continue; Assignment, Check attempt,
profile, freshness, evidence and conversation bindings do not change.

Failed commit replies, conflicting results, uncertain delivery or explicit stop
permanently fence the coordinator. Retention can retry while preserving the first
failure, but host calls, grant commits and transport delivery cannot resume through
this object. A committed-but-undelivered grant/audit remains historical evidence,
not replacement permission. Dropping a coordinator does not retire resources,
release its claim/capacity or prove process absence.

`LeaseWatchdog::for_recovery` prepares an **unarmed** expectation from trusted
fresh host composition. It validates the original claim/lease binding, exact new
owner, digest representation and lease bounds, but does not accept permission.
`accept_recovery` arms only the identical committed envelope. Ordinary acceptance
cannot initialize this waiting watchdog; ordinary watchdogs still reject initial
non-one sequences. Exact retries never extend time; renewal remains sequential
in the same new epoch, and superseded envelope retries terminally fence. Existing
clock, expiry and explicit failure behavior is unchanged.

The watchdog initializer and TypeScript counterpart are defensive consumers, not
authentication or process proof. A future controller must verify predecessor
safety, retain canonical exclusive ownership and use protected private transport.
Never populate either expectation from a retained audit or an agent/operator
payload. The executable supervisor still uses its ordinary initializer/control
protocol; no recovery scheduling has become possible.

## Tests and verification

Actual commands from the Forge repository root:

```sh
cargo test -p forge-app recovery_watchdog
cargo test -p forge-store recovery_session
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final gates pass: **752 Rust harness tests plus seven supervised scenarios**, one
pre-existing opt-in ignored; runner typecheck and **76 Node tests** pass. Twelve
new Rust tests cover:

- Actual SQLite recovery commit before specialized delivery, exact audit/claim
  consumption and one initial delivery followed by waiting/ordinary renewal.
  A real `LeaseWatchdog` policy consumes that envelope and subsequent renewal.
  Unrelated tables and the original attempt/context remain unchanged.
- A real successful SQLite commit with a deliberately lost or conflicting reply:
  no delivery, no second commit/host observation and retained capacity.
- Unavailable or changed ownership, early/backward time, post-commit expiry,
  revocation and freshness changes. Already committed history is not erased.
- Initial delivery uncertainty with unavailable diagnostic persistence, followed
  by successful retention retry without retransmission or cause replacement.
- Active renewal expiry, revocation, delivery failure and explicit stop; stopping
  before delivery preserves the unconsumed claim and never contacts host/transport.
- Watchdog exact-envelope expectation/delivery, sequential renewals and retries,
  malformed expectation, ordinary/recovery startup separation, owner/claim/evidence
  mismatch, early/late grants, clock rollback, explicit fencing and superseded
  recovery retries.

Hosts, transport and clocks in new tests are scripted. The store/policy commits
are real; no process cleanup, independent executable recovery watchdog, Node
recovery delivery or successful same-conversation writable resume is inferred.
Existing actual supervisor/Node/runtime tests also pass. No new packaged,
real-provider or physical-power-loss verification ran. No live Assignment,
operator configuration, worktree or credentials were changed.

An initial focused test used a nonexistent fixture table for freshness; it was
corrected to the existing `record_work` transition and the focused suite passed.
Initial Clippy identified a large enum variant; the active session is now boxed.
A subsequent formatting check caught two newly added test assertions; formatting
was applied before the final complete gates. No historical transient failure is
claimed fixed.

## Compatibility

No dependency, DDL/schema, public CLI/RPC, executable control or Management
Protocol change. All transaction policy uses existing repository operations.
Grant and lease serialized representations are unchanged. New interfaces are
trusted in-process composition ports only; no audit reconstruction or startup
sequence fallback was added.

## Implementation review

T10's “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates” is **partial**:
the coordinator and consumer/watchdog policy prerequisites exist, but no concrete
host verification, protected proof, ownership handoff, executable transport,
daemon composition or writable reopen consumes them.

“Safe restart resumes the same attempt/conversation with no duplicate admission”
still lacks a successful supervised resume demonstration. Full restart-order
matrices, fresh Continuation/independent Review successor demonstrations and
glossary/ADR recovery completion remain outstanding. No scope creep or incorrect
execution-authority claim was found.

Summary: T10 remains incomplete; concrete host verification and authorized writable
resume are the main gaps. This is self-review, not independent T18.

## Coding-standards review

Reviewed changed app policy, crate-private renewal bridge, real-store/scripted
failure tests and documentation against `CODING_STANDARDS.md` and surrounding
lease/recovery contracts. Policy remains in app; existing store transactions
revalidate mutable preconditions and atomically record history without external
calls. Opaque verification cannot be deserialized as authority. Both before/after
observations and commit-before-delivery are enforced. Failure retains original
cause/capacity, and diagnostic retries do not become permission retries. Isolated
fixtures exercise policy, persistence and transport uncertainty; tests do not
claim real host proof. No secret exposure, worktree repair, dependency or public
contract change was introduced.

Summary: no introduced coding-standards violations found. This is not a T10 or
release-readiness certification.
