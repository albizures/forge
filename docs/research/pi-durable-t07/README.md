# T07 — Check-bound execution tracer

## Status: partial; do not advance to T08

This increment adds the runner-side bootstrap gate. It **does not complete T07**:
there is no enabled durable dispatch adapter, actual CLI/server tracer, accepted
Outcome or persisted missing-Outcome diagnostic from this work. Production durable
launch and automatic recovery remain disabled. No live Forge state, profile
selection, capability, worktree or real-provider credential was changed.

The checkout already contained uncommitted T06 work; that work was preserved.
The verification below covers the combined checkout, not a new isolated commit.

## Delivered increment

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

## Verification

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

## Implementation self-review

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

1. Integrate an explicitly test-enabled durable adapter with app Check dispatch.
   In particular, inspect `store/src/workspace_dispatch.rs::verify_preflight_selection`:
   Check acceptance currently joins the Herdr Workspace binding and provider
   generation. Durable readiness must not inherit that prerequisite, while
   retained Herdr/Grilling guards must remain intact. The general T02 provider
   seam alone does not establish backend-neutral Check admission.
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
5. Persist missing-Outcome diagnostics without an Assessment, Continuation,
   completion, successor or capacity release. Renew manual/parent/dependency,
   Herdr/Grilling and all repository/runner gates, then mark T07 complete.
