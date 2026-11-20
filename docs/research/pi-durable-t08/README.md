# T08 — Interrupted-effect safety (in progress)

## Status

**T08 remains incomplete; T09 must not proceed.** This increment establishes the
runtime read-only inspection/admission fence, Forge-visible transactional effect
retention, and live-consumer failure fencing. The latest increment adds the public
operator resolution CLI/RPC and explicit matched inspection-component wiring for
revoked Assignments. Live/same-Assignment resolution policy and production recovery
integration remain undelivered; original runtime intent still blocks writable recovery.
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

## Audited resolution backend increment

`crates/app/src/durable_execution/effect_resolution.rs` now defines a bounded,
trusted-operator evidence assertion and original acceptance receipt. Supported
external conclusions are `applied` and `not-applied`; there is no unknown-to-safe
default. Both old-agent-cannot-act and external-work-safe confirmations, bounded
attribution, evidence reference and explanation are mandatory. These are explicit
operator assertions, not authenticated human identity, external verification or
proof against escaped same-user processes. Evidence fields are ordinary operational
output: do not copy credentials, prompts or tool arguments/results into them.

`crates/store/src/durable_execution/effect_resolution.rs` authorizes Workspace and
complete immutable Launch before replay. Its immediate transaction requires a
closed/revoked Assignment for new acceptance, checks the exact retained effect,
and atomically retires only that effect and retains the original audit/timestamp.
Exact retries return the original receipt across restart. Reusing an operation
with different evidence or targeting an already resolved effect with another
operation rejects; concurrent conflicting conclusions accept only one. Accepted
Outcomes, runtime receipts, worktree evidence, other resources and Dispatch Holds
are untouched. Unknown external work cannot be resolved by this API.

This is additive use of schema 21's opaque resource contract:
`interrupted-effect-resolution` has canonical serialized assertion JSON as its ID
and equal creation/retirement timestamps. The already-retired audit does not itself
reserve capacity. At most 1,000 audits are accepted per runtime. Original effect
identities remain retained with their resolution timestamp; later observations do
not reactivate them or replace their audit. Generic insertion and retirement now
reject both protected effect kinds, preventing bypass through those generic ports.
No schema, dependency, public wire fields or runner component identity changed.

`crates/server/src/durable_authorization/effects.rs::resolve_exclusive` validates
assertions, maps canonical storage, acquires the existing shared ownership flock,
performs fresh bounded committed inspection/retention, and retains/revalidates the
same guard through the resolution commit. A competing owner, missing/unsafe lock,
failed inspection or foreign binding rejects. A post-commit path error can leave an
accepted audit; exact operation replay discovers it without overwriting evidence.
The host lock fences supervisors, not arbitrary escaped external children; those
must separately be made safe, as explicitly asserted. Nothing opens writable runtime
storage, issues a grant, repairs a receipt, reruns a command or adopts dirty work.

### Actual verification

From the repository root, all final gates passed:

