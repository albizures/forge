# T08 — Interrupted-effect safety (in progress)

## Status

**T08 remains incomplete; T09 must not proceed.** This increment establishes the
runtime read-only inspection/admission fence, Forge-visible transactional effect
retention, and live-consumer failure fencing. It does not deliver the required
audited operator resolution or production observation/recovery controller.
Production durable dispatch and automatic recovery remain disabled. No live Forge
records, profiles, credentials or worktrees were changed.

## Initial implemented boundary

`runner/src/interrupted-effects.ts` uses the pinned physical read-only adapter to
inspect committed execution tasks before any writable Harness opening/recovery.
The pinned upstream `pi.tool` v3 implementation commits an `execute` checkpoint
with actual arguments and replay policy before entering the tool, then commits
its terminal result separately. Those existing protected records supply effect
intent/identity; this increment does not introduce a second uncoordinated ledger.

- The complete retained Launch must match. Interrupted initialization (no root)
  and incompatible/corrupt storage fail closed, rather than recreate/repair it.
- Paginated scans include all conversations in the one-Assignment store, not just
  its root. A maximum of 1,000 pages/1,000 blocker identities bounds inspection;
  exceeding a limit denies admission, never silently truncates to safety.
- A pre-execution `call` checkpoint has no execution intent and is not an effect
  blocker. An unsettled `execute` checkpoint blocks, even if labelled replay-safe.
  No automatic replay-safe operation is approved for the current coding tools.
- Completed tool results remain accepted runtime receipts. Failed, aborted,
  faulted and orphaned receipts conservatively block; interruption/cancellation
  errors cannot turn an external unknown into known absence. Unknown tool
  versions/checkpoints fail closed.
- Returned data contains only task, conversation and call identities. Arguments,
  outputs and results stay in protected runtime storage. No runtime repair, new
  input, bootstrap, model/tool call, Git mutation or Forge lifecycle change occurs
  during inspection.

`createCheckRuntime` now requires the canonical protected runtime directory and
checks the fence after exact lease admission but **before bootstrap**, profile
construction or caller Harness opening. Failure closes this local lease consumer
and emits only the existing fixed redacted runtime failure. The trusted caller
still owns host-exclusive locking, verified process identity and durable retention;
this read-only inspector is not ownership evidence or recovery authorization.
Both existing Check component/supervised tracer callers supply their actual bound
directory. The synthetic main remains unchanged and cannot resume execution.

Compatibility: no dependency, Forge schema, runtime schema or public CLI/RPC wire
change. The internal TypeScript helper gains a required argument; all callers are
updated. The component hash includes the new source and changed runtime helper,
so old component identities cannot silently reuse the changed implementation.

## Initial-increment verification

Commands from repository root:

