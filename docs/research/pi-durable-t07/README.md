# T07 — Check-bound execution tracer

## Status: T07 complete; T08 may proceed

The explicitly injected controlled adapter now composes application Check dispatch,
real Git Managed Worktree preparation, server-owned protected capability delivery,
retained runtime admission, canonical protected runtime mapping, the shared-flock
Linux supervisor, actual daemon authorization service/driver, pinned Harness,
guarded fake model, actual Bash tool and authenticated Forge CLI/server Outcomes.
Committed read-only answer observation feeds the transactional missing-Outcome
boundary and actual CLI/RPC inspection. All T07 acceptance checks have retained
local evidence below.

**Production durable dispatch and automatic recovery remain disabled.** This is
the narrow deterministic tracer required by T07, not installed-component, real-
provider, retirement/recovery, automatic-scheduling or release readiness. No live
Forge state, profile selection, worktree or real-provider credential was changed.
Earlier partial-status sections below are historical evidence, superseded by the
current supervised tracer. Existing checkout/planning changes were preserved;
verification covers the combined checkout, not an isolated commit.

## Supervised dispatch tracer (latest)

Run from the repository root, with Node 24 and the locked runner install available:

```sh
cargo test -p forge-cli --test durable_supervised_tracer
```

`crates/cli/tests/durable_supervised_tracer.rs` is a custom-harness executable that
runs **six isolated scenarios concurrently**. For host execution it re-execs itself
with explicit absolute paths and calls the **same** `forge_runner_host::supervisor`
implementation as the shipped `forge-runner-host` binary. Moving that implementation
into the host library avoids PATH/sibling-artifact discovery or recursive Cargo
builds in a test. The CLI adds only an existing-workspace dev-dependency on the host
library, with no new external dependency. Existing real binary/Node host regressions
continue to exercise the executable wrapper.

The test embeds the actual server router with the explicitly injected adapter and
matching protected capability root. Application dispatch is the single controlled
scheduler; the default Herdr background reconciliation loop is deliberately not a
second controller of these test Assignments. Automatic scheduling integration is
T16, not claimed here. No CLI/server response is substituted in the accepted,
missing, rejected or denied cases. The uncertainty case intentionally withholds an
actual committed bootstrap reply as described below.

`support/supervised_dispatch.rs` provides the finite controlled adapter. New launch
preflight and the immediate provision boundary delegate to T06's profile/auth
preparation. The existing server capability wrapper supplies only the short
bootstrap and protected file metadata. Real one-shot provision/launch intents and
acknowledgement deadlines precede the process effect. Immutable launch intent is
reserved and marked Unknown before spawning. The runtime directory comes from
`DaemonAuthority::prepare_directory`, not a fixture-selected store path. The host
acquires/shared-inherits its flock before the Node worker receives readiness; a
second real host start rejects with `runner-owner-busy` before opening storage.

`DaemonAuthority::bind` validates the directly spawned Child and live kernel
ownership, and the real service transactionally commits/delivers the initial grant
and starts its actual renewal driver. Neither the JS lease fixture nor a caller-
invented grant/epoch is used. `runner/src/owner.ts` shares the existing worker-side
ownership checks with the synthetic runner; it is included in the source component
hash. The Node worker validates the v1 request and full live lease and invokes
`createCheckRuntime` before any model/tool construction or runtime writable open.
At successful bootstrap it synchronizes with the test controller: context is
consumed but provider/tool counters are zero. Repeated app dispatch returns the
same bound Assignment with one provision. Only then is the test barrier released;
that barrier grants no authority and every execution boundary still checks the
lease. The Linux abstract socket used for this barrier avoids filesystem-socket
path-length limits and is not a new production protocol/control.

| Scenario | Observed evidence |
| --- | --- |
| Accepted | The guarded fake model makes two requests. Its actual Bash tool invokes a controlled effect that creates a new clean Git commit B and submits the **first** accepted Outcome through the actual delivered Forge CLI. One external mutation is recorded. Forge retains input A, binds output B and authenticated actor/Assignment/generation, and exact replay returns that acceptance. The Task remains In Progress with safety resources reserved. |
| Missing | One model request ends in plausible final prose, with zero tools/effects. The host validates/persists the exact v1 admission response, independently reads the committed answer using physical read-only storage, and retains `missing-outcome` plus a hold. No Assessment, pass, Continuation, completion or successor is created. |
| Rejected | Two model requests and one actual Bash effect submit a stale-generation Outcome through the actual CLI. Rejection followed by final prose still produces `missing-outcome`, never acceptance. The effect occurs once; no replay/recovery is attempted. |
| Denied bootstrap | An intentionally incorrect protected capability file fails the actual CLI/server bootstrap. Model/tool counters remain zero, no runtime SQLite DB opens and admission remains Unknown. The real service retains authorization uncertainty/capacity. |
| Uncertain bootstrap | A controlled loopback proxy forwards exactly one actual authenticated bootstrap to the real server, lets its acknowledgement/start/context effects commit, then withholds the trusted reply with HTTP 503. The actual CLI/gate fails closed: no model, tool or runtime DB, no retry, retained Unknown/diagnostics/capacity. Server context-read is true while the worker never reports successful bootstrap. |
| Revoked before grant | Explicit fixture closure after retained process intent but before the initial grant makes the real service deny issuance. No bootstrap/model/tool/runtime SQLite open occurs. Ownership intent and capacity remain retained after reopen. Closure is not retirement proof. |

