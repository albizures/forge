# T16 — Gate mixed-backend scheduling and safe rollback

Controlled T16 deliverable. Production durable dispatch and automatic recovery remain disabled: default eligibility is withheld in code until T19 retained verification and the T20 handoff.

## Delivered

### Launch gate (first increment, `ab4fc82`)
- `durable_authorization::release_gate::ReleaseGate` wraps the production provider factory. Only `validate_profile`/`validate_preflight` for `pi-durable` profiles consult it. When it refuses, it never falls back to Herdr and never asks the inner factory. `adapter_for` is deliberately ungated, so an existing Assignment keeps its original backend for inspection, recovery and retirement.
- `LaunchControl` defaults to `Withheld`. `Controlled` can be granted only by a composition that holds the control (the verification harness). `Disabled` is sticky. Production composition constructs the default and exposes no setter.

### Persisted operator rollback (this increment)
- App contract `durable_execution::launch_control`: `DisableRequest` (operation ID plus a nonblank reason, validated before I/O), the `Rollback` receipt, `Status` (`withheld | controlled | disabled`, `new_launches_admitted`, `replayed`, truthful notes) and a `Repository` trait.
- Store: a `durable_launch_rollbacks` single-row table, added to schema 23 in the same way as the earlier T12–T14 expansions (fresh schema, reference fixtures and upgrade paths). Rows can be neither updated nor deleted. Disabling runs in one immediate transaction and returns the first rollback unchanged on any repeat (`replayed`). There is no stored "enabled" state.
- `ReleaseGate.rollback_store` reads the persisted rollback on every new durable admission. A recorded rollback mirrors into the in-process control as sticky `Disabled`. An unreadable store refuses new durable launches (fails closed) and leaves Herdr untouched. Production wires the daemon store.
- Operator surface: RPC `durableLaunchStatus` / `durableLaunchDisable` and CLI `forge dispatcher durable-launch-status` / `durable-launch-disable --operation-id ID --reason TEXT` (with `--json`). The daemon status reports `new_launches_admitted: false`, because production composition never grants controlled eligibility.
- Embedded-server builder `Daemon::with_durable_operator_store`, the same operator service that production `serve` attaches.

## Evidence

All gates are run from the repository root, with real Node 24 first on `PATH` (the Volta shim breaks `durable_check_bootstrap`):
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `git diff --check` are clean.
- `cargo test --workspace --no-fail-fast`: 880 passed, 0 failed, 1 pre-existing opt-in probe ignored. The custom-harness supervised tracer passed all 10 scenarios.

New and changed tests:
- **App (2):** request validation; status never admits after a rollback.
- **Store (1):** sticky, idempotent and immutable rollback (UPDATE, DELETE and a second row all abort), surviving a store reopen.
- **Release gate (3):** withheld by default; controlled → rollback in-process; persisted rollback refuses across a "restart" even after a fresh control is enabled, and an unreadable policy store fails closed while Herdr still validates and the original adapter still resolves.
- **Production composition (`server/src/rollout_tests.rs`, 3):** these use `application_from_config_with_cli` with a scripted Herdr socket.
  - A durable-pinned Task is refused while withheld, after an RPC rollback, and after an app/daemon restart. Each time there is no Assignment, no worktree claim, no Herdr provisioning call, unchanged Check rows/pins and Workspace selection, and the Task stays Open. Rollback via the operator RPC is idempotent and rejects invalid input. Herdr Grilling still launches through Herdr afterwards.
  - An existing durable Assignment survives a Workspace Task-profile change plus a rollback, both live and after a restart. Its profile is unchanged, no duplicate or fallback Assignment is created, there is no Herdr provisioning, and `assignment-info` still works.
  - Spec and dependency gates for durable-pinned Tasks report `spec-execution-blocked` before any backend is consulted.
- **Real supervised harness (actual Linux host, shared flock, Node runner, CLI/RPC):** `SupervisedDispatch::app()` now composes the production `ReleaseGate` (controlled eligibility granted explicitly, persisted rollback read from the scenario store). So all ten scenarios (accepted, retired, recovered, missing, rejected, bootstrap-denied/uncertain, revoked, the continuation/review chain and the new rollback) run through the gate.
  - **Automatic scheduling:** in the Continuation → Implementation → Review chain, the independent Review is now launched by `reconcile_agent_dispatch` (normal scheduling). This happens only after the accepted Continuation output and explicit retirement/resolution. The run asserts exactly one provision and a fresh assignment/attempt/runtime/conversation/store, with the Review bound to the accepted Implementation output.
  - **Missing Outcome / incomplete retirement:** the existing `missing` and `retired` scenarios (now gated) keep the replacement Task unlaunched while capacity, the outcome blocker or the provision intent are retained.
  - **`rollback` scenario:** with the accepted Assignment still live, the actual CLI `durable-launch-status` shows `withheld` with nothing admitted. `durable-launch-disable` records the rollback, and a retry returns the original receipt. The inner provider still accepts the profile while the gate refuses it, which proves the gate is the cause. `assignment-status` on the original still works without leaking its capability. The original then goes through the full retirement path through its own host: unretired provision intent → explicit unknown confirmation → retired, capacity released. This covers a live Assignment and a closed-but-uncertain one under rollback. After that, the replacement durable Task is still refused across fresh app instances, with no Assignment, no claim, provision count 1 and status Open. Any Herdr call panics in this harness.

## Observations
- Registering a profile selects it as the Workspace Task profile. If that selection is a durable revision, Workspace readiness reports `task-profile-invalid` (an existing prerequisite), which also blocks Herdr Grilling in that Workspace. The durable backend is reached through Check pins, so the tests explicitly keep Herdr as the Workspace selection. That is a prerequisite blocker, not a fallback. T17 should document it for operators.
- The supervised harness registers the original host for retirement only in named modes, so the new `rollback` mode was added to that list. Before that, retirement reported `durable-retirement-unverified` for an unregistered host, which is the correct conservative outcome.

## Not claimed
- In the production-composition tests, durable preflight would also fail without matched components and credentials. Those tests therefore prove no fallback, no allocation and no redirect. That the gate itself causes the refusal is proved by the gate unit tests and by the supervised `rollback` scenario (inner admits, gate refuses).
- Unchanged-result publication and stacked-publication rules are not re-exercised with a durable pin. They are backend-independent pre-provider checks (`prepare_task_stack`) covered by the existing Stack/unchanged-checkout tests. Packaged full dispatch, real-provider runs and the release verification gate remain T19. Enabling default eligibility belongs to T20 and is not implemented here.
- Runner TypeScript was not changed, so runner typecheck and Node tests were not re-run.