```sh
cargo test -p forge-store effect_resolution
cargo test -p forge-cli --test durable_check_bootstrap
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final full-suite results: **671 Rust harness tests passed, 0 failed, 1 pre-existing
opt-in probe ignored; all 6 custom-harness supervised scenarios passed; runner
typecheck and 51 Node tests passed**. The initial focused store run had six tests;
two more were subsequently added and all eight pass in the full default-parallel
suite. Eight store regressions cover atomic audit/resource rollback, exact restart
replay, every Launch identity and wrong Workspace, live authority, both missing
confirmations, invalid/unknown conclusions, conflicting operations, concurrent exact
and conflicting claims, accepted-result preservation, other effects/holds/capacity,
and generic-port forgery/retirement denial. Rollback/accepted-result cases are
store fixtures, not additional authenticated Outcome paths.

Both existing actual Bash/publication-like fault cases now compose the resolution
service after daemon restart: one dies after external acceptance and one after tool
return before completed-result commit. The controlled endpoint reports one mutation;
the worker group is killed/reaped and the endpoint stopped by test teardown before
resolution. These tests use an explicitly labelled protected ownership-lock fixture,
**not product supervised retirement or universal process-death evidence**. They
reject competing ownership, live authority, wrong scope/binding and unconfirmed
resolution; accept and replay the exact assertion; reject a conflicting conclusion;
and inspect the original audit through actual human/JSON CLI/RPC Assignment info.
Runtime DB bytes, Git status, index, tracked and untracked bytes remain unchanged.
A final actual committed inspector still reports the unsettled runtime intent:
resolution deliberately manufactures no runtime result or replay authority. Original
model and authorization fixtures remain fixtures, not real-provider evidence.

During implementation, compilation caught SQLite signed/unsigned timestamp reads;
the first focused test correctly rejected an accepted-result fixture missing its
closed-attempt marker. Clippy caught two test idioms and an overlong composed-test
helper; these were corrected before the successful full gates. No production path
validation was relaxed. No clean-install, packaged-component, real-provider or
release verification was run; no live Forge state changed.

### Backend-increment remaining work and task-local reviews

T08's “Establish a bounded, explicit operator inspection/resolution path” remains
**partial**: the internal audited backend exists, but a public operator CLI/RPC with
trusted matched-component wiring and command-level denial/retry tests is missing.
The currently exposed CLI/RPC only reads the accepted audit via Assignment info.
The initial backend deliberately requires revoked/closed authority; live/same-attempt
resolution and the policy for consuming external audit alongside still-unsettled
runtime intent are not implemented. Do not equate a retired Forge effect resource
with safe writable runtime recovery. The existing runner fence still denies it.

“Translate runtime interruption into Forge-visible uncertainty before recovery can
schedule more model/tools” has inspection, transactional retention and failure fences,
but still lacks production recovery-controller integration. That controller must
compose current authorization, evidence/ownership and effect policy before writable
opening; no existing gate is waived here. T09 remains blocked, and durable production
dispatch/automatic recovery remain disabled. No unrelated scope expansion found.

Implementation-review summary: T08 incomplete; public resolution controls and
production recovery composition remain the blocking gaps.

Standards review: no introduced violations found. App owns assertion types/policy,
store owns atomic scope/replay/audit and resource changes, server owns canonical
mapping and external read-only inspection under host ownership. External calls stay
outside transactions; failures preserve unrelated safety state and no runtime or
repository repair occurs. Tests isolate roots/processes and the complete workspace
ran in default parallel mode. This task-local self-review does not replace T18.

Standards-review summary: no introduced coding-standard violations found; missing
T08 deliverables and unrun release gates remain explicit blockers.

## Public operator resolution increment

`forge dispatcher assignment-resolve-effect` and the additive
`agentDispatch_assignmentResolveEffect` RPC now expose the conservative revoked-
Assignment resolution boundary. Explicit Workspace/canonical client-cwd selection
uses the existing Record selection contract; a bare Assignment ID does not select
scope. The daemon derives the immutable Launch from the owning stored runtime,
not an RPC-selected profile, component or filesystem path. Strict wire decoding
rejects extra credential/path/Launch fields and unsupported conclusions. The
Assignment capability API remains unchanged. Operator RPC is still trusted and
unauthenticated; Workspace routing and rejecting credential-shaped fields do not
authenticate a human or sandbox a same-user agent.

New acceptance requires the same bounded assertion, both safety confirmations,
closed authority, exact retained effect, fresh committed scan and existing protected
exclusive host lock. The CLI exposes explicit effect identity, stable operation,
conclusion, attribution and evidence flags. Missing confirmations reject locally
before endpoint resolution and independently on the server. Errors keep stdout
empty in JSON mode; accepted JSON is the typed original receipt. Human output
explicitly disclaims replay/recovery. Evidence fields are ordinary operational
output, not protected transcript storage. No agent secret is read or forwarded.

`durable_authorization::operator::InspectionComponents` selects both absolute
trusted Node and inspector paths or neither. Manual daemons read the explicit
`FORGE_DURABLE_NODE` and `FORGE_DURABLE_EFFECT_INSPECTOR` host settings. Partial or
relative selection rejects; there is no PATH/npm/current-checkout fallback. The
inspector still verifies the retained component hash/schema/Launch. Managed
launches do not inherit these settings; matched Revision packaging and selection
remain T15 work. This configuration supplies no model key or Assignment authority
and enables no dispatch/recovery. Missing components deny new acceptance with a
fixed diagnostic. An extra scoped repository connection supplies operator
inspection/resolution without altering the application's provider factory.

A new scoped accepted-result read separates **discovering an original audit** from
**accepting a new safety assertion**. Scope and the complete retained Launch are
checked in a consistent read transaction before returning the receipt. Conflicting
operation/effect reuse rejects. Exact accepted-result retries require neither host
ownership nor runtime storage/components: they perform no inspection, retirement,
authority renewal or safety acceptance. This fixes the earlier backend's overly
strict dependency on current inspection availability for lost-reply replay. The
mutation transaction independently repeats scope/replay checks before any new
acceptance. No later runtime state can rewrite an accepted audit.

Compatibility: new CLI command and operator RPC method, plus an internal optional
DaemonConfig component field and explicit manual environment configuration. No
existing arguments, response fields, management protocol, schema, dependencies,
runner code/component hash or accepted resource representation changed. Existing
DaemonConfig constructors/fixtures explicitly select no components; managed
configuration remains independent of ambient settings. Help and operational/
store documentation describe the command and controlled limits.

### Public-boundary verification

Both real Bash/publication-like fault cases now accept the resolution through the
**actual CLI → HTTP daemon → canonical service → committed inspector → store**
path, after daemon restart. Direct backend checks remain as separate boundary
assertions. New public cases cover missing confirmations before an unreachable
endpoint, independent server confirmation rejection, missing/wrong Workspace,
closed-authority requirements, competing lock ownership, unknown conclusions,
credential/path/Launch-shaped RPC extras, conflicting conclusion and exact retry
in human/JSON modes. Ambient Assignment credential canaries are not consumed or
returned. Both existing human/JSON Assignment-info reads retain the original audit.

After acceptance, exact RPC/CLI retries run while another inspection guard owns the
host lock. Then the daemon is stopped/restarted **without inspector configuration**
and the runtime directory is temporarily renamed away from its canonical location.
The original receipt/timestamp remains discoverable without creating storage,
changing resources/holds or taking the host lock. A new operation still rejects as
unconfigured. The original runtime directory is restored afterward: this tests
unavailability, not transcript deletion or managed packaging. Original-reply replay
is a lost-reply analogue, not an injected dropped HTTP response.

Worktree/index/tracked/untracked and runtime DB bytes remain unchanged, and actual
committed inspection still reports the unsettled intent. The endpoint's actual
mutation count remains one before controlled endpoint teardown; resolution has no
command/runtime write path. Closure uses the existing repository port as a **fixture**,
not product supervised retirement or proof against escaped external children.
The ownership lock is still a labelled fixture; real supervisor/worker fencing is
separately covered by the pre-existing host tests. Model/lease fixtures remain
fixtures; this is not real-provider verification.

Five new standalone regressions cover strict RPC encoding, complete/absolute host
selection, a scoped accepted-result read and CLI parser required identity/evidence/
conclusion/credential/path contracts. The expanded composed cases run in the default
parallel CLI target. Commands from repository root:

```sh
cargo test -p forge-cli --test durable_check_bootstrap
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final corrected-snapshot results: **676 Rust harness tests passed, 0 failed,
1 pre-existing opt-in probe ignored; all 6 custom-harness supervised scenarios
passed; runner typecheck and 51 Node tests passed**. `git diff --check` passes.
The server binary was also rebuilt with `cargo build -p forge-server --bin
forge-server`, then its `--help` and the new CLI command's `--help` were inspected.
The initial compile caught that CLI depends on RPC, not app; shared resolution types
are now deliberately re-exported by RPC rather than adding a dependency. Clippy
caught inline test-module placement; tests were moved to a capability-local file.
No clean-install, packaged-component, real-provider or release gate was run. No
live Forge records, profiles, credentials or worktrees were changed.