Each scenario also verifies Manual, dependency and Open Spec parent gates before
provisioning/capability/worktree effects. Missing model credentials likewise reject
before new Assignment/capability/worktree creation. No Herdr binding or Workspace
Task/Grilling selection is supplied. Herdr verification, automatic recovery and
retirement adapter methods panic if invoked. The process launch uses only the
absolute Node worker and Forge CLI, never Herdr resources or the Pi CLI.

Missing/rejected cases exercise actual human/JSON CLI → RPC Assignment inspection
and compare unchanged persisted state. All scenarios verify retained capacity
blocks another Task, explicit fixture closure/reopen retains exact resources and
runtime/attempt identity, and runtime files plus fully drained process output
contain neither the actual random capability nor the model-key canary. The model
submits via metadata/path-only tool environment. Model authentication remains
separate from Forge authority. Controller stop persists authorization uncertainty;
test process-group teardown does **not** retire product resources, release capacity
or claim universal descendant absence. Worktree bytes are never reset/cleaned to
make a test pass.

### Final verification

```sh
cargo test -p forge-cli --test durable_supervised_tracer
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
(cd runner && npm run check && npm test)
```

All final gates pass: **647 Rust harness tests passed, 0 failed, 1 pre-existing
opt-in probe ignored; all 6 parallel supervised-tracer scenarios passed within the
workspace gate; runner typecheck and 45 Node tests passed**. The six custom-harness
scenarios are reported separately, not silently added to Rust's harness count.
The renewed workspace gate includes the prior real CLI/Outcome tracer, backend
admission gates, Herdr/Grilling regressions and real service/host/Node lease tests.
The historical supervisor-death timeout did not recur in these runs; no diagnosis
or general flake-resolution claim is made. Clean installation, packaged/matched-
Revision delivery and real-provider verification remain unrun/reserved for later
Tasks, not waived release checks.

Initial composition runs found fixture errors: missing protected `.forge` setup
under ambient group-writable umask, an overlong filesystem-socket test barrier,
reversed constructor/field assumptions and proxy positional-param decoding. An
additional parallel run exposed a competing default Herdr background reconciler
administering the controlled Assignment; the tracer now embeds its explicitly
selected adapter and single scheduler through the actual server router, rather
than running two independent controllers. Provision/launch intents are now also
retained before process effects. A Clippy similar-name finding was corrected.
All affected focused and final default-parallel workspace/Node gates were rerun;
no ownership, authorization, retained-resource or freshness policy was weakened.

### Implementation and standards self-review

Against T07's “from Forge dispatch through the durable runner ... authenticated
bootstrap and accepted Outcome”: complete for the explicitly enabled deterministic
tracer, with real ownership/service/capability/CLI/server/tool composition rather
than a directly claimed Check, captured-only transport or fixture lease. Failed and
uncertain bootstrap, stale/revoked authority, missing/rejected Outcomes, exact
identity/evidence, source/auth preflight and ordinary dispatch gates are covered.
Production dispatch remains disabled exactly as requested. No scope creep or
incorrect completion claim found; later interrupted-effect/recovery/retirement,
packaging, automatic scheduling and real-provider deliverables remain deferred.

Standards review found **no introduced violations**. Production changes extract
existing supervision/ownership and expose trusted server composition only; app/
store policy remains authoritative. The internal dev-dependency reuses the actual
host, external calls stay outside transactions, environments/listeners/stores are
isolated, synchronization is explicit and all uncertainty/resources remain retained.
This is a task-local self-review, not independent T18 review or release approval.

**T07 is complete; T08 may proceed.**

## Application-dispatched component tracer (previous)

The three real CLI/server/model/tool cases now enter through
`Application::dispatch_record`, not a direct store Check claim. The application
selects the immutable durable Check pin and prepares the actual Git Managed
Worktree using the server's `GitWorktreeAdapter` and `GitInspector`. No worktree
claim or checkout is pre-created by the fixture. The bound Base Commit remains
explicitly supplied through the existing store contract.

