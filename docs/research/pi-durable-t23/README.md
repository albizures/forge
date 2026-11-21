# T23 — Compose the production durable dispatch and recovery adapter

Corrective Task for T18 **B1**, server part
([review](../pi-durable-t18/README.md#b1--blocker-no-production-durable-launch-or-execution-path-exists)).
Controlled deliverable. New durable launches stay **withheld by default** behind the T16 release
gate, and no command enables them. Packaging `forge-runner-host` and handing matched components
to a managed daemon is T24.

## Problem (T18 B1, server part)

- `retirement::Factory` refused `pi-durable` in `validate_profile`/`validate_preflight`, and its
  adapter returned "durable production dispatch and recovery remain disabled" from `probe`,
  `provision` and `observe`. Production composition set `recovery: None`.
- `durable_execution::Transport` had no non-test implementation.
- Launch orchestration existed only in `crates/cli/tests/support/supervised_dispatch.rs`: it
  blocked until the run settled, used the fixed `controlled-source-tracer` Revision, and drove
  admission, grants and retirement handoff from test code.

## Delivered

### Production launcher (`crates/server/src/durable_authorization/launch.rs`, new)

`Launcher::provision` is short and non-blocking. In order:

1. Validates the explicit profile against the bound Managed Worktree and the configured
   model-key file (`durable_profiles::prepare`).
2. Takes the **atomic single-launch claim** `begin_agent_provision`. If the claim already
   exists (a duplicate, a previous daemon lifetime or a closed Assignment), it returns
   `durable-launch-claimed` (`Conflict`) and spawns nothing.
3. Mints fresh random identities (operation, runtime, incarnation, conversation, store) and
   records the **real Forge Revision** (`forge_build_info` commit, `-dirty` when built from a
   modified checkout) and the **attested component hash**.
4. Commits admission intent with `reserve` (`Pending`) **before any external effect**.
5. Prepares the canonical protected runtime directory and writes `check-runtime.json` with
   the T22 writer `forge_runner_host::check_runtime::write`. The directory must be separate
   from the worktree.
6. Commits launch intent (`begin_agent_launch`).
7. Spawns the matched host with explicit arguments, `env_clear`, and piped stdin/stdout. It
   then registers a daemon-owned driver and returns.

The **driver thread** (one per launch, owned by this daemon lifetime):

- Requires the host's `owned` event, with compatibility equal to the Launch's component hash.
- Binds through `DaemonAuthority::bind_inspected`, the interrupted-effect gate, before the
  first grant. It then delivers the first lease before any admission request.
- Admits through the existing idempotent `durable_execution::admit`: `reserve`, then
  `begin_send`, then the transport, then `record_result`.
- **`PipeTransport`** (the production `Transport`) sends the T22 `RunnerRequest`
  (`Admit`/`Observe`) on the private pipe. It waits up to 120 s for the correlated
  `RunnerResponse` (`RunnerResponse::validate`), renewing the lease while it waits and keeping
  any runner events for the supervision loop. A refusal, a malformed line, a closed stream or
  a timeout returns an error, so the durable admission **stays `Unknown`**.
- After an `Accepted` admission:
  - It reconciles the one-shot `provision-intent` marker. This is the documented rule for
    every provider ([Dispatcher](../../dispatch.md)): reconcile only after the launch
    succeeded and all identities are retained. The `durable-runtime` identity and its capacity
    stay until verified retirement, and the marker row still prevents a second launch.
  - It renews on the app-owned schedule.
  - It delivers recorded follow-ups, using the recovery driver's shared helpers.
  - It parses every stdout line strictly as `RunnerOutput`.
- On `settled`, it inspects the answer through the matched answer inspector and
  `observe_answer`.
  - With an accepted Outcome, the state becomes `Idle` and the driver keeps renewing.
  - With a missing Outcome, the state becomes `Blocked` (intervention) and the driver keeps
    renewing. This lets the operator's recorded follow-up ("state the result with the Outcome
    tool") reach the same conversation, and Forge may still refuse a renewal.
- `runtime-failed`, an unexpected reply or a non-protocol line fences renewal
  (`AuthorityUnavailable` retained).
- When the stream closes while the driver is still renewing, the result is `Crashed`. The
  driver fabricates no stop or hold, reaps the host after it has written its crash receipt,
  and leaves recovery to exact proof.
- Retirement (`stop_for_retirement`) stops the session and hands the **same directly spawned
  `Child` and original pipe** to `retirement::Controller::register`. There is no PID
  adoption.

### Production adapter and composition

- **`retirement::Factory`** gains `launcher: Option<Arc<Launcher>>`.
  - Durable `validate_profile`/`validate_preflight` validate through the launcher. They refuse
    when no launcher is composed.
  - `adapter_for` stays ungated (original-backend routing).
- **The durable adapter:**
  - `probe`: matched launch components are composed. This is not eligibility.
  - `validate_provision`/`provision`: delegate to the launcher.
  - `observe`: returns the live driver's observation. Without one, a retained runtime is
    `Unknown`, and unreadable runtime facts are a `Transient` error. It never returns
    `Absent` or `Done`.
  - `recover`: a live first-launch driver is returned as-is, so recovery never competes with
    it. A `Crashed` driver is released to the existing `recovery::Controller`, which requires
    exact crash proof.
  - `retire`: stops the launcher driver, then the recovery driver, then runs the existing
    retirement.
- **`launch::compose`** builds the retirement controller, launcher and recovery controller
  exactly as `application_from_config` uses them.
  - `DaemonConfig.durable_execution` (`launch::Execution`) is selected on a manual daemon by
    `FORGE_DURABLE_RUNNER_HOST` and `FORGE_DURABLE_RUNNER_ENTRY`. Both or neither must be set,
    together with `FORGE_DURABLE_NODE` and `FORGE_DURABLE_EFFECT_INSPECTOR`, and all paths
    must be absolute.
  - Inspectors are the entry's siblings.
  - The component hash is attested from the matched `contract.ts` with the matched Node.
  - A managed daemon gets `None` until T24.
- **`ReleaseGate`** wraps the composed factory with `LaunchControl::default()` (withheld) and
  the persisted rollback. There is no Herdr fallback, and Herdr/Grilling prerequisites are
  unchanged.
- **`recovery::Components`** gains `host_arguments` (empty for the packaged host). The recovery
  driver's follow-up helpers and answer inspection are shared with the launcher.
- **`Authorization`** gains `stop`/`withdraw`, used by the launcher's own loop.

### Test-support orchestration removed

`supervised_dispatch.rs` no longer mints identities, writes the input, spawns hosts, binds,
ticks, sends admission, records results or registers retirement. It now composes
`launch::compose` behind the production `ReleaseGate`, with two **test-only substitutions**:

- controlled eligibility (`LaunchControl::enable_controlled`);
- the runner entry: `runner/test/supervised-check-worker.mjs`, which is the shipped
  `runCheckRunner` with only a faux model. The host is the test binary re-executing the same
  `forge_runner_host::supervisor` library (`host_arguments = ["--supervisor"]`).

A thin wrapper counts provisions and, for `bootstrap-denied` and `bootstrap-uncertain` only,
replaces the delivered capability file or daemon endpoint. For `revoked`, it closes the
Assignment immediately after the production provision returns.

`recovered-supervised-check-worker.mjs` is merged into the single worker. The worker detects
its recovery incarnation from its own out-of-band boundary file, because stdout must stay
strictly protocol 1 for the production driver.

## Acceptance checks

| Check | Evidence |
| --- | --- |
| With the gate admitting (test-only control), explicit and automatic dispatch launch through production composition, in the bound Managed Worktree, with no Herdr calls and no Pi CLI | `durable_supervised_tracer`. Explicit `dispatch_record` in every scenario. Automatic scheduling (`reconcile_agent_dispatch`) launches the independent Review in `continuation-chain`. `Execution` has no Herdr/Pi component. The original-backend stand-in `NoHerdr` and `verify_workspace` panic on any call. The worktree-separate runtime directory and the bound `execution_directory` are asserted. Each Launch carries `current_forge_revision()` and the attested hash. |
| Withheld or rolled back: same paths refuse with no allocation, fallback or pin rewrite | `withheld_refuses` (accepted scenario): the same production composition behind `LaunchControl::default()`. Explicit dispatch fails with `invalid-check-profile`, and an automatic scan launches nothing. There is no Assignment, worktree claim or capability directory, zero provisions, and Check pins are unchanged. Rolled back: the `rollback` scenario (T16 assertions, now over production composition). Server units `composition_without_matched_components_refuses_launch_and_recovery_without_fallback` and the existing `release_gate` and `rollout` tests. |
| Exclusive ownership across duplicate provision, daemon restart and an already running supervisor | `duplicate_provision` (accepted). A second provision in the same lifetime is a no-op. A **fresh composition** (new launcher and daemon epoch over the same store, standing in for a restarted daemon) returns `Conflict(durable-launch-claimed)` without spawning. Its `observe` is `Unknown` and its `recover` refuses while the original owner is live. Assignment info is unchanged. A direct second host gets `runner-owner-busy` from the kernel flock (every non-fault scenario). |
| Dropped requests/replies resolve through idempotent admission or stay `Unknown` | `PipeTransport` errors leave `begin_send`'s `Unknown` (code path above). A failure before intent records the runner's typed `Rejected{assignment-runtime-failed}` (`bootstrap-denied`, `bootstrap-uncertain`). Revocation before the first grant sends no request (`Pending`). No scenario exercises a dropped reply on a live pipe; see below. |
| Recovery, follow-up delivery and retirement work for production-launched Assignments | `recovered`: the worker is killed at its injected pre-generation boundary, and the production driver reports `Crashed`. Ordinary `reconcile_agent_dispatch` drives the production recovery controller from the host's crash proof, with the same `check-runtime.json`, Launch and attempt. The original admission is `answered`, and the Outcome is `OutcomeAccepted`. `missing`: an actual CLI `assignment-followup` is delivered by the launch driver into the same conversation and becomes `delivered` with its own admission. `retired`/`rollback`: CLI retirement goes through the launcher handoff to the original host, then purge (T21 journey). `continuation-chain`: acceptance-driven retirement of production-launched predecessors. |
| `durable_supervised_tracer` runs every scenario through production composition | All 10 scenarios: accepted, retired, recovered, continuation-chain, missing, rejected, bootstrap-denied, bootstrap-uncertain, revoked, rollback. |

### Changed scenario expectations (behavior, not weakening)

- **`retired`/`rollback`/chain no longer need `assignment-resolve-unknown`.** The T07 test
  adapter never reconciled its own `provision-intent`. Production reconciles it after an
  accepted admission, so verified retirement releases capacity directly. Capacity stays
  reserved until then, which is asserted.
- **Bootstrap failures record `Rejected{assignment-runtime-failed}` instead of `Unknown`.**
  The production driver commits the runner's typed reply (T22 contract). The old test broke
  out before reading it.
- **`revoked` records `Pending`.** The first grant is fenced, so no admission request is ever
  sent. If the grant were to win that race, the revoked capability would still deny bootstrap
  (`Rejected`), which the assertion also accepts. In every case there is no bootstrap, no
  Outcome and no effect, and capacity is retained.
- **Runner event-stream assertions became store and runtime assertions.** The production
  driver consumes stdout: `assignment_context_read` replaces the `bootstrapped` checks, and
  secret scans run over the runtime tree.

## Gates

`PATH` had the real Node 24.18.0 first (`~/.volta/tools/image/node/24.18.0/bin`); the Volta shim
breaks `durable_check_bootstrap`.

| Gate | Result |
| --- | --- |
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --no-fail-fast` | pass: 900 passed, 0 failed, 1 ignored (pre-existing opt-in probe). All 10 supervised tracer scenarios passed, including the retained Herdr (`workspace_dispatch_tests`, `rollout`) and Grilling (`registered_grilling_launch`) regressions. |
| `git diff --check` | pass |
| `runner`: `npm run check` (tsc) | pass |
| `runner`: `npm test` | pass: 97/97 |

`durable_supervised_tracer` also passed in four separate full runs during development.

## Not claimed

- **No real daemon-process restart test.** "Daemon restart" is a fresh composition (new
  launcher and epoch) over the same store while the original owner lives. An actual restart
  ends the driver thread with the old process. The host then loses lease renewal, the
  watchdog fences, and recovery requires the existing crash/quiescence proof. That path is
  T10 behavior and is not re-exercised here.
- **No live dropped-reply fault.** A reply lost on a live pipe leaves `Unknown` by
  construction, but no scenario withholds a `RunnerResponse`.
- **The recovery driver still stops renewing after a missing Outcome.** The launch driver now
  keeps renewing so follow-ups can be delivered. Recovery-mode behavior is unchanged.
- **The duplicate-host check is a direct spawn of the same supervisor library.** The
  production composition never spawns twice.
- **Packaging is not done.** Forgeup does not bundle `forge-runner-host` or pass
  `--durable-runner-*` to a managed daemon (T24), so a managed daemon composes no launcher.
- **No real-provider coverage (T19).** Production durable dispatch stays withheld, and
  automatic recovery is not composed on managed daemons.
