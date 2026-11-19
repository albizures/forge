# Forge durable runner — T04 controlled execution boundary

This is a **synthetic-only development component**, not a production dispatch
backend or installed Forge Revision. No Pi CLI, Herdr, credentials, ambient Pi
configuration, tools, network provider, Assignment capability or accepted Outcome
is used. The daemon does not launch this component. T05 adds authorization/leases,
T06 profile/authentication mapping, T07 actual Check-bound dispatch, and T15 matched
installation. Do not use this harness to execute live Assignments.

## Reproduce

Supported/tested host: Linux x64, local filesystem with `flock`, Node **24.18.0**,
npm **11.16.0** (SQLite **3.53.1**). Other platforms are not supported yet.
From the Forge repository root:

```sh
cargo build -p forge-runner-host
cd runner
npm ci --ignore-scripts --no-audit --no-fund
npm run prepare-reader
npm run check
npm test
```

Node's native TypeScript stripping runs the source; TypeScript and `@types/node`
are development-only dependencies for strict type checking. The runtime's three
exact 1.5.2 dependencies supply persistence, the synthetic provider and explicit
execution context; their transitive closure/integrities are locked. No new Rust
library is invented: `fs2` provides the existing workspace flock mechanism,
`rustix` supplies safe no-follow descriptor flags/process UID (already in the
workspace dependency closure), and `serde_json` encodes protected identity facts.

## Boundary and local protocol

`forge-runner-host RUNTIME_DIRECTORY WORKTREE ABSOLUTE_NODE ABSOLUTE_RUNNER`
requires an existing canonical owner-owned 0700 runtime directory separate from
the canonical worktree, and explicit absolute component paths. It acquires a
nonblocking exclusive flock on a persistent 0600 no-follow single-link lock inode
**before spawning the worker or opening SQLite**. It never unlinks a stale lock.
The caller must map the T03-reserved store identity to one canonical directory;
this development command does not look up Forge's store or authorize Assignments.

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
not accepted Outcomes or Forge lifecycle facts. Errors carry only stable codes,
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