A test-only durable transport adapter captures the actual provision context and
runs `durable_profiles::prepare` against the bound checkout and explicit protected
model-key file. The component runner consumes that prepared configuration,
including the registered tool set and context, rather than a hand-built execution
profile. Bootstrap identity is derived from the captured Assignment/profile/cwd
and the app-created persisted Check attempt. Assignment IDs are no longer fixed
fixture strings; exact authenticated Outcome attribution and replay assertions use
the app-selected identity.

All three cases assert that:

- Missing explicit model credentials reject before any worktree claim,
  Assignment, capability file or transport provisioning; restoring the explicit
  key permits dispatch without weakening the gate.
- Exactly one provision context is captured; repeating dispatch returns the same
  Assignment without another provision. The Check, pin, input evidence and
  execution directory match the retained attempt and prepared configuration.
- App admission needs no Herdr binding or Workspace Task/Grilling selections.
  Herdr verification, recovery and retirement methods panic if called.
- Accepted, prose-only and rejected-Outcome execution still traverses the actual
  CLI/HTTP daemon, pinned Harness, guarded fake model and actual Bash tool.
  Committed-answer observation, retained missing-Outcome inspection/closure and
  secret-canary checks remain in place.

**Limits:** the adapter captures the transport request and the fixture later
executes it; it does not start a supervised worker from `provision`. The JS lease,
protected test-selected runtime directory and capability file are still fixtures.
There is no shared flock, real grant/renewal service, canonical runtime mapping,
production adapter or installed Revision attestation in this composition. The
full app-dispatch → supervisor → grant-service tracer is still outstanding; T07
remains partial and T08 remains blocked. No production or live Forge state changed.

Verification from repository root:

```sh
cargo test -p forge-cli --test durable_check_bootstrap
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
(cd runner && npm run check && npm test)
```

All final gates pass: **647 Rust tests passed, 0 failed, 1 pre-existing opt-in
probe ignored; 45 Node tests passed, 0 failed**. The focused tracer's three cases
pass in both the final focused run and the default-parallel workspace run,
including the missing-key assertions. Initial compilation exposed reversed Git adapter/inspector
constructor arguments; the first execution exposed an omitted persisted Base
Commit. Both fixture errors were corrected without changing product policy.
The previously observed intermittent supervisor test passed in this run; that is
not a diagnosis or claim that the intermittent failure is resolved. No packaged
or real-provider release verification was performed.

Implementation self-review against T07's “from Forge dispatch through the durable
runner”: app dispatch, preparation and component execution now share the exact
bound Assignment/configuration, but the transport capture and fixture lease leave
supervised dispatch **partial**. “Use existing ... bootstrap and Outcome commands”
and accepted/missing/rejected Outcomes retain their actual CLI/server evidence.
No scope creep or incorrect completion claim identified. Standards self-review:
no introduced violations found; the new adapter is test-only, delegates profile
policy, isolates credentials/stores/listeners, and releases its capture mutex
before asynchronous execution. This is not independent T18 review.

## Committed answer observation and diagnostic delivery (previous)

`runner/src/storage.ts::observeAnswer` reads the exact journal Launch, root and
accepted submission ID through physical read-only SQLite and the pinned derived
read adapter. Only a committed `input` submission marked `done` with an answer
entry counts as answered. Absent journal/root/input and unsettled submissions are
unknown; failed runs are unanswered. A passive write is not an answer. Full Launch
or admission mismatch rejects. No Harness, scheduler, writable repair, model/tool
call, new input or lifecycle verdict is created by inspection. The helper returns
only the observation classification, never prose or a claimed Forge verdict.

The real CLI/server component tracer now reserves actual runtime intent in Forge
**before** its first bootstrap and marks send intent unknown before execution.
The worker journals that same Launch/root in its runtime store, submits using the
stable operation ID and reports the actual committed submission ID. After writer
closure, `observeAnswer` reads the committed answer. The trusted Rust fixture
persists the accepted admission and passes that observation to the existing
transactional missing-Outcome boundary. Three isolated default-parallel scenarios
exercise this composition:

- An authenticated accepted Outcome reports `OutcomeAccepted`, preserving the
  existing exact evidence, actor-attribution, replay and revocation checks.
- Plausible completion prose with no Outcome reports `MissingOutcome`.
- A fake-model Bash submission rejected for stale generation, followed by final
  prose, also reports `MissingOutcome`; a rejected request is not acceptance.

The missing cases verify one retained diagnostic resource and Dispatch Hold,
unchanged unresolved Check/Task/attempt, one Assignment and reserved capacity.
Actual human/JSON CLI → RPC inspection displays the newly produced diagnostic
without changing persisted state or revealing behavior/capabilities. Explicit
fixture closure plus a fresh repository connection retains the same resources,
blocker and capacity; that closure is **not** evidence of safe retirement.
All scenarios scan runtime files and process output for secret canaries.