```sh
(cd runner && npm run check && node --test test/interrupted-effects.test.mjs test/bootstrap.test.mjs)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

All final gates pass: **647 Rust harness tests passed, 0 failed, 1 pre-existing
opt-in probe ignored; all 6 parallel supervised-tracer scenarios passed; runner
typecheck and 48 Node tests passed**. `git diff --check` passes. Verification covers
the combined checkout, preserving the user's existing planning changes, not an
isolated commit. No clean-install, packaged-component or real-provider release
verification was run. The historical supervisor-test timeout did not recur here;
this is not a diagnosis or general flake-resolution claim.

### Actual process/effect evidence

Two isolated process scenarios in `runner/test/interrupted-effects.test.mjs` use a
real pinned Harness, the actual configured unsafe Bash tool and an independent
loopback HTTP publication-like mutation endpoint. Only the model and lease are
fixtures; these are **not** supervised Forge dispatch or real-provider evidence.
The worker uses a separate process group solely for test teardown, not product
retirement or proof against escaped same-user descendants.

1. The endpoint counts/accepts a POST effect and deliberately withholds its reply.
   The test synchronizes on that acceptance, then SIGKILLs the worker group before
   a tool result can commit. Inspection finds one unsettled effect on two separate
   reads, and the real Check construction boundary rejects before bootstrap and
   closes authorization on each attempted reopen. Mutation count remains one.
   The test never opens a second Harness or sends a retry to the endpoint.
2. When the endpoint returns normally, the tool result commits and inspection finds
   no interrupted effect. This establishes the receipt distinction, not permission
   to recover an Assignment without other policy gates.

Both scenarios compare tracked bytes, staged index contents, untracked bytes and
Git status before/after death and inspection, and verify unchanged runtime DB
bytes during inspection. Read-only SQLite coordination sidecars may change; this
is not a byte-immutability claim about SHM reader locks or a power-loss test.
Foreign Launch identity rejects. Worker output excludes the configured model-key
canary. No repository repair/adoption or hold clearance is used.

A separate committed-storage fixture covers more than one page, a nested call in
another conversation, an alleged replay-safe intent (still blocked), all four
non-completed terminal receipt classes, completed receipts, missing root, unknown
tool version and unknown checkpoint. These are storage-contract fixtures, not
additional process/effect evidence.

## Remaining T08 work / handoff

1. Wire the trusted observation/retention service into the future production
   supervisor/recovery controller. The bounded server-to-runner adapter described
   below now delivers actual observations, including lost admission replies and
   daemon restart, but is not production scheduling or recovery authorization.
2. Define/implement a bounded operator CLI/RPC inspection/resolution operation:
   scope authorization before replay; exact retries return original audit;
   conflicting reuse rejects; verified ownership/absence and current binding
   preconditions cannot race the resolution commit. Unsupported external outcomes
   remain unknown. Resolution records evidence, never reruns the command or
   authorizes dirty-commit adoption, and cannot clear unrelated holds.
3. Compose the fence and retained Forge blocker with that resolution policy before
   T10 same-Assignment recovery is possible. The live failed/cancelled invocation
   fence is now implemented and tested, but does not supply recovery authority.
4. Extend controlled publication-like effect tests to the eventual resolution
   path and unsafe-ownership/wrong-scope/conflicting resolutions. Post-tool-return/
   pre-result-commit death, daemon restart, observation scope/conflict denial,
   accepted-result preservation and capacity retention now have regression tests.
   Resolution audit/ownership still need implementation and verification.

There is currently **no operator resolution path**. Do not manually mutate runtime
SQLite or use existing unknown-provisioning resolution/retained-work salvage as an
interrupted-effect waiver. This partial increment does not establish exactly-once
execution, external reconciliation or universal process death.

## Initial-increment task-local reviews

Implementation review against T08: “Translate runtime interruption into
Forge-visible uncertainty” and “Establish a bounded, explicit operator
inspection/resolution path” remain missing; the runtime gate alone does not meet
these acceptance checks. “On reopen, neither tool replay nor a fresh model request
bypasses the blocker” is implemented at Check construction but not yet a full
recovery controller or live-runtime failure policy. Resolution rejection/audit and
daemon-restart composition remain unverified. No scope creep or completion claim.

Standards self-review: no introduced violations found. The new capability is a
read-only runner adapter over pinned runtime machinery, not duplicated Forge
lifecycle/recovery policy; no external calls occur inside Forge transactions.
Incompatible records and unknown external effects fail closed, no destructive
worktree operations occur, and process tests isolate roots/listeners/credentials
with explicit effect synchronization. T18 independent review remains required.

Initial-increment summary: runtime interruption inspection/fencing delivered;
Forge-visible retention and operator resolution were missing at that snapshot.

## Forge-retention and live-fencing increment

`crates/app/src/durable_execution/effects.rs` defines a trusted host observation
port and bounded task/conversation/call identities. No commands, arguments, tool
outputs or external-outcome assertions cross that port. The store implementation
rechecks Workspace and the complete retained Launch before any replay, including
observations arriving after closure/invalidation or with a lost admission reply.
An immediate transaction atomically retains `interrupted-effect` resources and a
Dispatch Hold. It preserves another hold and all accepted Outcome bytes. Exact
retries preserve timestamps, conflicting task identity reuse rejects, and a batch
or accumulated runtime limit over 1,000 rejects atomically. Empty/later scans never
clear previous uncertainty. Existing resource capacity and lease gates retain the
blocker even after explicit hold clearance. Application policy and the store reject
generic resource retirement of this kind; cleanup cannot resolve external effects.

This is additive use of schema 21's opaque resource representation, not new DDL,
a schema conversion, a public mutation RPC or an agent-authorized operation.
Existing CLI/RPC Assignment inspection exposes the resource and new explicit
`interrupted-effect` blocker without changing response fields. No dependency or
version change. Production durable launch/automatic recovery remain disabled.

Actual coding tools now use `guardEffectTool`. A thrown/cancelled invocation closes
that consumer before another provider or tool request; the fixed error excludes
upstream details. Synchronous lease denial remains the existing contract. Returned
results (including a known shell exit code) remain receipts; this wrapper does not
claim to commit them. The pinned Harness owns result persistence; reopening still
requires committed read-only inspection. Two real-Harness/fake-provider tests cover
ordinary exception and cancellation-shaped exception, fresh input after failure,
one model request/one attempted effect, retained failed receipts and redaction.
These are local failure simulations, not universal child-exit evidence.

### Composed process and daemon evidence

Two new default-parallel Rust CLI integration cases use actual application
Check dispatch/Managed Worktree, authenticated bootstrap, a real pinned Harness,
actual configured Bash, independent loopback HTTP publication-like endpoint and
read-only runtime inspector. Model and lease are fixtures; this is not the shared-
flock supervisor or real-provider verification. Each endpoint receives one mutation:

- Kill the worker group after the endpoint accepts the effect, withholding its reply.
- Return the endpoint reply and pause the actual storage adapter immediately before
  the completed tool receipt commits; kill the worker group at that explicit marker.
  This is a controlled persistence-boundary proxy, not a physical SQLite/power fault.

Both deliver the actual inspected identities to Forge while its admission result
is still Unknown (lost-reply analogue). Retention is retried after daemon abort/
restart and read through actual human/JSON CLI/RPC inspection. No second Harness
opens, no replay is submitted, and each mutation count stays one. Tracked bytes,
staged index bytes, untracked bytes and Git status remain unchanged; inspector DB
bytes also remain unchanged. Separate process-group teardown is test isolation,
not product retirement or proof against escaped same-user descendants.

Eight store tests cover transactional hold/resource rollback, concurrent independent
connections, exact retries, wrong scope/Launch, conflicting identities, invalid/
unbounded inputs, cumulative limits, closure/restart, empty observations, accepted
Outcome preservation, unrelated holds, generic-retirement denial, capacity and
renewal rejection even after explicit hold clearance. Accepted-Outcome preservation
is a store fixture, not an additional authenticated Outcome path.

### Final verification for this increment

From the repository root, all passed:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final results: **657 Rust harness tests passed, 0 failed, 1 pre-existing opt-in
probe ignored; all 6 custom-harness supervised scenarios passed; runner typecheck
and 50 Node tests passed**. Two added fault-boundary CLI cases also passed
individually before the full default-parallel run. The first Node run found a
synchronous lease-denial regression in the new async wrapper; it was corrected
and the complete suite rerun successfully. No clean-install, packaged-component,
real-provider or release readiness gate was run. No live Forge state was changed.

### Task-local reviews on the increment

Implementation review (T08): “Translate runtime interruption into Forge-visible
uncertainty” now has a transactional boundary and controlled actual-observation
composition. “Establish a bounded, explicit operator inspection/resolution path”
remains partial: inspection exists, but audited evidence-based resolution and its
scope/conflict/unsafe-ownership rejection tests are absent. “On reopen, neither
tool replay nor a fresh model request bypasses the blocker” has local reopening
and live-failure fences; full production supervisor/recovery composition is not
implemented. No unrelated scope expansion found.

Summary: **T08 incomplete; operator resolution and production observation/recovery
composition remain the worst gaps; T09 remains blocked.**

Standards self-review: no introduced violations found. Domain/retirement policy is
in app, transactional retention in store, and CLI integration exercises existing
wire inspection; external process/filesystem calls remain outside transactions.
No implicit outcome resolution, capacity release, secret-bearing payloads or
worktree repair was added. T18 independent review remains required.

Summary: no introduced coding-standard violations found; remaining implementation
gaps are explicitly retained, not waived by the passing local gates.

## Bounded server observation increment

`crates/server/src/durable_authorization/effects.rs::inspect_and_retain` now
composes actual committed runtime inspection with transactional Forge retention.
The two real publication-like fault scenarios use this service rather than
submitting fixture-returned identities directly to the store. It is a callable
trusted daemon adapter, **not yet an automatically scheduled production recovery
controller or an operator resolution command**. T08 remains incomplete and T09
remains blocked.

- Scope and the complete retained Launch are checked before process spawning.
  Storage comes from the existing protected canonical Workspace/store mapping;
  no RPC-selected path, current profile source, model credential, capability or
  worktree access is needed for this historical inspection.
- Explicit trusted absolute Node/inspector paths replace PATH discovery. The
  inspector checks the retained component hash, runtime schema and complete
  journal identity. The process has a cleared environment, protected identities
  on stdin, a 16 KiB request cap, a 20 MiB response cap and a 30-second deadline.
  Failed/timed-out/oversized inspectors receive a kill/reap attempt bounded by
  one additional second; an unverified exit stays unknown (including kernel I/O
  stalls). Stderr and arbitrary parser/storage errors are not forwarded. This is
  only direct read-only child cleanup, not Assignment retirement or proof about
  surviving external work.
- `runner/src/inspect-effects.ts` is a separate read-only entrypoint. Missing
  runtime storage and interrupted initialization fail closed, not a successful
  empty scan. It uses the pinned physical read-only adapter, never a Harness.
  Only protocol version, exact Launch and bounded effect identities are returned.
- Response validation denies foreign bindings, unknown fields/protocol versions,
  unbounded batches and invalid identities. Retention then rechecks scope and
  binding transactionally. It remains monotonic: no response, failure or empty
  scan clears a resource/hold, rewrites an Outcome or authorizes execution.
- Read-only scans can race an active writer; they are not an exclusive recovery
  snapshot. Recovery must still obtain verified host ownership and inspect again
  before any writable opening/model/tool admission. No scheduler is enabled here.

Compatibility: additive internal protocol-1 observation entrypoint; no public
CLI/RPC fields, dependency versions or storage schemas changed. Tokio's existing
`process` feature is enabled to bound/reap the inspector without blocking an async
executor; no new dependency is added. Shared Launch validation is extracted
without changing admission rules. Storage ownership checks are shared between
historical inspection and new-launch validation; historical inspection does not
require the original worktree to exist. The component hash includes the new
entrypoint. Old hashes are never rewritten: retained runtimes still require
matching components; this increment does not supply T15 packaging/selection.

### Verification

From the repository root, all final gates passed:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Results: **659 Rust harness tests passed, 0 failed, 1 pre-existing opt-in probe
ignored; all 6 custom-harness supervised scenarios passed; runner typecheck and
51 Node tests passed**. The two fault scenarios retain actual inspected effects
through the service, repeat observation after daemon abort/restart and verify
unchanged runtime DB bytes, retained Assignment state/capacity and actual CLI/RPC
inspection. Mutation count remains one; tracked, staged and untracked bytes stay
unchanged. Tests also reject wrong scope/binding before a missing executable,
and preserve the prior blocker when the inspector executable is unavailable.

New adapter tests cover malformed/extra fields, all Launch identity mismatches,
version/batch/identity bounds, deadline expiry, excessive output and redacted
nonzero exits. A separate Node wire test covers absent storage, interrupted
initialization, mismatched hash/journal, oversized/malformed requests, exact
repeated committed inspection, argument-canary redaction and unrepaired corrupt
storage. Shell scripts used only as broken-inspector fixtures use builtins and
spawn no descendants. These tests are not real-provider or packaged release
verification. The historical supervisor timeout did not recur; no general flake
resolution is claimed.

During implementation, typechecking caught a narrowed-key indexing error and
Rust compilation/lint caught test error-field access and let-else style; all were
corrected before the final gates. The first composed run correctly rejected a
group-writable test `.forge` ancestor inherited from host umask 0002. The fixture
now creates its owner-only `.forge` boundary before worktree setup; production
path validation was not weakened. An initial name-filtered Rust command selected
zero tests; the entire five-case integration target and full workspace were then
run successfully.

### Task-local reviews

Implementation review against T08: “Translate runtime interruption into
Forge-visible uncertainty before recovery can schedule more model/tools” now has
a bounded reusable server observation/retention adapter, but production recovery
controller integration remains absent. “Establish a bounded, explicit operator
inspection/resolution path” remains partial: no audited resolution command or
resolution scope/conflict/unsafe-ownership acceptance tests exist. Successful
inspection is not execution authority. No unrelated scope expansion found.

Summary: T08 incomplete; audited operator resolution and production recovery
composition remain the blocking gaps; T09 must not advance.

Standards self-review: no introduced violations found. Policy/transactional
retention remain in app/store; server owns process adaptation and canonical
mapping. External calls finish before the store's atomic scope/binding recheck.
Failures retain uncertainty/capacity, stderr is discarded and wire payloads
exclude tool arguments/results. Tests isolate roots/processes and the final
workspace suite ran in default parallel mode. This is a task-local self-review,
not the independent T18 review.

Summary: no introduced coding-standard violations found; remaining T08 work and
unrun release gates are not waived.

## Exclusive inspection increment

`crates/runner-host/src/inspection.rs::ExclusiveInspection` now provides an
explicit host-exclusive inspection guard. It acquires the **existing** protected
`owner.lock` with the same nonblocking kernel flock used by the supervisor and
its worker. Missing lock evidence rejects without creating anything. The lock
opens read-only with no-follow/nonblocking/close-on-exec flags; owner-only
canonical directory, regular-file ownership, exact permissions and single-link
checks fail closed. Directory and lock inode/device identity are revalidated.
The guard retains its original descriptor even if path validation fails; dropping
it releases only that descriptor's ownership, not Forge resources or capacity.

`crates/server/src/durable_authorization/effects.rs::inspect_exclusive_and_retain`
checks canonical Workspace/runtime binding before acquiring the guard, then runs
the existing bounded committed inspector. It revalidates host path identity before
transactional Forge retention and again before returning the still-held guard.
Callers can keep competing supervisors fenced throughout dependent investigation;
errors release the inspection guard without clearing retained blockers. Ordinary
`inspect_and_retain` remains non-exclusive for read-only investigation alongside
an active writer. No writable Harness, model/tool input, signal, repository
mutation, lease grant or resource retirement is added.

This is a reusable inspection boundary, **not production controller integration,
a resolution waiver, or recovery permission**. Acquiring the flock does not prove
that escaped external children stopped or that an external outcome is known.
Trusted same-user filesystem writers must still be excluded; inode observations
are not a sandbox or a filesystem/database distributed transaction. Production
scheduling remains disabled. No schema, dependency, CLI/RPC wire or runtime
component hash changes are required: the runner sources are unchanged.

### Verification

All passed from the repository root:

```sh
cargo test -p forge-runner-host
cargo test -p forge-cli --test durable_check_bootstrap
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final results: **663 Rust harness tests passed, 0 failed, 1 pre-existing opt-in
probe ignored; all 6 custom-harness supervised scenarios passed; runner typecheck
and 51 Node tests passed**. Four new guard unit tests cover missing evidence,
competing descriptors, unchanged lock bytes, release-on-drop, permissions,
symlinks/hardlinks, replaced locks and replaced directories. Existing actual-host
process tests now also reject inspection while the supervisor is live, after its
worker exits but the supervisor retains the lock, and after supervisor death
while a surviving worker retains the inherited lock. Once the controlled worker
has exited and its supervisor is killed/reaped, inspection acquires the lock,
retains original owner evidence, and fences another descriptor until dropped.
These are isolated process/lock tests, not universal process-death evidence.

