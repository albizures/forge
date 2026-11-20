# Forge durable runner — T04–T06 controlled execution boundary

This is a **synthetic-only development component**, not a production dispatch
backend or installed Forge Revision. `src/main.ts` remains synthetic-only: it uses
no Pi CLI, Herdr, credentials, ambient Pi configuration, tools, network provider,
Assignment capability or accepted Outcome. The daemon does not launch it. T06's
separate explicit profile factory configures the pinned OpenAI provider and
coding tools; tests use synthetic credentials and intercepted provider requests,
never a real account or live Assignment. T05's application policy,
trusted Linux evidence adapter, consumer guards and independent host watchdog are
present. The executable requires a grant before new execution. The daemon-side
service now owns canonical storage binding, commit-before-delivery, nonblocking
controls, renewal scheduling and durable failure retention; cross-layer tests
exercise it with the actual supervisor and pinned runtime. Production dispatch
still does not select or launch this backend. T06 provides tested explicit
profile/authentication mapping, T07 actual Check-bound dispatch, and T15 matched installation. Do not use
this harness to execute live Assignments.

## T06 explicit profile/authentication construction

`src/profiles.ts::createProfileRuntime` consumes the Forge-owned resolved
configuration (`test/profile-v1.json` is shared with Rust), a deliberately selected
credential-file path, the existing lease consumer and an explicit capability
metadata/path allowlist. It opens no runtime store, creates no conversation and
admits no work by construction alone. The model key stays in a private auth
context, not environment values or persisted configuration. Actual provider/tool
entry points retain lease guards. The trusted T07 launcher must bind Check,
profile and worktree identity, preflight before capability issuance, and gate
behavioral context behind successful authenticated bootstrap.