### Current task-local review and blockers

Implementation review: T08's “Establish a bounded, explicit operator inspection/
resolution path preserving accepted results, ownership guards and audit” now has
public commands and controlled CLI/RPC/process evidence for revoked Assignments.
“On reopen, neither tool replay nor a fresh model request bypasses the blocker”
remains fail-closed: public resolution leaves the runtime intent unsettled and the
existing runner fence still rejects writable recovery. However, live/same-Assignment
resolution policy and production controller consumption of effect audit alongside
runtime uncertainty remain missing. The public command is not a waiver for those
gates. No unrelated scope expansion found.

Implementation-review summary: **T08 incomplete; live/same-Assignment effect policy
and production recovery composition remain blocking. T09 must not proceed.**

Standards review: no introduced violations found. App owns resolution alternatives
and replay policy; store owns scope, consistent read and atomic mutation/audit;
RPC owns strict operator contracts; server owns selection, canonical mapping and
matched process adaptation; CLI owns flags/presentation. External inspection stays
outside database transactions, with binding rechecked at commit and host ownership
held for new acceptance. Exact replay reads admit nothing and preserve original
evidence. Denied actions preserve safety state, tests isolate processes/roots and
full workspace verification runs in default parallel mode. Independent T18 remains
required.

Standards-review summary: no introduced coding-standard violations found; remaining
T08 deliverables and unrun release gates are not waived.
