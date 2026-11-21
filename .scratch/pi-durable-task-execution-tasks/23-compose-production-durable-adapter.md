# T23 — Compose the production durable dispatch and recovery adapter

## Delivered outcome

The production server composition can launch, observe, recover and retire durable Task Assignments through the shipped runner whenever the release gate admits new durable launches. While the gate is withheld or rolled back, it keeps refusing. The default remains withheld.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC1, AC2, AC4, AC5, AC6, AC7, AC11, AC21. This is a correction Task for T18 finding **B1**, server part ([review](../../docs/research/pi-durable-t18/README.md#b1--blocker-no-production-durable-launch-or-execution-path-exists)). Responsible deliverables are T07 and T16.

Today's production composition (`crates/server/src/lib.rs`) wraps `durable_authorization::retirement::Factory` in `ReleaseGate`, with `recovery: None`.

- `Factory::validate_profile` and `validate_preflight` refuse `pi-durable`.
- `Adapter::probe`, `provision` and `observe` return "durable production dispatch and recovery remain disabled".
- `durable_execution::Transport` has no non-test implementation.
- Launch orchestration exists only in `crates/cli/tests/support/supervised_dispatch.rs`. That includes:
  - identity minting;
  - runtime directory preparation;
  - `durable_profiles::prepare`;
  - capability delivery;
  - spawning the supervisor and the lease driver;
  - recording the admission.

  There it blocks until the run settles, uses a fixed `controlled-source-tracer` Revision, and spawns the test binary as `--supervisor`.

Implement this in `server`, keeping policy in `app` and transactions in `store` (ARCH-1/ARCH-2):

1. A production adapter. Its provision must be non-blocking and must commit admission intent before any external call. It mints identities, prepares the runtime directory, delivers capabilities, and spawns the packaged `forge-runner-host` supervisor. After provisioning, a daemon-owned lease driver keeps the grant alive.
2. A production admission `Transport` over the supervisor pipe, using the T22 contract.
3. `observe` and `probe` from protected runtime facts, with unavailability reported as uncertainty.
4. Production recovery wiring through the existing recovery controller.
5. The real Forge Revision and matched component identities recorded on the Launch.

Move the test-support orchestration into this production code. The tracer should then exercise production composition with a test-only gate setting and a substituted provider. Remove the duplicate implementation in the test support.

Keep everything T16 established:

- withheld by default and a sticky rollback;
- no Herdr fallback;
- original-backend routing;
- Herdr and Grilling prerequisites.

Do not add any command that enables durable launch.

## Acceptance checks

- With the gate admitting launches (test-only control), explicit and automatic dispatch launch a durable Assignment through production composition. It runs in the bound Managed Worktree, with no Herdr calls and no Pi CLI.
- With the gate withheld or rolled back, the same paths refuse, with no allocation, fallback or pin rewrite.
- Ownership is exclusive, including across duplicate provision, daemon restart and a supervisor that is already running.
- Dropped requests and replies resolve through the existing idempotent admission, or stay explicit `unknown`.
- Recovery, follow-up delivery and retirement work for Assignments launched by production composition.
- `durable_supervised_tracer` runs every scenario through production composition rather than test-only orchestration.

## Verification and evidence

Run real-process tests (real daemon, host, Node runner, SQLite and Git; faux provider) for:

- launch and Outcome;
- missing and rejected Outcome;
- duplicate provision;
- daemon restart during execution;
- recovery after a runner crash;
- retirement;
- rollback with live Assignments.

Run the retained Herdr and Grilling regressions.

Run all four repository gates, runner typecheck and the Node tests. Record the results and the no-fallback evidence in `docs/research/pi-durable-t23/README.md`.

## Dependency

Depends on T22. Code Stack predecessor: T22.