Supported configuration and the raw-file authentication/permission matrix are
in [Agent Profile policy](../docs/agent-profile-policy.md#durable-profiles-and-authentication-controlled-pre-dispatch-boundary).
No OAuth, Pi credential discovery, extensions/MCP, custom nested tools,
image/classifier/deferred APIs, automatic compaction or request retries are
supported. The four coding tools are all explicitly unsafe for replay; cwd and
shell environment are explicit, not a sandbox. Provider-event redaction prevents
exact configured-key echoes from being persisted; it does not make arbitrary
repository/tool output safe to disclose. Direct same-user access to credentials
is outside hostile-agent containment guarantees.

The host library performs the same protected raw-file preflight. Server operator
preview/registration additionally require explicit local model authentication
for durable profiles, without contacting the provider. The composed test fixture
`test/profile-preflight.mjs` consumes actual Rust-prepared input in an isolated
Node process and verifies configured model access still grants no execution
lease. Production launch, bootstrap and Outcome integration remain T07; this
module is not a second production entrypoint.

## T07 bootstrap gate increment (not dispatch integration)

`src/bootstrap.ts::bootstrapCheck` is a trusted composition function, not a tool
or runnable entrypoint. It consumes the persisted complete Check binding, launch,
freshness generation, protected capability metadata and an already granted lease.
It invokes only the delivered absolute CLI with `assignment bootstrap --json` in
the bound worktree. No shell, ambient environment or Pi discovery is involved.
The CLI, not this module, reads/authenticates the capability. Only a successful
version-1 response matching the complete binding releases context and behavior.
The selected lease must match this exact launch/generation before and after the
invocation. Output is bounded to 1 MiB; bootstrap times out after 10 seconds.

Failures, malformed replies, binding drift, cancellation and uncertain replies
fence this local lease consumer and return only a fixed redacted diagnostic.
They do not retry, accept an Outcome, prove cleanup or release capacity. A future
host caller must persist uncertainty before reconciliation/replacement. Unit tests
use directly spawned **fake CLI executables**. The separate Rust component tracer
uses the actual CLI/server and authenticated Outcome path.

`src/check-runtime.ts::createCheckRuntime` snapshots trusted launch inputs, checks
the bound cwd, then composes the explicit profile runtime only after successful
bootstrap. Authenticated behavior/context are added to the agent instructions;
model/tool construction failures also fence this consumer. Before bootstrap it
now validates the supplied runtime directory and reads committed interrupted-tool
state (see the T08 increment below). It opens no writable storage, creates no
conversation and supplies no execution authority of its own.
The caller still owns pre-capability preflight, exclusive ownership, admission,
supervision and durable uncertainty retention.

`cargo test -p forge-cli --test durable_check_bootstrap` additionally composes this
helper with a real pinned Harness/SQLite store and a deterministic faux provider.
The fake model invokes the actual guarded Bash tool, which submits the first
accepted Outcome using the delivered CLI against the real HTTP daemon/store.
Rust prepares the Outcome file and Git output evidence; the fake model does not
implement the code. The test checks two model calls, one successful tool result,
exact evidence/replay and absence of secret canaries in runtime storage. Its lease
is still a fixture, and the store Check claim bypasses app dispatch: this is not
supervised execution, a real-provider test or proof of exclusive process ownership.

The component now also reserves Forge runtime intent before bootstrap, journals
its exact Launch/root/input, and reads the committed answer through
`src/storage.ts::observeAnswer` after writer closure. That read-only helper returns
`answered`, `unanswered` or `unknown` only; it verifies full Launch and accepted
submission identity and cannot schedule or repair execution. Settled passive
writes are not answers. Read-only SQLite can still create empty WAL/SHM reader
coordination files; no runtime history is committed by this inspection.

The Rust tracer delivers actual answered observations to the existing
missing-Outcome persistence boundary. Separate accepted, prose-only and
rejected-Outcome-then-prose cases verify the distinction, actual human/JSON CLI/RPC
diagnostics and retained capacity across fixture closure/reopen. This is component
delivery with fixture authorization, not a supervised production caller.

`main.ts` and production dispatch do not call the bootstrap/runtime helpers.
Actual supervised Check dispatch, capability issuance ordering and supervised
host answer delivery remain outstanding T07 work; see the
[T07 evidence/handoff](../docs/research/pi-durable-t07/README.md).

## T08 interrupted-effect admission fence (partial)

`src/interrupted-effects.ts` scans committed `pi.tool` v3 tasks through the pinned
physical read-only adapter, before `Harness.open` can settle interrupted calls or
schedule a model. Full Launch identity must match. Unsettled execution intents and
failed/aborted/faulted/orphaned receipts block reopening; completed tool results do
not. The scan includes child conversations, is paginated and bounded, and fails
closed for unknown tool contracts. No automatic replay-safe exception is approved.
Intent, arguments and results remain in protected runtime storage; the returned
identities contain no command text or tool output.

`createCheckRuntime` requires an explicit protected runtime directory and performs
this fence before authenticated bootstrap. Its caller must already hold exclusive
host ownership, as for writable opening. Failure closes that lease consumer; it
does not revoke Forge authority or prove process/child absence. Inspection never
repairs runtime state or changes Git/worktree contents.

**This is not complete T08 or usable recovery.** Forge-visible interrupted-effect
resource/audit persistence, operator resolution, and integration into a future
recovery controller are not implemented. There is no resolution command, no
permission to rerun an interrupted command, and no exactly-once guarantee. Do not
edit runtime SQLite or clear an ordinary Dispatch Hold to bypass this fence.
Production durable dispatch and automatic recovery remain disabled. See the
[T08 evidence and remaining work](../docs/research/pi-durable-t08/README.md).

## Reproduce

Supported/tested host: Linux x64, local filesystem with `flock`, Node **24.18.0**,
npm **11.16.0** (SQLite **3.53.1**). Other platforms are not supported yet.
From the Forge repository root:

```sh
cargo build -p forge-runner-host
cargo test -p forge-runner-host --test authority
cd runner
npm ci --ignore-scripts --no-audit --no-fund
npm run prepare-reader
npm run check
npm test
cd ..
cargo test -p forge-runner-host --test authorization_session
```

Node's native TypeScript stripping runs the source; TypeScript and `@types/node`
are development-only dependencies for strict type checking. The runtime's three
exact 1.5.2 dependencies supply persistence, the synthetic provider and explicit
execution context; their transitive closure/integrities are locked. `fs2` provides
the existing workspace flock mechanism; `rustix` supplies safe no-follow flags,
process UID and fallible CLOCK_BOOTTIME reads. `serde`/`serde_json` decode protected
identity facts. These packages already belong to the workspace dependency closure;
the host library implements the `forge-app` trusted-evidence interface without
introducing runtime/provider types into application policy. The host also depends
on the existing `forge-rpc` crate to reuse the strict supervisor control contract,
not a second hand-maintained grant decoder; no package/version was added to the
workspace closure. Test-only dependencies on the existing server/store crates and
`rusqlite` permit real commit/service/host integration without compiling a second
server or substituting protocol-only fakes. The host's production dependencies do
not depend on the server; the reverse test edge adds no package/version.

## Boundary and local protocol

`forge-runner-host RUNTIME_DIRECTORY WORKTREE ABSOLUTE_NODE ABSOLUTE_RUNNER`
requires an existing canonical owner-owned 0700 runtime directory separate from
the canonical worktree, and explicit absolute component paths. It acquires a
nonblocking exclusive flock on a persistent 0600 no-follow single-link lock inode
**before spawning the worker or opening SQLite**. It never unlinks a stale lock.
This development command does not look up Forge's store or authorize Assignments.
The trusted server service now derives its path from the exact T03 reservation:
`<workspace-root>/.forge/assignment-runtimes/<sha256(store_id)>`. Preparation creates
only missing protected directories; binding does not create missing paths. Root
and `.forge` must be canonical, owned and not writable by other users; the runtime
subtree is exactly 0700. Symlinks, foreign scope, changed Launches and permissive
runtime directories fail closed, and opaque IDs never become path components.
Partial preparation preserves its reservation/directories rather than cleaning up.

The host creates a separate process group, clears the environment, and records
boot ID, host/worker start identities, group and lock inode/device. It supplies a
protected first-line handshake after that evidence commits. The worker verifies
its direct parent and retained identity, including its inherited descriptor for
**the same flock open description**. Both processes retain ownership: a live
worker fences contenders if the host dies; the host fences contenders if the
worker dies. Descriptive PID text never authorizes adoption or termination.
Host stdout is the worker's newline-delimited JSON stream; stderr is diagnostic.

After `event: owned`, requests use the internal Rust `RunnerRequest` v1 envelope
(`crates/rpc/src/durable_execution.rs`). `admit` and `observe` return correlated
`RunnerResponse` objects. Readiness/settled events are synthetic harness events,
not accepted Outcomes or Forge lifecycle facts. New `admit` execution additionally
requires prior trusted supervisor authorization; an ungranted request returns
`assignment-lease-denied` before creating SQLite. Exact read-only replay and
observation do not require a new grant. Errors carry only stable codes,
not echoed input or upstream exception details. Unknown protocol/schema/component,
unknown fields, unavailable operations and identity conflicts fail closed.
The shared `test/wire-v1.json` fixture is checked by Rust and TypeScript.

The component digest covers the lockfile, read-adapter derivation and executable
runner sources. This is local compatibility checking, **not** T15's installed
component/Node/host attestation. Explicit component paths are not an authorization
or installation proof.

One 0600 `runtime.sqlite` contains the upstream schema **2** plus an independently
versioned `forge_runner_identity` table (version **1**). Intent records the entire
immutable T03 Launch before creating a root/submitting input. The root's upstream
ID is retained separately from the opaque Forge conversation ID. The one fixed
synthetic input uses the launch operation ID as the upstream request ID. Exact
retries compare the entire canonical Launch and read the original committed
submission ID; changed payloads fail before runtime admission. The accepted result
is derived from that upstream committed submission, not a second uncoordinated
accepted-result write.

Root creation and submission are separate upstream commits. A crash before the
root mapping or submission commit remains **unknown**. Existing stores are never
reopened writable by an ordinary retry, even if the ownership lock is free.
Thus crashes cannot cause task repair, arbitrary replay or a second intended
submission. Authorized same-attempt writable recovery is T05/T08/T10 work.

The host deliberately retains its lock after worker exit, pending reconciliation.
There is no production stop, adoption or cleanup command here. The isolated tests
terminate only their own directly spawned processes. T09 must implement retirement
and verified cleanup; lock freedom, process exit, successful signals and labels
alone remain insufficient cleanup evidence. If both owner processes die, Forge's
T03 capacity reservation/blockers still must gate replacement. No worktree bytes
are changed by this synthetic runner. Process separation is not a hostile-agent
sandbox; same-user malicious processes and shared-host/NFS execution are excluded.

## T05 trusted daemon evidence seam

`forge_runner_host::SpawnedOwner` borrows a live directly spawned Rust `Child`
after readiness. It is not a PID-based adoption API. Each observation verifies
canonical 0700 directory identity, protected bounded 0600 owner evidence, live
host/worker ancestry/start/group identities, boot and matching Linux time
namespaces, lock inode/device and exclusive WRITE FLOCK entries in **both**
inherited descriptors' `/proc/PID/fdinfo`. Mere inode equality or a global lock
lookup is not authority. Only then does it supply CLOCK_BOOTTIME evidence; missing,
changed, backward or unreadable evidence terminally fences that observer.

The server's `durable_authorization` service seam creates a fresh 256-bit daemon
lifetime epoch, binds only a directly spawned verified owner and delegates grant
construction/persistence to the app/store. `prepare_directory` validates the
reservation before filesystem creation. `bind` accepts a repository/Workspace/
Launch and the actual directly spawned Child, not a directory supplied by an RPC;
it derives and verifies the existing canonical path, takes the private pipe and
captures host evidence. Pipe/owner binding failures retain a scoped blocker.
`tick` implements commit-before-delivery; `run` is a blocking driver for a dedicated
daemon worker at absolute app-owned renewal times. Stop/disconnected controller
requests are checked before initial issuance, not after starting work. Any failure
terminally fences renewal and attempts atomic resource/hold retention, preserving
older holds. A suspended/late driver cannot move the boottime deadline; on resume,
consumer/watchdog checks deny expired work and the driver cannot renew it. Failed
retention can retry diagnostics only. The service is not an
operator RPC and is not selected by production dispatch yet. No owner/clock/epoch
payload supplied by an agent becomes permission.

The eight `authority` tests use the actual Rust supervisor and an inert Python
worker with explicit readiness/acknowledgement channels. They require
`/usr/bin/python3` (test-only, standard library only), not credentials or Node.
They test kernel evidence, stale/replaced identities/files, lost descriptors,
unlocking and supervisor/worker exit—not runtime lease expiry or safe cleanup.
These observational checks are not hostile-agent containment or an atomic
guarantee that the process cannot exit/change after an observation. Separate
executable tests below now exercise lease enforcement and watchdog expiry.

## T05 protected control and consumer guards

`src/authorization.ts` consumes the app's serialized grant shape. A new separate
`SupervisorControl` v1 line on the directly spawned host's private stdin pipe is
`{"host_control":1,"lease":<committed app Lease>}`. The host strictly validates
version, shape, live owner coordinates, sequence and absolute bounds before
forwarding it to the worker. The worker independently binds it to its verified
parent/worker/boot/time namespace, then emits `{"event":"authorized","sequence":N}`.
The acknowledgement is delivery observation, not bootstrap, heartbeat or Outcome.

The pipe's write end belongs only to the trusted spawning controller. JSON fields
are **not authentication**: never forward agent/operator payloads as controls or
load retained grants as permission. Production daemon composition must create a
fresh epoch, validate current Forge authority and persist every grant through the
existing app/store service before delivery, renewing every 10 seconds. The trusted
server service now implements that commit/delivery/renewal loop. The standalone
Node tests still simulate the controller; separate Rust integration tests use the
actual SQLite-backed service. The local command does not authorize Assignments
in Forge.
Existing `RunnerRequest`/`RunnerResponse` v1 envelopes are unchanged, but lease-free
new execution is intentionally denied. Changed executable sources change the
matched component digest; no silent compatibility fallback is supported.

The consumer starts without permission, checks absolute 30-second boottime
bounds, accepts only sequential renewals or exact retries, and permanently aborts
on invalid/late grants, expiry, clock failure or backward observations. It never
adopts a retained grant on startup. `/proc/uptime` is rounded up to the next
centisecond for conservative admission; the trusted host must first verify boot
and time namespace equality. Unsafe JSON integers fail closed. A 50ms cooperative
poll requests cancellation even without another admission; it is not a hard
cancellation deadline when JavaScript is stalled or the host is suspended.

`guardProvider` gates the pinned provider's stream, simple stream, deferred fetch,
image and classifier request methods; cancellation stays callable. `guardTool`
checks at actual execution rather than relying on a preflight hook. Both combine
caller and lease cancellation and deny already-cancelled calls. Future registry
composition must guard **every** provider/tool, including dynamic tools. These
wrappers do not implement replay/recovery policy or prove child exit. `close()`
terminally aborts instead of merely removing the cancellation timer. `main.ts`
now gates before intent/writable opening, Harness opening, root creation and
submission, combines execution cancellation and wraps its sole synthetic provider.
It accepts no tools; future profile mapping must apply the tested tool guard to
every admitted tool. Existing runtime stores still cannot be reopened writable
by an ordinary retry, even with a valid lease.

Twelve new tests cover clock/renewal/restart failures, the cooperative timer,
all five request methods and signal composition. Actual pinned Harness tests use
controlled provider responses to cover initial and same-store resumed requests,
in-flight model cancellation, explicit compaction, direct/nested tool preflight
expiry and one cooperative long-running Node child. The shared
`test/authorization-lease.json` fixture is also validated and round-tripped by the
Rust app. These consumer tests are not production-daemon or real-provider release
tests. Production durable launch remains disabled.

## Independent supervisor watchdog

The Rust host applies app-owned `LeaseWatchdog` policy independently of JavaScript.
It polls CLOCK_BOOTTIME and live owner evidence every 50ms while scheduled. A
blocked input-to-worker write never holds its policy mutex. Only short serialized
clock/policy checks share that mutex, avoiding false backward-clock races between
renewal delivery and polling. Bounds, discontinuities and terminal fencing mirror
the consumer; neither process can renew itself.

On expiry/uncertainty the host attempts to retain an owner-only no-follow
`watchdog.json` marker, then requests TERM for its directly spawned, reverified
process group. After 5 seconds of boottime grace it attempts KILL once. When the
clock is unavailable/backward it uses monotonic `Instant` elapsed time for the
cancellation grace only, never to extend authority. These are initiation bounds
subject to OS scheduling and filesystem progress, not universal death guarantees.

Every signal rechecks the actual unreaped Child, parent/start/group identities,
boot/time namespace, protected directory and both shared flock descriptors. If
the leader exited or evidence is uncertain, signalling is refused rather than
using retained numeric IDs. Descendants may survive; cleanup stays unverified.
Per-stage 0600 JSON evidence records signal sent/unverified, with file/directory
sync attempts and stable diagnostics on failure. The host retains its flock
indefinitely even after worker exit or successful signals. No worktree repair,
Forge lifecycle update, capacity release or diagnostic deletion occurs.

A retained marker (even malformed or a dangling symlink) blocks future grants;
read-only observation/replay remains available. Marker persistence failure still
initiates cancellation and retains the live lock; Forge's T03 durable reservations
must continue to block replacement after both processes die. This marker is not a
replacement for Forge store blockers or an operator resolution API (T09/T11).

Five additional executable tests cover no-lease/mismatched admission with no
SQLite creation, expiry with SIGSTOP-stalled JavaScript **and a full worker pipe**,
TERM/KILL escalation, changed-owner signal refusal, retained locks/diagnostics,
read-only restart, live renewal/exact retries and malformed/foreign controls with
redacted diagnostics. Together with existing tests, **27 Node tests pass**. No
binary-daemon crash, real-provider or packaged release verification is claimed by
those Node tests.

## Composed service/store/runtime verification

`cargo test -p forge-runner-host --test authorization_session` now runs seven
isolated, default-parallel integration tests using the actual SQLite repository,
server service, private nonblocking pipe, Rust supervisor and pinned Node runtime.
It requires Node 24 on the **test** executable-discovery PATH and the locked runner
npm installation; the launched host still clears its environment and uses absolute
component paths. It is not ignored or replaced by an opt-in release probe.
Consequently `cargo test --workspace` now needs that documented Node/npm setup.

A test-only `test/session-worker.mjs` uses explicit faux provider/tool definitions
and the production consumer guards; it is not a profile or shipped production
entrypoint. During real unshortened 30-second renewal outages, tests count one
controlled pending model request or one tool child, observe cancellation (including
actual child SIGTERM exit), then request another pinned-runtime turn and verify no
second model/tool admission. They reopen Forge storage, retain diagnostics and
closed-Assignment capacity, and restart a fresh authority/supervisor without
reviving old grants or changing committed runtime bytes. Separate tests exercise
10-second committed renewal, driver stop/disconnection before first issuance,
revoked initial permission, failed pipe/owner binding, uncertain-owner signal
refusal and canonical mapping attacks.

The pipe adapter makes exactly one nonblocking control write: a full pipe,
partial write or write error becomes retained uncertainty, never local replay or
an unbounded wait.
Application fake-clock tests cover exact renewal/expiry, backward/failed evidence,
missed deadlines across slow commits and uncertain commit replies. Store tests
verify atomic resource/hold rollback, scope, idempotency, old-hold preservation and
retained capacity across closure/reopen. No new DDL, protocol version, capability,
bootstrap acknowledgement, Task completion or cleanup permission is introduced.
T05's controlled pre-dispatch authorization deliverable is complete; real
profile/bootstrap/Outcome dispatch remains T06/T07 and release verification T19.

## Committed read-only inspection and provenance

`node runner/src/inspect.ts RUNTIME_DIRECTORY WORKTREE` is a **local bounded
snapshot diagnostic**, not the T11 operator CLI/RPC. It returns retained identity
and at most 100 committed raw entries, indicating truncation. It never creates a
Harness, starts a scheduler, initializes a database, repairs tasks, submits input
or acquires writable ownership. It works beside a writer and with the worker
stopped. Full stable transcript pagination belongs to T11.

The maintained `prepare-reader.mjs` derives an explicitly named sibling module in
the local ignored dependency installation; it never modifies the stock module.
It checks the full upstream source hash, replaces migration with exact schema
validation, and refuses commit/ID allocation. Physical read-only SQLite enforces
no writes. The runtime checks the derived hash before loading it. The inherited
read/decoding implementation is pi-durable 1.5.2 from upstream tag `v1.5.2`, commit
`b4afdddf44e059c67ccf5097e5317d913521b470`, MIT; see `UPSTREAM-LICENSE` (copied from
that source revision's root LICENSE). Source:
<https://github.com/earendil-works/pi/tree/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable>.

- Upstream storage module SHA-256: `0d9d8cf56898afe5d38f0e56bd3cbcad23a8e68b942b81c39587ac731fe48642`.
- Derived module SHA-256: `b7801ce1290d4fc33d95848fbab5e8981fb2b47e9497650f5049c00c474c62aa`.

The generator is an explicit build step, not an npm lifecycle mutation. Dependency
upgrades must revalidate the derivation and read/process probes; T15 must package
and attest it. SQLite WAL / synchronous NORMAL offers process-crash persistence;
these tests do not establish physical power-loss durability, exactly-once external
effects or full recovery/release readiness.
