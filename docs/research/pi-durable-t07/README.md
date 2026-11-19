# T07 — Check-bound execution tracer

## Status: partial; do not advance to T08

Four increments provide the runner-side bootstrap gate, trusted app/store
missing-Outcome observation boundary, real CLI/server bootstrap/Outcome component
tracer and backend-specific Check admission described below. They **do not complete
T07**: there is no enabled production durable dispatch adapter, supervised fake-model
execution tracer or runner caller of the observation boundary. Production durable launch and automatic
recovery remain disabled. No live Forge state, profile selection, capability,
worktree or real-provider credential was changed.

The checkout already contained uncommitted T06 work; that work was preserved.
The verification below covers the combined checkout, not a new isolated commit.

## Backend-specific Check admission increment (latest)

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

## Next work within T07

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
4. Exercise a deterministic fake model with actual runner/CLI/server and existing
   Outcome commands, exact evidence, wrong/stale/revoked submissions and counted
   external calls. No Herdr resource or Pi CLI may be needed for durable admission.
5. Wire actual conversation answers to the missing-Outcome observation boundary
   below without an Assessment, Continuation, completion, successor or capacity
   release. Renew manual/parent/dependency, Herdr/Grilling and all repository/runner
   gates, then mark T07 complete.

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