Both actual publication-like mutation/receipt fault cases now exercise exclusive
observation and retention, including after daemon restart. They verify unchanged
runtime DB bytes, unchanged Forge Assignment/resource state on exact retry,
retained capacity/blockers and guard release after inspector failure. Their
component worker is **not** supervised: an explicitly labelled protected lock
fixture composes this adapter, after first asserting missing evidence rejects
without creating a lock. Actual supervisor/worker lock behavior is tested
separately above. Existing mutation counts remain one and tracked/staged/untracked
work is preserved. This does not establish successful production recovery or
operator resolution. No packaged or real-provider release checks were run.

### Task-local reviews

Implementation review against T08: “Establish a bounded, explicit operator
inspection/resolution path” is still partial: the exclusive inspection prerequisite
exists, but audited resolution and wrong-scope/conflicting/unsafe-ownership
resolution tests remain missing. “Translate runtime interruption into Forge-visible
uncertainty before recovery can schedule more model/tools” now has an exclusive
observation/retention adapter, but no production recovery controller invokes it.
No unrelated scope expansion or new execution permission was found.

Summary: **T08 remains incomplete; audited operator resolution and production
recovery integration remain blockers. T09 must not proceed.**

Standards review: no introduced violations found. The host owns the process/lock
boundary, server owns canonical mapping and bounded process adaptation, and app/
store retain the existing domain policy/transaction. External inspection remains
outside the database transaction. Failure neither releases retained capacity nor
clears holds, and no credentials or tool payloads are added to output. Tests use
isolated paths and actual process synchronization; workspace verification ran in
default parallel mode. This is a task-local self-review, not independent T18.

Summary: no introduced coding-standard violations found; missing T08 deliverables
and unrun release checks remain explicitly blocking.
