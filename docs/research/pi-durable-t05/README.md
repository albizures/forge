# T05 — bounded authorization leases: implementation and evidence

## Disposition

**T05 is complete for the plan's controlled pre-dispatch authorization boundary.**
The final increment below delivers canonical storage binding, the actual
SQLite-backed daemon-service delivery/renewal loop, atomic durable failure
retention and combined host/consumer outage/restart evidence. Earlier increment
sections are a historical journal: their then-pending work does not override this
current disposition.

New synthetic execution requires a grant; read-only replay remains ungranted.
Normal production dispatch still does not select/launch this backend: explicit
profiles/authentication are T06, the Check-bound dispatch/bootstrap/Outcome tracer
is T07, and matched installation/release gates remain later Tasks. No live
Assignment, Forge Record, operator profile, capability, Outcome or rollout was
changed. T05 completion does not authorize live work, automatic recovery or
release. **Production durable dispatch remains disabled.**

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

## Consumer-side admission/cancellation increment

Delivered `runner/src/authorization.ts`, not yet imported by `main.ts` or exposed
through runner wire v1. Trusted composition supplies immutable Launch, generation,
verified owner and fresh daemon epoch. The consumer accepts only committed grants
from a future protected daemon channel; accepting JSON is not authentication.
There is no new tool, agent capability, operator RPC or caller-controlled
production grant path. The existing T04 synthetic executable remains unchanged
and still does not require a lease. Production durable dispatch remains disabled.

The consumer starts without permission, enforces the app's 30-second absolute
bounds and sequential renewals, and permits exact delivery retries without
extension. It copies grant primitives so payload mutation cannot renew locally.
Binding/owner/epoch changes, sequence gaps, unsafe integer values, late delivery,
clock failure/backward readings and exact expiry terminally abort the consumer.
Fresh daemon-epoch binding rejects even an unexpired sequence-1 prior-lifetime
grant; a fresh consumer cannot start from a retained renewal sequence. It has no
persistence/adoption or authority-issuance API. A shared JSON fixture is checked
by both TypeScript consumer tests and a new Rust app round-trip/validation test.

The production-default consumer clock reads `/proc/uptime` and rounds up one
centisecond, denying conservatively relative to CLOCK_BOOTTIME. Future trusted
composition must verify matching boot/time namespaces before delivery. Each
admission rechecks time; a 50ms cooperative interval also initiates abort when no
new request arrives. This timer cannot enforce bounds during a stalled JS loop
or host suspension and is explicitly **not** the independent host watchdog.
Closing the consumer aborts active signals rather than leaving permission alive.

The provider proxy gates all five request-producing methods audited for pinned
pi-ai 1.5.2 (`stream`, `streamSimple`, `fetchDeferred`, `generateImages`, `classify`),
while permitting cancellation/metadata calls. The tool wrapper checks immediately
before actual execute; it does not rely on preflight hooks. Both combine caller
and lease cancellation, rejecting already-cancelled callers before dispatch.
Future composition must wrap every provider/tool, including dynamic/nested tools;
these functions alone do not enforce coverage in the executable. The component
hash now includes `authorization.ts`; this deliberately changes the matched
local component digest, not the runtime schema or runner wire contract. No new
dependency or npm installation was needed.

Twelve new Node tests cover deterministic renewal/expiry/retry and failure cases,
cooperative timer cancellation, all request methods and combined signals. Using
actual pinned Harness/storage plus a controlled provider, tests also verify zero
provider calls for expired initial/same-store resumed input, one in-flight model
request cancelled with no subsequent request, explicit compaction denial, direct
and nested tool preflight expiry with zero effects, and cancellation/observed
SIGTERM exit of a real cooperative long-running Node child. These are fake-clock
consumer-outage tests, not actual daemon transport-loss or process-tree cleanup
evidence. The first test run caught the test hook's arguments in the wrong order;
that fixture was corrected before the passing runs. Review also identified that
the initial epoch must be pinned at construction; that check and a prior-lifetime
sequence-1 rejection test were added before final verification.

Verification from the repository root:

```sh
cargo build -p forge-runner-host
(cd runner && npm run prepare-reader && npm run check && npm test)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

All pass: **601 Rust tests passed, 0 failed, 1 opt-in probe ignored**;
**22 Node runner/process tests passed** (12 new consumer tests plus the 10
existing executable/process tests). Derived reader hash is unchanged. No clean
npm installation, real-provider, packaged release, daemon outage or independent
watchdog verification ran. Existing store tests retain capacity across restart;
the new consumer tests do not establish durable expiry diagnostics/blockers.

Implementation self-review against T05: “Gate all model/tool admissions” is only
partially delivered through separately exercised guards; executable composition,
pre-writable-open authorization and authenticated launch/renewal delivery remain
missing. “Expiry blocks new work, cancels active execution and retains
uncertainty” is demonstrated only at the consumer boundary: independent watchdog,
bounded TERM/KILL initiation and persisted expiry diagnostics remain missing.
No scope creep or incorrect implemented requirement was found after correction.
Standards self-review found no introduced violation: policy issuance remains in
app/store, defensive consumption stays at the runner trust boundary, cancellation
errors expose only stable codes, and no capacity/cleanup state is cleared. This
is not T18 independent review. **T05 remains incomplete; do not advance to T06.**

## Executable enforcement and independent-watchdog increment

Delivered since the consumer-only increment:

- App-owned `LeaseWatchdog` policy validates owner-bound sequential grants, exact
  live delivery retries and monotonic observations. Expiry, gaps, changed bindings
  or uncertainty terminally fence it; it cannot issue authority. Two new fake-clock
  policy tests cover renewal/retry, exact/late expiry, retained last grant,
  restart-renewal rejection, owner/epoch changes, backward clocks and explicit
  fencing.
- `SupervisorControl` v1 in `crates/rpc/src/durable_execution.rs` is a deliberately
  separate protected-pipe control: `{"host_control":1,"lease":<app Lease>}`. A new
  Rust test checks strict round-trip decoding against the shared grant fixture.
  Existing runner request/response v1 envelopes are unchanged, but lease-free new
  execution now deliberately rejects. The matched component hash changes with
  executable source; no old-component fallback is added.
- The host uses the already-existing `forge-rpc` crate for that control's strict
  decoder rather than duplicating a wire type. `Cargo.lock` gains only that
  dependency edge, with no new package/version. The actual channel is the private
  stdin pipe held by the trusted spawning controller, not an agent tool, operator
  request body, credential-shaped JSON or retained file. The host does not
  independently query Forge's database or manufacture committed grants.
- `runner/src/main.ts` consumes host-validated controls, binds to actual worker,
  parent, boot and time namespace evidence, and emits a sequence-only `authorized`
  delivery event. Matching authorization is required before intent/writable
  opening, Harness opening, root creation and submission. Execution context and
  the sole synthetic provider use lease cancellation. No tools are configured in
  this executable; the separately tested wrapper covers current tool/nested paths
  for future explicit profile composition. Read-only observation/exact replay does
  not obtain a grant or reopen storage writable, including after expiry/restart.
- `crates/runner-host/src/watchdog.rs` serializes short clock/policy observations
  with control acceptance. Clock reads inside the policy critical section avoid
  a poll/renewal race being misdiagnosed as a backward clock. Worker pipe writes
  and process/filesystem observations happen outside that mutex; a full pipe or
  stopped JavaScript cannot block independent expiry polling.
- `crates/runner-host/src/supervision.rs` borrows the directly spawned unreaped
  Child. Before every group signal it checks protected directory/evidence,
  live parent/start/group identities, boot/time namespace and both inherited
  kernel flock descriptions. It never adopts PID text; if the leader exited or
  identity is uncertain it refuses a retained numeric group ID. Descendant
  absence remains unknown, not inferred from a successful wait or signal.
- The host polls every 50ms while scheduled, attempts TERM on expiry/uncertainty,
  then attempts KILL once after 5 seconds of boottime grace. Clock failure or
  backward readings use `Instant` only for cancellation grace, never permission.
  These bounds require OS scheduling/filesystem progress and do not guarantee
  universal termination. The host retains its ownership lock indefinitely.
- Before signalling it attempts an owner-only no-follow `watchdog.json` marker;
  per-stage 0600 JSON files retain sent/unverified signalling evidence and the last
  grant, with file/directory sync attempts. Diagnostic failure is reported with
  stable codes and still initiates cancellation/retains the flock. Writes recheck
  captured directory device/inode, canonical boundary, owner and permissions.
  Existing markers are never overwritten/removed, and any marker entry (including
  malformed content or a dangling symlink) blocks new grants after restart while
  preserving read-only access. Owner evidence rename now syncs its directory too.

This does **not** wire `DaemonAuthority` into daemon launch/delivery/renewal.
Executable tests synthesize controller grants; they are not evidence of store
commit followed by daemon delivery, 10-second renewal scheduling, or real daemon
connectivity loss. Fresh-epoch issuance and persisted rejection remain established
by earlier app/store tests, not by the synthetic controller. Runtime watchdog
files supplement but do not replace Forge store diagnostics or the existing T03
reservation/blocker; no code clears capacity, closes an Assignment, deletes a
marker, signals unrelated retained processes or repairs a worktree.

Five new Node executable tests prove: no-grant/mismatched execution creates no
runtime DB; stalled JavaScript plus a filled worker input pipe still expires and
receives TERM/KILL attempts; uncertain owner evidence refuses signalling while
retaining marker/lock; exact retries and sequential renewal survive the original
deadline while polling competes; foreign/malformed/wrong-version controls grant no
permission and leak no sentinel payload. The stopped-JS test observes actual KILL
exit, retained lock contention, unchanged committed inspection, read-only restart
and rejected new authorization without overwriting the marker. The uncertain-owner
test establishes that the original worker remains live after signal refusal,
not that descendants are absent. Tests use controlled absolute boottime boundaries
or explicit events; production clocks/timings were not shortened by test flags.

On the corrected snapshot these commands pass:

```sh
cargo build -p forge-runner-host
cargo test -p forge-runner-host --test authority
(cd runner && npm run prepare-reader && npm run check && npm test)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