Four new Node tests cover inspection before intent/root, a pending and committed
answer beside a writer, stopped-writer observation, failed runs, all Launch-field
mismatches, wrong admissions and settled passive writes. They compare committed
DB/WAL bytes and count provider calls. SQLite read-only handles may create empty
WAL/SHM coordination files and change reader-lock metadata; those are not runtime
initialization, scheduling or committed history writes, and the tests deliberately
do not claim byte-for-byte immutability of that lock metadata.

Verification from repository root:

```sh
(cd runner && npm run check && node --test test/completion.test.mjs)
cargo test -p forge-cli --test durable_check_bootstrap
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
(cd runner && npm run check && npm test)
```

All final gates pass: **647 Rust tests passed, 0 failed, 1 pre-existing opt-in
probe ignored; 45 Node tests passed, 0 failed**. Initial new tests exposed two
fixture problems: waiting for provider readiness before starting `Submission.wait`
left scheduling unrequested, and comparing reader-lock sidecars overstated the
read-only contract. They now start the wait before observing the pending boundary
and compare committed bytes. Initial Rust compilation found incorrect fixture
field names; the first CLI inspection used the HTTP base rather than `/rpc`.
Those fixture errors were corrected and all gates rerun. Earlier interrupted test
commands are not counted as passes. No safety policy was weakened.

**Remaining:** this is real admission/bootstrap/model/tool/CLI/answer/store
**component** integration, not app-dispatch → supervisor → grant-service execution.
The Check is still directly claimed, the lease remains a fixture and runtime
storage is a protected test-selected directory, not the daemon's canonical mapping.
Runtime acceptance is retained after the worker report; a lost report remains
unknown and must not be replayed as permission. The component uses the current
source hash but not installed/matched-Revision attestation. No production adapter,
renewal driver, ownership proof, automatic recovery or real-provider test was added.
T07 stays in progress and T08 stays blocked.

Implementation review against T07: “Persist and expose missing-Outcome
diagnostics” now has actual committed-answer delivery through the component
fixture; “from Forge dispatch through the durable runner” remains partial because
supervision and app dispatch are separate tests. No scope creep or incorrect
completion claim found. Standards self-review found no introduced violation:
identity validation precedes read-only inspection; the store retains atomic
admission/diagnostic policy; external operations remain outside transactions;
fixtures isolate stores/listeners/environments and use explicit synchronization.
This is not independent T18 review or release readiness.

## Bootstrap-gated runtime and fake-model Outcome component (previous)

- `runner/src/check-runtime.ts::createCheckRuntime` snapshots the trusted launch,
  full Check binding, capability metadata and explicit configuration before the
  bootstrap await. It requires the configured cwd to match the binding, invokes
  the actual bootstrap gate and only then constructs profile model/tool inputs.
  The authenticated behavior and complete context are added to agent instructions.
  No pre-bootstrap model request, storage open or conversation is performed.
  Bootstrap/configuration/authentication failure fences this lease consumer with
  a fixed redacted error. This helper is not an executable or production caller;
  the host still owns pre-capability preflight, live grant issuance, exclusive
  ownership, admission and durable failure retention.
- Extended `crates/cli/tests/durable_check_bootstrap.rs` and its Node driver to
  compose the helper with the pinned Harness and real SQLite storage. A guarded
  faux provider makes exactly two requests: one Bash tool call and one final
  prose response. The actual guarded Bash tool executes the delivered CLI against
  the actual HTTP daemon/store and submits the **first accepted Outcome**.
  The test verifies one successful tool result and confirms accepted state before
  Rust reads the result back through exact replay. Existing wrong/stale/revoked
  submission and immutable input/output evidence assertions remain in place.
  Runtime files and process outputs are checked for capability/model-key canaries.
- Two new Node tests cover successful trusted instruction composition and fenced
  bootstrap, lease, cwd, configuration and model-authentication failures. Existing
  bootstrap failure/redaction tests remain green. The executable-source component
  digest now includes the new helper; no wire/storage schema or dependency changed.

**Limits:** the Check is still claimed directly, its lease is a JS fixture, and
no supervisor/shared flock/runtime reservation is composed with app dispatch.
Rust prepares the changed Git commit and Outcome file; the fake model submits
that file, not an independently implemented deliverable. Only the test replaces
the selected model with faux; production profile compatibility is unchanged.
The component's private test store is not evidence of host ownership or the
Forge runtime journal contract. Runtime answer delivery to the missing-Outcome
port remains missing. No real provider, installed component or live Forge state
was used. T07 stays in progress; T08 stays blocked.

Verification from repository root:

```sh
cargo test -p forge-cli --test durable_check_bootstrap
(cd runner && npm run check && node --test test/bootstrap.test.mjs)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
(cd runner && npm run check && npm test)
```

