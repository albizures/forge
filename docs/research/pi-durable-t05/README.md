# T05 — authorization lease foundation (in progress)

## Disposition

**T05 is not complete. Do not advance to T06.** The increments delivered so far
cover application/store policy, the issuance coordinator, a trusted Linux host
evidence adapter and an unwired server service seam. The T04 synthetic runner and
Rust supervisor executable do not yet enforce grants. No server launch path, live
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

## Foundation verification (prior increment)

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

## Foundation self-review against T05 and standards

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

## Issuance-coordinator increment

`crates/app/src/authorization_lease.rs` now adds `Issuer`, a serial,
one-Assignment/daemon-lifetime application coordinator, and the `AuthorityHost`
external-evidence port. It obtains fresh trusted owner/boottime evidence on every
issuance, constructs the bounded grant itself, validates succession and delegates
transactional authority checks/persistence to `Repository`. It returns only a
successfully persisted grant. Storage/delivery latency does not move its deadline;
consumers must independently check expiry before admission.

Unavailable evidence, backward clock readings, changed owner/boot/time namespace,
exact/late expiry, overflow and any repository error permanently fence that
coordinator. A lost commit reply returns no permission and cannot trigger a local
retry or extension. New daemon epochs cannot adopt retained history as permission.
The application does not clear diagnostics, close resources or release capacity.

This is **not** the trusted Linux evidence adapter, daemon service wiring, epoch
generation, renewal timer, authenticated delivery or an admission/watchdog guard.
`AuthorityHost` is an internal trusted port, not a new RPC accepting caller-supplied
owner/clock values. No production code instantiates `Issuer` yet. The coordinator
is not persisted; repository history remains the durable fence across restart.

Six additional tests pass: five fake-clock/host/repository tests cover renewal,
terminal outage fencing, clock/owner discontinuities and expiry, uncertain commits
and new daemon epochs; one actual SQLite integration test uses reopened stores to
verify sequential renewal, rejected new-epoch restart, transactional rejection
after Assignment closure and retained history/capacity. These are deterministic
policy/store tests, **not** real daemon or process-outage evidence.

The four repository gates listed above were rerun after this increment: all pass,
**591 Rust tests passed, 0 failed, 1 opt-in probe ignored**. The focused application
lease suite also passed (7 tests). Runner/build/process and real-provider tests
were not rerun because runner/host/protocol are unchanged; runtime enforcement
remains missing. No schema, wire, dependency, shipped skill or live state changed.

Self-review against T05: “server-side validation”, “Gate all model/tool admissions”
and “Expiry blocks new work, cancels active execution” remain partial/missing as
detailed above. No scope creep or incorrect implemented requirement was found in
the coordinator increment. Coding-standards review found no introduced violation;
the trusted external-evidence port and store calls preserve ARCH-1/2 and TX-4,
while terminal uncertainty fencing and retained capacity preserve SAFE-2/3.
This is self-review, not T18 independent completed-system review.

## Trusted Linux evidence and server seam increment

Delivered `crates/runner-host/src/lib.rs`: `SpawnedOwner` borrows a live directly
spawned `Child` after readiness; retained PID text alone cannot construct an
adoption. Every observation rechecks protected canonical directory device/inode,
bounded strictly decoded owner evidence, live host/worker start/parent/group
identities, boot and matching daemon/host/worker time namespaces. Both inherited
FDs must name the expected lock inode/device **and** report an exclusive WRITE
FLOCK in `/proc/PID/fdinfo` belonging to the spawning host. Global lock searches
or mere open-file identity are not substituted for kernel ownership evidence.
Fallible `CLOCK_BOOTTIME` reads use checked millisecond conversion; missing,
changed, backward or unreadable evidence terminally fences the observer. No
runtime database is opened and no process is adopted or signalled.

`crates/server/src/durable_authorization.rs` composes that adapter with `Issuer`
and the actual SQLite repository. `DaemonAuthority` generates a fresh 256-bit
epoch from OS randomness, accepts only a directly spawned `Child` and immutable
trusted Forge bindings, and exposes no agent-supplied grant/clock/owner/epoch
parameters. This is an **unwired service seam**, not production dispatch, canonical
Assignment-directory mapping, renewal scheduling/delivery or runtime enforcement.
No daemon startup instantiates it yet. Historical process adoption/recovery is
still unsupported rather than inferred from owner records.

Dependencies: the host library adds the existing `forge-app` interface and
`serde` derive/strict decoding; `rustix`'s existing dependency gains its `time`
feature for safe fallible boottime syscalls under the workspace unsafe-code ban.
The server depends on the existing host crate to reuse verified kernel evidence
rather than duplicate ownership checks. No new package/version was added to the
workspace closure; `Cargo.lock` records the new dependency edges. There is no
storage or serialized wire change. The new Rust API is internal composition,
not a new CLI/RPC contract. `runner/README.md` documents limits and reproduction.

Eight new real-process tests use the actual Rust supervisor and an inert Python
worker, with isolated directories and explicit readiness/acknowledgement/exit
channels. They cover successful shared-flock/monotonic observations with zero
runtime database files, mismatched/foreign/stale identities, changed evidence and
terminal fencing, unlock/closed descriptors, exited worker with a retained host
lock, exited/unrelated supervisor with a surviving worker lock, unsafe
permissions/symlinks/hardlinks and replaced directory/lock inodes. Python's standard
library at `/usr/bin/python3` is test-only; no credentials/providers are involved.
One server test checks that new daemon-lifetime factories generate distinct epochs.
The first compile/clippy runs exposed a signal-constant name, ambiguous test
conversion, test naming/import layout and epoch formatting lint; all were corrected.

On the corrected snapshot, these commands all pass:

```sh
cargo build -p forge-runner-host
cargo test -p forge-runner-host --test authority
(cd runner && npm run prepare-reader && npm run check && npm test)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

**600 Rust tests passed, 0 failed, 1 opt-in probe ignored** (including eight
host-evidence tests); **10 TypeScript runner/process tests passed**. The derived
reader hash is unchanged. No clean npm install, packaged release, real-provider
verification or new process-outage/expiry enforcement test ran. The inert child
and existing synthetic model tests do not establish cancellation on lease loss.

Self-review against T05: the requested trusted evidence and server policy seam
now exist, but “Gate all model/tool admissions” and “Expiry blocks new work,
cancels active execution” are still unimplemented. Production daemon wiring,
renewal transport/timing, durable expiry diagnostics and independent watchdog
remain blockers; no T06 advancement or rollout is authorized. No scope creep or
incorrect implemented requirement was found. Standards review found no introduced
violation: host I/O stays outside store transactions, OS observations are explicit
and fail closed, and failed observation does not clear retained capacity. The
new service's full daemon-to-runner integration is unverified, not implied by
composed component tests. This is self-review, not T18 independent review.

## Resume here

1. Wire `DaemonAuthority`/`Authorization` at the trusted daemon launch boundary,
   using canonical Assignment/runtime mapping and owned supervisor readiness.
   Own renewal cadence and authenticated grant delivery there; do not expose
   caller-supplied grant/clock/owner/epoch values as authority. Preserve the
   production launch gate until all required runtime guards are present.
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