**604 Rust tests passed, 0 failed, 1 opt-in probe ignored; 27 Node runner/process
tests passed.** The derived reader hash is unchanged. Initial clippy runs caught
missing documentation backticks, a needless Arc value parameter, formatting and
function-size findings; those were corrected before final gates. Review found
and fixed the potential clock-observation ordering race and strengthened retained
diagnostic directory checks. A later full default-parallel gate run failed in
the unchanged server test
`store_ownership_lock_rejects_a_second_owner_and_releases_for_a_later_one`: immediate
reacquisition after dropping the first owner returned WouldBlock. The isolated
test and a subsequent **full default-parallel workspace rerun** both passed
(604 passed, 1 ignored). The intermittent cause was not established or fixed;
this is retained verification uncertainty, not concealed by the passing retry.
No npm install, packaged release, real provider or production-daemon integration
verification ran.

Implementation self-review: T05's “A runner can execute only within current Forge
authority” remains partial because the trusted daemon orchestration boundary is
unwired. “Cancellation failure retains diagnostics/capacity” has runtime-file and
lock evidence plus earlier store reservations, but not integrated persisted
daemon-to-runner outage/restart evidence. No scope creep or incorrect implemented
requirement was found after correction. Standards self-review found no introduced
violation: application owns lease policy, RPC owns the new control shape, host
I/O stays outside store transactions, and all uncertainty remains retained. This
is not T18 independent review. **T05 is incomplete; T06 remains blocked.**

## Committed delivery, canonical binding and composed-outage increment

### Delivered interfaces and failure semantics

- `crates/app/src/authorization_lease/session.rs` owns one fresh-epoch renewal
  Session and the trusted `SessionRepository`/`Delivery` ports. A tick reads fresh
  host evidence, waits until the absolute 10-second renewal deadline, or validates
  and commits the next 30-second grant. It reobserves after persistence before
  bounded delivery: a slow commit crossing the initial **or prior** deadline
  cannot revive execution. No heartbeat/acknowledgement is an issuance input.
- Any observation, policy, persistence, delivery or stop failure terminally
  fences the Session. The first cause is preserved. Later calls may retry only
  diagnostic retention, not observation, issuance or uncertain delivery. Lost
  commit replies remain unknown even if history committed; there is no local
  retransmission or deadline extension.
- `forge_runner_host::control::ControlPipe` takes the directly spawned host's
  private stdin writer and makes it nonblocking. A control is serialized within
  the existing 1MiB bound and written exactly once. Partial writes, a full pipe,
  interruption or errors report redacted uncertainty. A successful write is not
  worker acceptance, bootstrap, heartbeat, Outcome or proof of liveness.