All final gates pass: **645 Rust tests passed, 0 failed, 1 pre-existing opt-in
probe ignored; 41 Node tests passed, 0 failed**. The real CLI/fake-model driver is
exercised by the Rust integration test, not counted as another Node test. An
initial typecheck found a readonly agent-property assignment; the helper now
returns a composed agent value instead. No policy or safety gate was weakened.
Clean installation, packaged integration and real-provider verification were
not run. The earlier supervisor-test timeout remains historical evidence, not
claimed fixed by this increment.

Implementation self-review: T07's “fake model and actual runner/CLI bootstrap
path” now has model/tool/CLI component evidence. “From Forge dispatch through the
durable runner” remains partial because dispatch, retained admission, supervision
and this component are not composed. “Persist and expose missing-Outcome
diagnostics” still lacks actual runner delivery. No scope creep or incorrect
completion claim found. Standards self-review found no introduced violation;
external calls remain outside database transactions, environments/listeners/stores
are isolated and secret-file delivery is retained. This is not independent T18
review or release readiness.

## Backend-specific Check admission increment (previous)

The app's Check dispatch now distinguishes `CheckDispatchPreflight::Herdr` from
`PiDurable`, based on the immutable pinned profile, not current Workspace Task
selection. The store verifies the token against that same pin inside the immediate
claim transaction. Herdr continues to require its complete Workspace selection,
exact binding and current endpoint generation. A caller cannot bypass those checks
by sending the durable token for a Herdr pin. Unknown/mismatched providers reject.

Durable Check admission no longer requires Task/Grilling Workspace selections or a
Herdr binding. The actual pinned adapter must pass preflight/probe; the pre-worktree
path also checks the backend before filesystem preparation. Production server
adapter selection remains disabled for durable execution. No new execution grant,
capability issuance, supervisor, automatic recovery or cleanup behavior was added.
A preflight token is an internal consistency input, not authorization to run.

Added four tests in `crates/store/tests/durable_check_preflight.rs`:

- Application dispatch with an explicitly injected durable-only adapter, a prepared
  fixture checkout and no Workspace profile selections/binding: exactly one
  provision call at the bound checkout, repeat dispatch returns the same Assignment,
  one Check attempt, and no Assessment. Herdr verification/recovery/retirement calls
  panic if attempted. This is a protocol fake, **not runner/model execution**.
- Cross-backend preflight tokens reject without an Assignment or bound attempt.
- Both application and store preserve Manual, dependency and Open Spec parent gates
  for durable dispatch, with zero provisioning calls.
- Herdr application/store admission still rejects absent Workspace configuration.

The existing real runner-module → CLI → HTTP/store bootstrap/Outcome component
tracer now runs without any Herdr binding or Workspace profile selections; its
complete bootstrap identity explicitly contains `provider_workspace: None`. The
Check is still claimed directly and the JS lease is still a fixture. This renews
bootstrap/Outcome component evidence, not the missing supervised execution tracer.
Existing Assignment insertion may still snapshot an already configured Herdr
binding; this increment does not rewrite historical bindings or change their schema.
The durable adapter must use its own retained runtime ownership, never that binding.
Workspace-wide status remains the existing mixed/Herdr readiness surface; it is not
proof that durable launch is enabled.

Verification from repository root:

```sh
cargo test -p forge-store --test durable_check_preflight
cargo test -p forge-cli --test durable_check_bootstrap
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
(cd runner && npm run check && npm test)
```

Final Rust gates pass: **645 tests passed, 0 failed, 1 pre-existing opt-in probe
ignored**. Runner typecheck passes. The first Node suite had 38 passes and one
20-second timeout in unchanged `supervisor death cannot free ownership while its
worker survives`; the complete rerun passed **39/39**. This observed intermittent
process-test failure is retained, not claimed resolved. Initial implementation runs
also caught an unavailable fixture API, an outdated bootstrap binding expectation,
Clippy findings and a Herdr readiness error-order regression. These were corrected;
the full workspace suite was rerun successfully without weakening the regression.

Compatibility: an internal Rust repository method now takes a typed Check token;
all in-workspace callers were updated. No storage/schema, public CLI/RPC, runner
wire, dependency or runner-component digest changed. No live Forge state changed.

Implementation self-review: T07's “Durable readiness must not require a Herdr
Workspace binding” now has application/store and real bootstrap component evidence.
“From Forge dispatch through the durable runner” and “deterministic local tracer
with a fake model” remain partial/missing: the dispatch fake and CLI component are
separate, not composed with runtime admission, real supervision, model tools or
answer delivery. No scope creep identified. Standards self-review found no
introduced violation; atomic pin/token validation and existing safety gates remain.
This is not independent T18 review or release verification.

**T07 remains in progress; T08 remains blocked.**

## Real CLI/server bootstrap and Outcome component tracer (previous)

