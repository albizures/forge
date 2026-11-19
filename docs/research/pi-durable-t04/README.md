# T04 — single-owner persistent runner evidence

## Disposition

Implemented and locally verified for the T04 controlled-model boundary. T05 is
next. Production durable dispatch remains disabled; no live Assignments, Forge
Records, profiles, capabilities, provider requests, Outcomes or rollout changed.
This is not authorized writable recovery, packaged installation or release
readiness.

## Delivered implementation

- `crates/runner-host`: independent Linux Rust host acquires exclusive flock
  before spawning TypeScript or opening runtime storage. A persistent no-follow,
  owner-only lock inode is shared with the worker using the same inherited open
  description. Either survivor fences a contender. Worker exit never releases
  the host's ownership or implies cleanup. Host and worker boot/start/group/lock
  identities are recorded and checked; stale PID text is never adopted/signalled.
- `runner/src/main.ts`: one synthetic provider, no tools, explicit empty auth/env,
  one root/conversation and one SQLite store per retained Launch. Version/schema/
  component negotiation, strict fields and operation correlation precede writable
  initialization. Exact admission retry reads the committed submission ID;
  changed payload/operation identities reject before runtime admission.
- `runner/src/storage.ts`: independently versioned intent/root journal in the
  runtime database; protected canonical storage outside the worktree; atomic
  exclusive database file creation. Interrupted initialization/root/admission
  remains unknown. Existing stores are never opened writable by ordinary retries.
- Explicit maintained derived reader, pinned upstream source hash and runtime
  derived hash, exact schema validation and physical read-only SQLite. No stock
  module mutation, migration, scheduler creation, task repair or input admission
  during observation. `inspect.ts` is a local bounded committed snapshot, not
  T11's operator CLI/RPC or pagination implementation.
- `runner/test/wire-v1.json` is checked against Rust's typed v1 request and the
  actual TypeScript decoder. Runner/package/provenance/reproduction contracts are
  in `runner/README.md`, including the upstream MIT license and upgrade obligations.

No Forge store schema/wire version changes: Forge revision 20, runtime schema 2,
runner protocol 1. The runtime-local Forge identity table is version 1. New Rust
crate dependencies already exist in the workspace closure: fs2 (host lock),
rustix (safe no-follow/inheritance/UID primitives), serde_json (identity evidence).
Pinned JS dependencies provide durable storage, synthetic models and execution
context. TypeScript/Node declarations are development-only strict-check tooling.

## Retained deterministic evidence

`runner/test/process.test.mjs`: **10 passed, 0 failed**, using isolated directories,
explicit readiness/settlement/exit/diagnostic channels and no credentials or sleeps:

1. Shared Rust/TypeScript wire fixture, strict fields and protocol/schema/component
   incompatibility diagnostics.
2. Real concurrent starts: exactly one writable host; same Launch retry and observe
   return the same submission. A reader beside the writer sees synthetic input/
   completion, cannot mint IDs and leaves database bytes unchanged. All runtime,
   WAL/SHM, ownership files have mode 0600.
3. Unknown protocol creates no database; conflicting launch payload, wrong runtime
   binding and unimplemented recovery fail explicitly.
4. SIGKILL worker: supervisor retains ownership, contender rejects, stopped-worker
   inspection preserves identity/content. After the test controller stops that
   supervisor, a new host reuses the lock inode and only observes the original
   accepted result, without writable recovery or another submission.
5. Interruption after ownership readiness but before admission leaves no SQLite
   store; stale evidence naming an unrelated process does not adopt or signal it.
6. Two Assignments have distinct processes/stores. Killing the first worker does
   not interrupt the second's synthetic model execution and committed completion.
7. Symlink lock, unsafe directory mode, worktree overlap and a dangling database
   symlink reject without touching or creating the external target.
8. Missing/old/new read schemas reject without initialization or byte changes.
9. SIGKILL supervisor while its worker survives: inherited flock still rejects a
   contender and read-only inspection works. Worker exit and host exit are tested
   independently; neither alone frees ownership while its counterpart survives.
10. Intent committed without a root/submission stays unknown on retry; read-only
    inspection does not initialize or repair the incomplete runtime schema.

The process suite initially exposed Linux socket-to-pipe `io::copy`/splice holding
its destination pipe while waiting for more input, blocking worker readiness.
Explicit bounded-buffer forwarding fixes this; Node-spawned socket stdin is
exercised throughout the suite. Initial TypeScript/clippy failures were corrected
before the final gates. No remaining self-review findings against T04 or
`CODING_STANDARDS.md` were identified in this bounded deliverable; independent T18
completed-system review is still required.

## Exact identities and commands

Node **24.18.0**, npm **11.16.0**, Linux x64, SQLite **3.53.1**.
`@earendil-works/pi-durable`, `pi-ai`, `chord`: exactly **1.5.2**, locked dependency
closure in `runner/package-lock.json`. TypeScript **5.9.3**, Node declarations
**24.10.1**. The component digest covers current source/lock/reader derivation;
matched installed Revision attestation remains T15, not this local digest.
Upstream/derived source hashes are retained in `runner/README.md`.

From the Forge repository root:

```sh
cargo build -p forge-runner-host
(cd runner && npm ci --ignore-scripts --no-audit --no-fund && \
  npm run prepare-reader && npm run check && npm test)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

All pass: **578 Rust tests passed, 0 failed, 1 opt-in probe ignored**; **10 runner
checks passed**. The ignored Rust probe is not claimed as coverage. Clean npm
installation emits an existing transitive `node-domexception` deprecation warning,
not an installation/test failure. No shipped skills changed.

## Successor obligations and limits

T05 must implement Forge authorization, bounded leases and independent expiry
watchdog/cancellation before real effects. T06/T07 provide explicit profiles/auth,
canonical T03-store-to-directory mapping, protected bootstrap and production
transport integration. Ordinary retries here deliberately do **not** resume
interrupted execution; T08/T10 must validate effects and authorize same-attempt
writable recovery before even opening a Harness. T09 supplies retirement and
verified cleanup; T11 full operator inspection/pagination; T15 installed component
attestation. No current server path launches this host or runner.

The host intentionally parks with its lock after worker exit; there is no new
operator adoption/termination/retirement API. Test controllers kill only their
own directly spawned, identity-checked fixture processes. Lock freedom after both
processes die is not proof of clean external resources: T03's retained capacity
and uncertainty still gate replacement. Observation never clears those blockers.

Tests use synthetic model completion, not real-provider authentication or external
effects. They establish local process crash/ownership and committed observation,
not power-loss durability, hostile-agent isolation, escaped-child cleanup,
exactly-once effects, a complete Implementation-to-Review cycle or release
readiness. SQLite WAL/NORMAL guarantees and trusted-local-host limitations remain
as recorded by T01. Required T19 packaged/real-provider verification is unrun.