- `crates/server/src/durable_authorization.rs` now composes Session, verified
  `SpawnedOwner`, the actual SQLite repository and ControlPipe. `tick` persists
  before delivery; `run` is a dedicated blocking-worker driver waking on app-owned
  absolute renewal times. Stop/controller disconnection is checked **before**
  first issuance and at renewal boundaries. Clock failure stops/fences the driver
  rather than falling back or accidentally issuing another grant.
- Binding derives its directory from the exact reserved runtime and immutable
  registered Workspace root. `prepare_directory` creates missing protected paths
  at `<workspace-root>/.forge/assignment-runtimes/<sha256(store_id)>`; `bind` only
  verifies an existing path and the actual directly spawned Child, accepting no
  caller-selected directory. Opaque IDs are never path components. Root/`.forge`
  must be owned, canonical and not writable by others; the runtime subtree is
  exactly 0700. Symlink, missing, foreign, mismatched and permissive paths reject
  before taking the control pipe. Partial preparation does not delete evidence.
- The host filesystem boundary lives in `crates/runner-host/src/storage.rs`; the
  server reuses its existing SHA-256 dependency for opaque store ID mapping.
  Pipe/owner capture failures retain scoped uncertainty before any grant. No
  retained PID, grant file or watchdog marker authorizes adoption.
- `SessionRepository::retain_failure` in the actual store atomically authorizes
  Workspace plus exact retained Launch, adds an `authorization-uncertain` resource
  with a fixed redacted cause code, and inserts a Dispatch Hold only if absent.
  It works after closure/freshness invalidation without reauthorizing execution.
  Exact retries retain timestamps, older holds remain unchanged, and both effects
  roll back on failure. Closed Assignments retain capacity and cannot be replaced.
  Unknown-resource resolution does not cover this new resource kind.
- Failure resources use the existing generic resource/hold contracts: no DDL or
  schema bump, fabricated historical grants, retirement or automatic clearance.
  Grant issuance already rejects these unresolved resource kinds. If diagnostics
  cannot commit, the prior durable-runtime reservation still blocks replacement.

### Verification

Four new app fake-clock tests cover due/early renewal and commit-before-delivery,
exact expiry, failed/backward/changed evidence, revocation, delivery loss, lost
commit replies, explicit stop, retention-only retries and slow commits crossing
initial/prior deadlines. One host unit test covers successful/partial/full-pipe
control writes without retries or payload disclosure. Two SQLite tests cover
atomic failure-resource/hold rollback, scope/identity rejection, exact retry,
preserved unrelated holds and capacity across closure/reopen.

Seven new **default-parallel**, real-process integration tests at
`crates/runner-host/tests/authorization_session.rs` compose the real SQLite store,
server authority/driver, nonblocking private pipe, Rust supervisor and pinned
Node consumer. The controlled `runner/test/session-worker.mjs` is test-only, not a
profile or production entrypoint. It explicitly installs faux providers/tools
with the production consumer guards and never loads ambient configuration.

1. A controlled model request stays pending during loss of renewal, with the pipe
   deliberately still open. The real, unshortened 30-second lease expires, one
   cancellation is observed, and a subsequent pinned-runtime submission produces
   no second provider request.
2. The analogous tool fixture starts one actual long-running Node child. Expiry
   requests cancellation and observes its SIGTERM exit; subsequent scheduling
   admits neither a second request nor another tool effect.
3. Both outage cases reobserve/persist the expired service failure, reopen Forge
   storage, close the Assignment and assert retained diagnostics/capacity and no
   replacement. A genuinely fresh authority epoch/supervisor is then rejected
   without changing runtime bytes, overwriting the watchdog marker or adding
   model/tool work. Retained grants are not restart permission.
4. A dedicated driver actually renews a committed sequence at 10 seconds, then
   stops with retained diagnostics/capacity rather than pretending cleanup.
5. Revoked initial work and a controller already disconnected before driver start
   receive zero grants and create no runtime DB.
6. A changed owner causes the service to persist uncertainty and the host to refuse
   TERM/KILL. The original worker remains live, the closed Assignment still
   reserves capacity across reopen, and replacement is rejected.
