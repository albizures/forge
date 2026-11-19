# T05 — authorization lease foundation (in progress)

## Disposition

**T05 is not complete. Do not advance to T06.** This increment delivers the
application/store portion only. The T04 synthetic runner and Rust host are
unchanged; they do not yet enforce these grants. No server launch path, live
Assignment, Forge Record, provider, profile, capability, Outcome or rollout was
changed. Production durable dispatch remains disabled.

## Delivered

- `crates/app/src/authorization_lease.rs`: Forge-owned typed grant/owner identities
  and renewal policy. Grants bind the full immutable Launch, generation, exclusive
  owner's boot/time namespace, host/worker start and lock identities, daemon
  lifetime epoch and monotonic sequence. No provider/runtime types escape into
  application policy.
- Timing follows T01: 30-second grants, intended renewal every 10 seconds and
  intended TERM grace of 5 seconds. Only the grant bounds/renewal policy are
  implemented here, **not** renewal scheduling or process signalling.
- First sequence is 1. Renewal cannot change authority, ownership or daemon
  lifetime, skip a sequence, issue in the future, extend non-30-second bounds,
  move issuance backwards or renew at/after expiry. Exact retries never extend
  the persisted deadline. A new daemon epoch cannot renew an old grant; retained
  grants cannot be treated as restart permission.
- `crates/store/src/durable_execution.rs`: trusted internal repository operation
  revalidates scope, live Assignment/attempt, profile and runtime identity,
  freshness, existing Spec/Stack/dependency execution gates, Execution Mode,
  Dispatch Holds and unresolved resource blockers in the same immediate
  transaction that appends a grant. Replay revalidates current authority first.
  Rejected admissions cannot obtain a grant.
- Schema **21** adds append-only `assignment_authorization_leases`, keyed by
  Assignment/sequence with an FK to its retained runtime. Ordinary schema-20 open
  fails before writes. Explicit offline 20 → 21 conversion creates empty history,
  preserves old rows, and creates no permissions. Frozen schema validation and
  older conversion composition remain intact; historical fixtures now remove the
  new expansion instead of falsely stamping new DDL as an older schema.
- The runtime-context glossary now distinguishes an Assignment Authorization
  Lease from capability, heartbeat and process absence. The authorization portion
  of the ADR is recorded at `crates/docs/adr/0012-authorized-durable-recovery-and-conservative-retirement.md`:
  0012 was the next free number, so the Spec's proposed 0013 was not copied as a
  reserved number. Retirement/recovery will complete that ADR later. Store and
  state-upgrade references describe the new contract and incomplete enforcement.

This API is not exposed as an agent capability or an operator RPC method. Its
owner and `now` parameters are **trusted adapter inputs**, not verified host
observations in this increment. Grant persistence is not an authenticated renewal
transport, host ownership verification, Task start, bootstrap acknowledgement,
heartbeat, accepted Outcome or release of capacity. No external calls occur in
the grant transaction and no secrets are included in grant identity/diagnostics.

## Verification

From the Forge repository root, on the corrected snapshot:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

All four pass: **585 Rust tests passed, 0 failed, 1 opt-in probe ignored**.
Seven new tests cover:

1. Deterministic clock observations at issuance, just before expiry, exact expiry
   and late renewal, including rejecting a renewal as a new initial grant.
2. Binding/owner/epoch/namespace/sequence/clock-bound changes rejected by policy.
3. Exact grant retry and renewal across repository reopen, old-sequence and new
   epoch rejection, immutable history and retained durable capacity.
4. Scope, mismatched profile, stale generation, revoked Assignment, closed attempt,
   hold, unresolved resources and unknown state rejecting even exact retry.
5. Competing renewal payloads on independent connections admitting one history
   branch only.
6. Failed schema-21 stamping rolling back all new DDL/history, then successful
   empty expansion preserving all previous table inventories and idempotent replay.
7. Ordinary schema-20 open preserving bytes; explicit copy-and-upgrade preserving
   source bytes and old inventories while adding no grants.

Initial compile/test runs exposed scoped-thread capture, a fixture using a
nonexistent Dispatch Hold timestamp column, stale schema assertions and historical
fixtures retaining new DDL. Those were corrected; the full default-parallel
workspace suite passes. Clippy's documentation/module-order findings were also
corrected before the final gates. No dependencies or shipped skills changed.
JavaScript/real-process runner tests were **not rerun** in this increment because
runner/host/protocol files are unchanged. Existing T04 results are not claimed as
lease-expiry evidence. No real-provider or packaged release verification ran.

## Self-review against T05 and standards

Partial requirements (blocking T05 completion):

- T05 says “Implement app-owned authorization and server-side validation.” The
  policy and transactional validation are present, but no trusted daemon service
  yet obtains host/clock evidence, issues/delivers grants or owns renewal cadence.
- T05 says “Gate all model/tool admissions … including compaction and
  internal/resumed execution.” Runner/provider/tool guards are not implemented.
- T05 says “Expiry blocks new work, cancels active execution and retains
  uncertainty.” The independent host watchdog, cooperative cancellation, durable
  expiry diagnostics and bounded TERM/KILL attempts are not implemented.
- Required real long-running child/controlled model outage tests, admission and
  cancellation counts, clock failure/backward readings and daemon/runner restart
  integration remain unrun. Deterministic policy tests do not replace them.

Summary: T05 remains incomplete; missing runtime enforcement is the release-
blocking issue. No scope expansion or known coding-standard violation was found
in this bounded foundation. This is self-review, not T18's independent review.

## Resume here

1. Add a trusted daemon/host authorization service with current boot/time-namespace
   and exclusive ownership verification, server-derived CLOCK_BOOTTIME readings,
   lifetime epoch handling and fail-closed unavailable/backward clock behavior.
   Do not expose caller-supplied grant/clock/owner values as authority.
2. Extend the matched runner contract deliberately. Require initial authorization
   before writable scheduling; gate actual provider dispatch and tool execution
   (including nested/compaction/resumed paths) and combine cancellation signals.
   Retain read-only retry/observation semantics without issuing execution grants.
3. Add the independent host watchdog, expiry cancellation and bounded TERM/KILL
   initiation. Persist diagnostics and reserve capacity until verified cleanup;
   do not infer absence from timer expiry, signalling success or worker exit.
4. Verify renewal races, connectivity loss, exact expiry, restart, stalled JS,
   long-running child and controlled model paths using fake clocks and explicit
   synchronization. Re-run runner/build tests plus all repository gates, renew
   this review/evidence and only then consider checking T05 complete.