Added `crates/cli/tests/durable_check_bootstrap.rs` and the test-only driver
`runner/test/check-bootstrap-tracer.mjs`. The Rust fixture registers an actual
`pi-durable` Task profile, pins a Check, prepares a real Git Managed Worktree,
claims the Check through the store and supplies its persisted complete binding to
`bootstrapCheck`. That module invokes the actual built Forge CLI against an actual
HTTP daemon and SQLite store, using an owner-only capability file. No CLI/server
response is substituted. Trusted context/behavior is checked but not printed.

Verified behavior:

- Wrong capability fails bootstrap without marking context read; successful
  bootstrap starts exactly the bound Task and records context consumption.
- An expected binding with changed input evidence fails closed and terminally
  fences the local JS authorization consumer. Revoked bootstrap also fails closed.
- Bootstrap alone creates no Assessment. Authenticated Outcome submission through
  the actual CLI rejects a missing attempt, stale generation and wrong capability.
- Implementation output B is a new clean Git commit distinct from bound input A.
  Acceptance binds B, retains immutable attempt/input A and replaces spoofed actor
  attribution with the authenticated Assignment ID.
- Exact Outcome replay returns the original accepted result, including after
  Assignment closure. A conflicting submission after closure is rejected. Closure
  is not permission to rewrite accepted results or an assertion of safe cleanup.
- The component prints only its fixed accepted/denied result. CLI/driver outputs
  are checked for capability canaries; the Node driver does not print behavior.

**Limits:** this is component integration, not the T07 execution tracer. The Check
is claimed directly rather than dispatched; the existing persisted Herdr binding
prerequisite is still supplied. There is no Herdr process/socket/resource or Pi CLI
in this fixture, but it does **not** establish binding-independent readiness. The
JS lease is explicitly a protocol fixture, not a real supervisor/service grant.
There is no model, conversation, runtime admission/store, external-call-counting
adapter, missing-Outcome observation delivery or new execution authority. Outcome
CLI submissions are driven by Rust, not a fake model's tool execution. Production
launch and automatic recovery remain disabled.

Verification from repository root:

```sh
cargo test -p forge-cli --test durable_check_bootstrap -- --nocapture
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
(cd runner && npm run check && npm test)
```

All final checks passed: **641 Rust tests passed, 0 failed, 1 pre-existing opt-in
probe ignored; 39 Node tests passed, 0 failed**. The new Node driver is exercised by
the Rust integration test, not counted as another standalone Node test. Initial
runs caught a fixture branch inconsistent with Forge's canonical Task branch,
incorrect assumptions about missing-attempt error classification and exact replay
after closure, and two compilation errors. Fixtures were corrected to preserve
existing policy; no production gate or redaction was weakened.

Implementation self-review against T07: “actual runner/CLI bootstrap path” and
accepted authenticated Outcome composition now have component-level evidence.
“From Forge dispatch” and “deterministic local tracer with a fake model” remain
missing, as does runtime-answer delivery. No out-of-scope lifecycle or execution
behavior was added. Standards self-review: no introduced violation found; isolated
stores, filesystem roots, child environments and daemon port, real Git evidence,
explicit process argv and no secrets in arguments/output. This is not independent
T18 review. No storage, wire, dependency or executable component digest changed.

**Summary: T07 remains partial; dispatch/supervisor/model/answer composition is
still missing. T08 remains blocked.**

## Bootstrap-gate increment (previous)

- `runner/src/bootstrap.ts::bootstrapCheck` consumes trusted persisted full
  Assignment/Check identity, launch, freshness generation, delivered capability
  metadata/path and an existing live lease. It opens no storage/conversation and
  creates no provider or tool registry. It is not registered as an agent tool,
  host control, public RPC or executable entrypoint.
- `Authorization::admitFor` verifies exact launch/generation before and after
  bootstrap. An otherwise valid lease for another runtime/Check cannot be
  borrowed. Identity inputs are snapshotted before awaiting the child.
- The only process invocation is the explicitly delivered absolute CLI with
  arguments `assignment bootstrap --json`, in the canonical bound worktree,
  with an explicit metadata/path-only environment. No shell, inherited Pi
  settings, secret env values, Herdr operation or Pi CLI invocation is used.
  The Forge CLI remains responsible for reading and authenticating the
  owner-only capability file; this module never reads its secret.
- Response version, complete immutable bound object (including evidence,
  continuation/recovery context), Record identity/type/status and separately
  labelled behavior are checked before context/instructions are returned.
  Expected numeric identities reject unsafe JavaScript integers rather than
  silently rounding them. This is defensive binding validation, not a replacement
  for app/store authorization or freshness policy.
- A 10-second timeout, 1 MiB stdout bound, lease cancellation, process failures,
  malformed UTF-8/JSON, drift or uncertain response fail closed. Stderr and
  upstream payloads are never surfaced in diagnostics. Failure terminally closes
  this local lease consumer; there is no retry or behavior fallback.