7. Canonical mapping tests reject foreign scope, changed/escaping IDs, permissive
   directories, missing paths and redirected ancestors without taking a pipe;
   separate failed-binding cases retain pipe/owner uncertainty before any grant.

These are service-level renewal-loss and restart tests, not a claimed crash of the
full production HTTP daemon binary or a real-provider Check cycle. Production
launch/capability/profile integration belongs to T06/T07; packaged, full-daemon and
real-provider release verification remains T19. The pre-tracer boundary follows
this plan's stated T03–T06-before-T07 order, rather than enabling dispatch early.

Test-only dependencies from the host to the existing server/store crates and
`rusqlite` permit this composed verification without shell-building a server or
substituting fake commits. There is no new package/version in Cargo.lock and no
production host-to-server dependency. Node 24 and the locked runner npm install
are now prerequisites for the default Rust workspace suite, documented in
`runner/README.md`; these tests are not hidden behind an ignore flag. Test PATH
is used only to discover the Node executable; launched components receive
absolute paths and a cleared environment.

On the corrected snapshot:

```sh
cargo build -p forge-runner-host
(cd runner && npm run prepare-reader && npm run check && npm test)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

All pass: **618 Rust tests passed, 0 failed, 1 pre-existing opt-in probe ignored**;
**27 Node runner/process tests passed**. The workspace total includes seven new
composed process tests (approximately 30 seconds in parallel), not seven extra
Node-suite tests. Reader hash is unchanged. Initial compile/lint runs caught a
SQLite count type, boxed enum/layout, pattern style and fixture-size findings;
those were corrected. Review additionally fixed initial-stop ordering and ensured
binding failures retain diagnostics. No clean npm install, installed package,
real-provider, live dispatch, bootstrap or Outcome verification ran. The earlier
intermittent unchanged server ownership test failure is retained above; it did
not reproduce on the final default-parallel gates and its root cause is not
claimed resolved.

### T05 completion review and successor handoff

Implementation self-review against the Task:

- “Revoked, stale, mismatched or unreachable authorization cannot start or resume
  work”: app/store denial, consumer expiry and composed initial/restart tests pass;
  no writable recovery operation is enabled.
- “No new model/tool admissions occur after expiry, including nested and
  compaction paths”: pinned-runtime consumer tests plus composed outage counts
  pass. Current executable tools remain empty; T06 must wrap every admitted tool
  when mapping profiles rather than bypass these guards.
- “Cancellation failure retains diagnostics/capacity”: actual service/store/host
  signal-refusal, closure, reopen and replacement-rejection evidence passes.
- “Heartbeats, capability acknowledgement and leases remain distinct facts”:
  renewal changes only grant history/delivery, never bootstrap/lifecycle/Outcome
  facts. No scope creep or incorrect implemented requirement remains in this
  reviewed pre-dispatch component.

Standards self-review found no introduced violation: app owns scheduling/policy,
RPC owns the existing control, server owns composition, and host owns process/FS
I/O (ARCH-1/2). Grant commits precede external delivery; resource/hold retention
is atomic (TX-1/2/4). All uncertain delivery/exit retains evidence and capacity
(SAFE-1/2/3); no cleanup or secret exposure was added. This is self-review, not
T18's independent completed-system review. **T05 acceptance is satisfied; T06 may
proceed.** Production durable dispatch remains disabled.

T06/T07 handoff:

1. Use explicit compatible profiles/authentication only. Wrap every registered
   provider/tool (including dynamic/nested paths); do not expose control delivery
   or grant issuance as an agent capability or operator-supplied JSON.
2. At the T07 trusted launch boundary, reserve immutable runtime state, prepare the
   canonical directory, spawn the matched host, read owned readiness, then bind
   the service using the repository/Workspace/Launch and actual Child. Retain one
   DaemonAuthority per daemon lifetime and drive its Session on a dedicated worker.
   Successful control writes alone do not authorize Task start or input replay.
3. Continue with T07's protected bootstrap/authenticated Outcome and admission
   correlation. Do not turn this component completion into default dispatch,
   arbitrary writable recovery, backend migration or installed-release readiness.
4. Keep runtime markers/resources/history/capacity until T09/T10/T14's explicit
   verified retirement, recovery and storage operations. Neither process exit nor
   lease expiry supplies cleanup permission.
