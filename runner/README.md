# Forge durable runner

This TypeScript package is the Forge-owned runner for durable Task execution. It
consumes a pinned pi-durable release (1.5.2) directly, not the Pi CLI, behind the
versioned Forge runner protocol 1. The daemon spawns it per Assignment under a
supervising `forge-runner-host`, in the Task's Managed Worktree, after authorization;
Forge's Rust application keeps all scheduling, Check, evidence and Outcome
authority. It ships with a Forge Revision as a matched bundle, together with the
pinned Node and the `forge-runner-host` built from the same commit
(`package-bundle.mjs --runner-host`). Never run it from `PATH`/npm or against live
Assignments by hand. New durable launches are withheld by the release gate in
production Revisions.

Operator documentation is in [Durable Task execution](../docs/durable-execution.md),
including the [upstream pi-durable constraints](../docs/durable-execution.md#upstream-pi-durable-152-constraints)
that shape this package. Recovery and retirement procedures are in
[Assignment recovery](../docs/assignment-recovery.md).

## Layout

| Path | Role |
| --- | --- |
| `src/check-entry.ts` | Packaged entry (Forgeup `ENTRY_RELATIVE`); runs `check-runner.ts::runCheckRunner` with no substitution |
| `src/check-runner.ts` | One process: first admission, read-only observation, same-attempt recovery, follow-ups |
| `src/check-runtime.ts` | Snapshots launch inputs, runs the effect fence, composes the profile runtime after bootstrap |
| `src/bootstrap.ts` | Runs `forge assignment bootstrap --json` through the delivered CLI |
| `src/profiles.ts` | Builds model, tools and private auth from the resolved Forge profile |
| `src/authorization.ts` | Lease consumer, `guardProvider`, `guardTool` |
| `src/interrupted-effects.ts` | Committed-effect scan, `guardEffectTool`, `meteredEnv` |
| `src/recovery.ts`, `src/recovery-consumer.ts`, `src/resume-runtime.ts` | Recovery candidate inspection, waiting consumer, writable resume |
| `src/followup.ts` | Idempotent follow-up delivery |
| `src/storage.ts`, `src/owner.ts`, `src/contract.ts` | Store/intent/identity, verified ownership, wire contract and component hash |
| `src/inspect*.ts` | Read-only subprocess entries: effects, recovery, answer, transcript, local snapshot |
| `prepare-reader.mjs` | Derives the physically read-only storage module (below) |
| `package-bundle.mjs` | Builds the matched bundle |
| `test/` | Node tests, fixtures and test-only workers (`synthetic-runner.ts`, `session-worker.mjs`, faux entries) |
| `test/upstream/` | Probes of pinned upstream behaviour, run with `npm run test:upstream` |

## Entry point and lifecycle

The packaged entry's input is the protected owner-only `check-runtime.json`
(`CheckRuntimeInput` v1), written once before spawn by
`forge_runner_host::check_runtime::write`. That immutable file is also the input of
every later recovery, which must match its launch and attempt exactly.

Effect inspection, bootstrap, the explicit profile, model-key loading and the
guarded tools all precede the runtime intent and the writable store. A failure
before intent is a `rejected` admission; after it, `unknown`. Tests substitute only
the model provider (`ProviderSubstitution`); the test workers are thin wrappers, and
the unsubstituted entry refuses without bootstrap, profile, model authentication or
lease.

**Bootstrap.** `bootstrapCheck` consumes the persisted Check binding, launch,
freshness generation, protected capability metadata and an already granted lease.
It invokes only the delivered absolute CLI with `assignment bootstrap --json` in the
bound worktree: no shell, ambient environment or Pi discovery. The CLI, not the
runner, reads and authenticates the capability. Only a successful version-1 response
matching the complete binding releases context and behaviour. The lease must match
this exact launch/generation before and after the call. Output is bounded to 1 MiB;
bootstrap times out after 10 seconds. Failures, malformed or uncertain replies,
binding drift and cancellation fence the lease consumer and return only a fixed
redacted diagnostic; they never retry, accept an Outcome, prove cleanup or release
capacity.

**Answer observation.** `storage.ts::observeAnswer` reads the committed answer after
writer closure and returns only `answered`, `unanswered` or `unknown`, verifying the
full Launch and accepted submission identity. Settled passive writes are not
answers. A runtime answer is never an Outcome: Forge records a Missing Outcome
diagnostic instead.

**Shutdown on EOF.** When the daemon's pipe closes, the runner exits through an
unhandled `Harness is closed` rejection. This is safe (the host proves exit before
anything is released) but not a clean shutdown.

## Profile and model authentication

`profiles.ts::createProfileRuntime` consumes the Forge-resolved configuration
(`test/profile-v1.json` is shared with Rust), a deliberately selected credential-file
path, the lease consumer and an explicit capability metadata/path allowlist. Building
it opens no store, creates no conversation and admits no work. The model key stays in
a private auth context, never environment values or persisted configuration.
Provider retries are disabled (`maxRetries: 0`).

Supported configuration and the raw-file authentication matrix are in
[Agent Profile policy](../docs/agent-profile-policy.md#durable-profiles-and-authentication).
No OAuth, Pi credential discovery, extensions/MCP, custom nested tools,
image/classifier/deferred APIs, automatic compaction or request retries are
supported. cwd and the shell environment are explicit, not a sandbox.
Provider-event redaction prevents exact configured-key echoes from being persisted;
it does not make repository or tool output safe to disclose.

## Local protocol

`forge-runner-host RUNTIME_DIRECTORY WORKTREE ABSOLUTE_NODE ABSOLUTE_RUNNER`
requires an existing canonical owner-owned 0700 runtime directory separate from the
canonical worktree, and explicit absolute component paths. The server derives the
directory from the reservation
(`<workspace-root>/.forge/assignment-runtimes/<sha256(store_id)>`); preparation
creates only missing protected directories and binding never creates paths. Root and
`.forge` must be canonical, owned and not writable by other users. Symlinks, foreign
scope, changed Launches and permissive directories fail closed; opaque IDs never
become path components; partial preparation is preserved, not cleaned up.

After `event: owned`, requests use the Rust `RunnerRequest` v1 envelope
(`crates/rpc/src/durable_execution.rs`) and return correlated `RunnerResponse`s and
typed `RunnerEvent`s. Readiness/settled events are runtime facts, not Outcomes or
Forge lifecycle facts. New execution requires a prior supervisor grant; an ungranted
`admit` returns `assignment-lease-denied` before creating SQLite. Exact read-only
replay and observation need no new grant. Errors carry only stable codes. Unknown
protocol/schema/component, unknown fields, unavailable operations and identity
conflicts fail closed. `test/wire-v1.json` is checked by both Rust and TypeScript.

**Control pipe.** The directly spawned host's private stdin carries
`SupervisorControl` lines, for example `{"host_control":1,"lease":<committed Lease>}`,
plus `host_prepare_recovery`, `host_recovery` and `host_followup`. Each control is at
most 1 MiB and written exactly once, nonblocking: a full pipe, partial write or error
becomes retained uncertainty, never a local retry or unbounded wait, and all pipe
clones then share one terminal framing fence. The host strictly validates version,
shape, owner coordinates, sequence and bounds before forwarding; the worker binds the
control to its verified parent, boot and time namespace and acknowledges with
`{"event":"authorized","sequence":N}` (delivery only, not bootstrap or liveness).
JSON fields are **not authentication**: never forward agent/operator payloads as
controls or load a retained grant or audit as permission.

**Storage.** One 0600 `runtime.sqlite` holds upstream schema **2** plus a separately
versioned `forge_runner_identity` table (version 1). Intent records the whole
immutable Launch before a root is created or input submitted. The upstream root ID is
kept separately from the opaque Forge conversation ID. The launch operation ID is the
upstream request ID; exact retries compare the entire canonical Launch and read the
original committed submission, and changed payloads fail before runtime admission
(upstream `requestId` lookup does not detect changed content). Root creation and
submission are separate upstream commits, so a crash between them stays **unknown**.
An ordinary retry never reopens an existing store writable, even with a free lock.

The component digest (`contract.ts::componentHash`) covers the lockfile, the
read-adapter derivation and executable runner sources, and is the Launch
`component_hash`. Changed sources change the digest; there is no compatibility
fallback.

## Ownership and supervision

The host acquires a nonblocking exclusive flock on a persistent 0600 no-follow
single-link lock inode **before spawning the worker or opening SQLite**, and never
unlinks a stale lock. It creates a separate process group, clears the environment,
records boot ID, host/worker start identities, group and lock inode/device, then hands
the worker a protected first-line handshake. The worker verifies its direct parent
and that it inherited **the same flock open description**. Both processes hold
ownership: a live worker fences contenders if the host dies, and vice versa. PID text
never authorizes adoption or termination. Host stdout is the worker's JSON stream;
stderr is diagnostic.

`forge_runner_host::SpawnedOwner` borrows the live directly spawned `Child`; it is not
a PID adoption API. Each observation verifies directory identity, protected owner
evidence, live ancestry/start/group identities, boot and time namespaces, lock
inode/device and an exclusive `WRITE FLOCK` entry in **both** inherited descriptors'
`/proc/PID/fdinfo`. Inode equality or a global lock lookup is not authority. Missing,
changed, backward or unreadable evidence terminally fences that observer.

The host forwards socket input to the worker through an explicit bounded buffer:
Linux `io::copy` (splice) from a socket into a pipe held the destination pipe while
waiting for more input and blocked worker readiness.

The host retains its lock after worker exit until reconciliation. Lock freedom,
process exit, successful signals and labels are never cleanup evidence; if both
owners die, Forge's capacity reservation and blockers still gate replacement. Process
separation is not a sandbox; same-user malicious processes and shared-host/NFS
storage are out of scope.

## Authorization leases and watchdog

The consumer in `authorization.ts` starts without permission, checks absolute
30-second `CLOCK_BOOTTIME` bounds, accepts only sequential renewals (sequence 1 first)
or exact retries, and permanently aborts on invalid or late grants, expiry, clock
failure or backward observations. It never adopts a retained grant on startup.
`/proc/uptime` is rounded **up** to the next centisecond; the host first verifies
boot and time-namespace equality. Unsafe JSON integers fail closed. A 50 ms
cooperative poll requests cancellation without waiting for another admission; it is
not a hard deadline if JavaScript is stalled or the host is suspended.

`guardProvider` gates the stream, simple stream, deferred fetch, image and classifier
request methods (cancellation stays callable); `guardTool` checks at actual
execution. Both combine caller and lease cancellation. Hooks are not used for
authorization because upstream swallows hook exceptions. Every provider and tool,
including any future dynamic tool, must be wrapped. `close()` aborts terminally.

The daemon side (`durable_authorization`) creates a fresh 256-bit daemon epoch per
lifetime, binds only a directly spawned verified owner, **commits each grant before
delivering it**, and renews every 10 seconds at absolute app-owned times. Any failure
terminally fences renewal and retains a resource and hold; a late driver cannot move
the deadline.

The host's `LeaseWatchdog` runs independently of JavaScript, polling
`CLOCK_BOOTTIME` and owner evidence every 50 ms; a blocked write to the worker never
holds its policy mutex. On expiry or uncertainty it retains an owner-only
`watchdog.json` marker, sends TERM to its reverified process group and, after 5
seconds, attempts KILL once. With an unavailable clock it uses monotonic time for the
grace only, never to extend authority. Every signal rechecks the unreaped Child,
identities, namespaces, directory and both flock descriptors; if evidence is
uncertain, signalling is refused. Per-stage 0600 evidence records each signal.

**Post-fence quiescence.** The host stays subreaper and writes
`watchdog-receipt-<lease sequence>.json` only after it positively reaped its worker,
the kernel reports ECHILD, storage and lock identity are unchanged, and a marker and
accepted lease exist. The receipt binds the marker bytes, owner and lease; the host
then exits, releasing the flock. An escaped descendant, an unverified signal or
ownership drift writes `watchdog-<pid>-<start>-quiescence-unproven.json` and keeps the
flock. A later fence in a recovered directory writes
`watchdog-<pid>-<start>-fence.json`, since `watchdog.json` is retained history.

**Pre-grant issuer loss.** If the private pipe closes before any accepted grant, the
host fences with `runner-issuer-lost` and a marker naming no lease, stops its worker,
and after the same proof writes `ungranted-receipt-<host pid>-<host start>.json`. A
directory sealed as never owned (`launch-sealed.json`) is refused before the host
records ownership.

A retained unsettled marker (even malformed or a dangling symlink) blocks future
grants; read-only observation stays available. The runner's `unblocked` check accepts
a marker only when a receipt binds its exact bytes (an ungranted receipt only for a
marker with no last lease). Forge, not the runner, decides recovery or retirement.

## Interrupted effects

`interrupted-effects.ts` scans committed `pi.tool` v3 tasks through the read-only
adapter **before** `Harness.open`, which would otherwise reconcile interrupted calls
and schedule the model. The full Launch must match. A `call` checkpoint is not an
effect; an unsettled `execute` checkpoint blocks even if labelled replay-safe, as do
failed, aborted, faulted and orphaned receipts; completed results do not. The scan
covers child conversations, is paginated and bounded (1,000 pages / 1,000 blockers;
exceeding a bound denies), and fails closed for unknown tool contracts. Returned
identities contain no command text or tool output. No automatic replay-safe exception
exists for the coding tools.

At execution, `guardEffectTool` terminally closes the consumer when a call throws or
is cancelled, before another request could repeat an unknown effect. Returned
results, including nonzero shell exits, are receipts.

**Effect-free failures.** The pinned tools report ordinary mistakes by throwing
(`read` of a missing path, `edit` text not found, invalid input). The coding tools
reach the filesystem and shell only through `meteredEnv`, which counts every started
`ExecutionEnv` operation outside a fixed read-only list (fail-closed for writes,
removal, directories, temp files, `exec`, watchers, cleanup and any future method).
Content reads also count unless the target is a regular file or absent, because
opening a FIFO or device can consume data. A call that throws before any counted
operation, with neither the lease nor the call aborted, returns an ordinary error
(`Tool failed without any effect: …`, at most 2000 characters), committed as a
completed call. Anything else stays an interrupted effect.

Forge retains these identities as `interrupted-effect` resources plus a hold;
resolution is an operator command and never reruns the command.

## Recovery

`recovery.ts::inspectRecoveryCandidate` reads the original Launch, committed root and
exact accepted input admission read-only. Only a `queued` or `placed` original input
is a candidate; finished or failed inputs need intervention, and passive writes,
`continue` submissions, missing admissions and interrupted initialization cannot
manufacture one. The whole-store effect scan runs first.

`Authorization.forRecovery(launch, generation, owner, epoch, expectedGrant)` prepares
a waiting consumer bound to an exact claim/lease/evidence envelope from freshly
verified host composition; it grants nothing. Only `acceptRecovery` with that exact
committed envelope permits an initial sequence above one; ordinary consumers still
require sequence one and ordinary lease delivery cannot initialize a waiting
consumer. Exact envelope retries cannot extend deadlines; an old envelope after
renewal fences. These are internal composition methods: never build an expectation
from a retained audit or expose them through ordinary input.

The full daemon sequence (proof, claim, waiting owner, commit before delivery,
writable resume) is in
[Assignment recovery](../docs/assignment-recovery.md#same-assignment-durable-recovery-composed-with-matched-components-launches-withheld).

## Retirement

Retirement journals the revoked operation and original owner before private control
delivery. The host acts as a Linux subreaper, fences new grants, verifies live
ownership and persists intent before group TERM; the runner closes authorization and
aborts guarded work on TERM; after five seconds the host may KILL only with
still-verified group ownership. A protected synced receipt requires kernel ECHILD for
the host's entire child set, not just leader exit. Escaped children, identity drift,
unavailable ownership and failed proof writes retain the flock and capacity; restoring
`owner.json` cannot authorize escalation; restarted hosts deny grants when retirement
intent exists. Runtime and worktree history are preserved.

## Read-only inspection and the derived reader

The `inspect-*.ts` entries are trusted read-only subprocesses with bounded input
(16 KiB JSON on stdin) and fixed-code failures. They never create a Harness, start a
scheduler, initialize or repair storage, submit input or take writable ownership; the
Rust caller bounds time and output and holds ownership around dependent operations.
`inspect.ts RUNTIME_DIRECTORY WORKTREE` is a local snapshot diagnostic (at most 100
committed entries); the operator transcript is `forge dispatcher assignment-transcript`.
Read-only SQLite can still create empty WAL/SHM coordination files.

The stock pi-durable SQLite opener always runs migrations and cannot open a store
physically read-only. `prepare-reader.mjs` derives an explicitly named sibling module
in the ignored dependency installation, never modifying the stock module. It checks
the full upstream source hash, replaces migration with exact schema-2 validation and
refuses commit/ID allocation; physical read-only SQLite enforces no writes. The
runtime checks the derived hash before loading it. The inherited read/decoding code
is pi-durable 1.5.2 from upstream tag `v1.5.2`, commit
`b4afdddf44e059c67ccf5097e5317d913521b470`, MIT; see `UPSTREAM-LICENSE`. Source:
<https://github.com/earendil-works/pi/tree/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable>.

- Upstream storage module SHA-256: `0d9d8cf56898afe5d38f0e56bd3cbcad23a8e68b942b81c39587ac731fe48642`.
- Derived module SHA-256: `b7801ce1290d4fc33d95848fbab5e8981fb2b47e9497650f5049c00c474c62aa`.

The generator is an explicit build step, not an npm lifecycle hook. Any dependency
upgrade must re-derive and review it and rerun `npm run test:upstream`. WAL with
`synchronous=NORMAL` gives process-crash persistence, not power-loss durability.

## Packaging

Bundle schema 2 contains the runner, its locked production dependency closure, the
derived reader, the pinned Node and `host/forge-runner-host`, and records the runner
component hash in `forge-runner-bundle.json`. The host answers `--compatibility` with
its runner protocol, runtime schema and Forge Revision; Forgeup and the daemon's launch
preflight check both. A retained Launch is served only by the installed bundle whose
component hash it records. See [matched runner packaging](../docs/development.md#matched-runner-packaging).

## Reproduce

Supported/tested host: Linux x64, local filesystem with `flock`, Node **24.18.0**,
npm **11.16.0** (SQLite **3.53.1**). From the Forge repository root, with a real Node
first on `PATH` (see [development verification](../docs/development.md#verification)):

```sh
cargo build -p forge-runner-host
cargo test -p forge-runner-host --test authority
cd runner
npm ci --ignore-scripts --no-audit --no-fund
npm run prepare-reader
npm run check
npm test
npm run test:upstream   # after any pi-durable/pi-ai/chord/Node change
cd ..
cargo test -p forge-runner-host --test authorization_session
```

Node's native TypeScript stripping runs the source; TypeScript and `@types/node` are
development-only. `cargo test --workspace` needs this Node/npm setup because the
`authorization_session` and CLI durable tests run the real pinned runtime. The
`authority` tests use the real Rust supervisor with an inert `/usr/bin/python3`
worker (standard library only).

Host crate dependencies: `fs2` (flock), `rustix` (no-follow flags, UID,
`CLOCK_BOOTTIME`), `serde`/`serde_json`, and `forge-rpc` for the shared strict
supervisor control contract. Test-only edges to the server/store crates and `rusqlite`
allow real commit/service/host integration; the host's production dependencies do not
include the server.