- Killing the directly spawned CLI is cancellation only, **not** proof that
  Assignment effects stopped. A future host caller must persist uncertainty and
  retain capacity through the existing service/store policies. This standalone
  gate cannot persist an Assignment diagnostic or authorize cleanup.
- The local component digest includes the new module. No protocol envelope,
  Forge/runtime storage schema, dependency or public CLI contract changed.

## Bootstrap-gate verification (previous)

From the Forge repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
(cd runner && npm run check && npm test)
```

All passed: **630 Rust tests passed, 0 failed, 1 pre-existing opt-in probe ignored;
39 Node tests passed, 0 failed**. This increment adds six Node tests, with real
child processes but **fake CLI executables**. The fixtures verify:

1. Exact CLI argv/cwd, environment allowlisting, secret-file path delivery and
   release of separately labelled context/behavior after successful validation.
2. Denial before spawn for absent/closed/expired or foreign-runtime leases,
   wrong Assignment/provider/generation and unsafe numeric identities.
3. No behavior release for wrong Assignment/profile, stale generation/evidence,
   changed worktree/recovery context, wrong Record/status or response versions.
4. Fixed redacted failure for nonzero exit with secret canaries, malformed,
   truncated, non-UTF-8, null and oversized replies, plus process-spawn failure.
5. Timeout of a still-running CLI without accepting partial output or retrying.
6. Cancellation after a complete response was printed but before process exit;
   printed success alone does not release behavior or retain local authority.

The initial targeted run caught a fixture alias: response mutation also changed
its expected binding. The reply now clones the expected binding, and the full
suite was rerun successfully. No implementation gate was weakened.

Not run/established by this increment: clean npm installation, real CLI bootstrap
through this gate, actual durable adapter dispatch, fake-model authenticated
Outcome tracer, installed matched components or real-provider requests. Existing
workspace tests still cover historical CLI/bootstrap, Check/Outcome and Herdr
behavior separately; those are **not** evidence of the missing durable composition.

## Bootstrap-gate implementation self-review (previous)

Against T07:

- “An explicitly enabled test Assignment goes end to end from Forge dispatch”:
  **missing**. The bootstrap function has no production caller; `main.ts` remains
  synthetic. No claim of headless Check admission is made.
- “Use existing `forge assignment bootstrap` and Outcome commands”: the exact
  bootstrap command is implemented at the process boundary, but tests substitute
  the CLI. Actual authenticated bootstrap and accepted/rejected Outcome
  composition remain **missing**.
- “Persist and expose missing-Outcome diagnostics”: **missing**. This gate does
  not observe conversation completion or mutate Assignment/Check state.
- “Preserve ... gates ... Check pin selection ... freshness validation”: no
  scheduling policy changed, but durable integration/regression proof remains
  **missing**. Exact returned identity is checked only defensively here.

No unrelated functionality or incorrect completion claim was introduced.
**Summary: T07 remains partial; end-to-end dispatch and Outcome authority are the
principal missing deliverables. T08 must not proceed.**

Coding-standards self-review (this increment only): no introduced violation found.
There is no new app policy, persistence mutation, external call inside a database
transaction or recovery/cleanup inference. The process boundary uses explicit
argv/environment, bounded outputs and static redacted failures. Tests isolate
files/environment and use readiness synchronization for cancellation. Their
protocol-fake limitation is explicit. This is not independent T18 review.

## Historical next-work checklist within T07

Closed by the supervised dispatch tracer above; retained here as planning history.

1. Compose an explicitly test-enabled real durable adapter with app Check dispatch.
   Backend-specific app/store Check admission is now implemented and tested with a
   protocol fake, without Herdr binding requirements. Wire it to actual retained
   runtime admission and supervised execution; do not mistake the injected adapter
   test for that composition. Preserve retained Herdr/Grilling guards.
2. Bind selected Check/profile, actual Managed Worktree, retained runtime identity
   and T06 prepared configuration. Preflight all mutable launch inputs before new
   capability issuance; recheck at the effect boundary. Reuse protected capability
   delivery and T05's real supervisor/grant service, not caller-invented authority.
3. Compose this gate with actual CLI/server bootstrap and pinned Harness/model/tool
   construction. Deliver only the short bootstrap before the successful response;
   trusted behavior/context must not enter execution through a pre-bootstrap
   model request. Retain failures durably rather than just closing the JS guard.
4. Compose the now-tested deterministic fake-model/Harness/tool/CLI/server
   component with actual supervised dispatch and retained runtime admission.
   Preserve exact evidence, wrong/stale/revoked submissions and counted external
   calls. No Herdr resource or Pi CLI may be needed for durable admission.
5. Move the now-tested committed-answer → missing-Outcome component delivery
   into that supervised host composition, without an Assessment, Continuation,
   completion, successor or capacity release. Renew manual/parent/dependency,
   Herdr/Grilling and all repository/runner gates, then mark T07 complete.

## Missing-Outcome persistence increment

Delivered:

- `app/src/durable_execution/completion.rs` defines a trusted host observation
  port and distinguishes an accepted Outcome from a missing Outcome. The caller
  supplies only immutable launch identity, never prose or an asserted verdict.
  This is not a new agent lifecycle tool, RPC method or execution entrypoint.
- `store/src/durable_execution/completion.rs` authorizes Workspace, exact retained
  runtime, profile and Check attempt inside one immediate transaction. Only an
  accepted runtime admission may report an answer. Pending, unknown, rejected,
  foreign and mismatched launches fail without writing diagnostic state.
- The same transaction checks the exact attempt's committed Outcome/result pair.
  If absent it retains a `missing-outcome` resource keyed by attempt id and a
  diagnostic Dispatch Hold. Existing holds and resources are preserved; duplicate
  observations preserve the first diagnostic timestamp. Errors roll back both
  effects. Closure/invalidation does not prevent delayed diagnostic retention or
  restore execution authority.
- No Assessment, Continuation, attempt closure, Task/Assignment lifecycle change,
  successor, capability issuance, cleanup or capacity release is performed.
  Missing Outcome stays unresolved even if the provider reports plausible success.
  Rejected Forge Outcomes do not count as accepted. Existing lease policy blocks
  renewal while the diagnostic is unresolved, even after hold clearance. This
  transaction does not cancel an active grant or establish process death; the
  existing bounded lease/watchdog remains necessary.
- `AgentAssignmentInfo::recovery_blockers` exposes `missing-outcome` for an
  unretired diagnostic resource. Existing CLI/RPC assignment inspection carries
  the resource and blocker, without provider/Git calls or execution admission.
  A later accepted Outcome observation reports acceptance but does not erase an
  earlier diagnostic/hold or infer a pass/freshness/retirement verdict.

Compatibility: no schema, dependency, CLI argument, RPC method or serialized field
change. The existing unconstrained resource-kind and blocker-string contracts gain
`missing-outcome`; existing inspection renders them generically. Runtime wire v1
and store schema 21 are unchanged. This is not a new recovery-resolution operation.
Later authorized intervention must reconcile retained diagnostics deliberately;
ordinary hold clearance cannot do so.

### Verification (combined checkout)

Commands run from the repository root:

```sh
cargo test -p forge-store durable_execution -- --nocapture
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
(cd runner && npm run check && npm test)
```

All final gates passed: **640 Rust tests passed, 0 failed, 1 pre-existing opt-in
probe ignored; 39 Node tests passed, 0 failed**. The focused store run passed
24 tests (nine new missing-Outcome tests). One new real CLI/HTTP/RPC inspection
integration test verifies human/JSON diagnostic visibility, unchanged persisted
snapshot, no prompt disclosure and zero provider/Git calls through panic guards.
Its diagnostic resource is a presentation fixture, not an actual runner answer.

Store coverage includes exact identity/scope and admission rejection, unchanged
Checks/work/Assignment state, replay/reopen and concurrent observers, delayed
observation of closed attempts, existing-hold preservation, rollback on injected
hold-write failure, a rejected stale Forge Outcome, and lease denial after hold
clearance. The accepted-Outcome branch uses explicitly seeded committed-field
fixtures; **it is not evidence of authenticated Outcome acceptance**. Existing
CLI/server/Outcome regressions remain green but do not establish the missing
runner composition. The first compilation caught a test partial move; the first
Clippy run caught a wildcard import. Both were corrected and all gates rerun.

Not run/established: new npm installation, actual durable dispatch/bootstrap/
Outcome tracer, actual runtime-answer delivery to this port, installed matched
components or real-provider verification. There is intentionally no default
production caller of this new internal port.

### Implementation and standards self-review (this increment)

Against T07's “Persist and expose missing-Outcome diagnostics”: transactional
persistence and existing CLI/RPC visibility are implemented, but delivery from the
actual runner remains missing. Against “Build a deterministic local tracer with a
fake model and actual runner/CLI bootstrap path”: still missing, as are controlled
durable Check admission and authenticated accepted/rejected Outcome composition.
No additional lifecycle behavior or automatic recovery was introduced.

Coding standards: no introduced violation found after corrections. App owns the
observation contract, store owns atomic enforcement, and CLI/RPC use existing
adaptation. No external calls occur within the transaction. Scope and immutable
identity precede reads/mutations; retention survives closure and retries without
clearing safety evidence. Added tests isolate stores/listeners and synchronize
concurrent observers with a barrier. This is self-review, not independent T18 review.

**Summary: missing-Outcome persistence is now available as an internal boundary;
T07 remains partial because dispatch and runtime/CLI/Outcome composition are absent.
T08 remains blocked.**
