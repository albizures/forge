# T26 — Make failed durable launches reconcilable and retirable

## Delivered outcome

A durable launch that fails at any point after its claim does not strand the Assignment. Afterwards, one of two things holds:

- the Assignment can be retired, with capacity released on proof; or
- it stays blocked with a specific, visible diagnostic and a supported path out.

The host process is never orphaned.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC5, AC6, AC11, AC12, AC13 ("explicit authorized intervention remains possible") and AC17. This is a correction Task for T18 second-pass finding **B4** ([review](../../docs/research/pi-durable-t18/README.md#b4--blocker-a-launch-that-fails-after-its-claim-can-never-be-retired)). The responsible deliverable is T23.

`Launcher::provision` and the driver (`crates/server/src/durable_authorization/launch.rs`) have two gaps.

### 1. The claim and the reservation are not atomic

- `begin_agent_provision` commits the `provision-intent` claim in one transaction.
- `reserve` re-authorizes in a *new* transaction, which requires `state='provisioning'` and the current generation.
- A close, a generation bump or a daemon restart between the two leaves a permanent claim with no runtime row.
- From then on:
  - `provision` returns `durable-launch-claimed`;
  - `observe` and `recover` return Transient;
  - `Adapter::retire` fails at `self.runtime()?`.

### 2. Failures after the reservation strand the Assignment

The stranding failures are:

- a failed `check_runtime::write`;
- `begin_agent_launch` returning `false`;
- a spawn failure;
- a host that never sends valid `owned` evidence;
- a failed `bind_inspected`.

In each case, the driver retains a failure and returns. In the last two cases, the `Child` is dropped without being killed, reaped or registered with retirement. The host process is orphaned and stays a zombie. Afterwards, `Controller::retire` finds neither an owned entry nor a revoked lease, so it returns `uncertain` forever. `launch_tests.rs` has no test for any of these failures.

### What to deliver

Keep policy in `app`, transactions in `store` and wiring in `server` (ARCH-1):

- **Atomic claim (TX-1).** Do one of two things:
  - make the claim and the reservation one atomic precondition;
  - or make a claim with no runtime reconcilable, through `resolve_unknown_agent_resources` or an equivalent store-enforced path that proves no spawn happened.
- **Never-spawned reservations.** A reservation where the host was never spawned gets a retirement or reconciliation path that proves the spawn never happened. One example is the persisted pre-spawn state plus an absent runtime lock and owner.
- **Spawned hosts.** For every failure after the spawn, do one of two things:
  - hand the original directly spawned Child and private pipe to the retirement registry, so that the normal revoke-then-retire path works;
  - or have the host's subreaper stop its process group and write a protected exact receipt.

  Reap the Child either way. No path may drop a live `Child`.
- **Diagnostics.** Every failure keeps a specific diagnostic that CLI/RPC status shows (RUST-3).
- **Docs.** Update the retirement and recovery documentation to match the delivered behavior.

### Must not

Do not:

- adopt by PID or label;
- release capacity without proof;
- retry an `unknown` admission blindly;
- add a command that enables durable launch.

Coordinate with T25: share its receipt mechanism when a spawned host fenced itself.

## Acceptance checks

- **Closed between claim and reserve.** An Assignment closed, or its generation bumped, between the claim and the reservation ends retirable or reconciled, with no permanent `durable-launch-claimed`.
- **Every injected failure is covered.** Each injected failure ends retirable, with capacity released only after proof:
  - a failed runtime write;
  - `begin_agent_launch` returning `false`;
  - a spawn failure;
  - a missing or invalid `owned` event;
  - a failed bind or grant;
  - a daemon restart at each of these points.

  Where proof is impossible, the Assignment stays visibly blocked with a specific code.
- **No orphaned host.** No test leaves an orphaned or zombie host. Verify through the process tree or the subreaper evidence.
- **Retries are exact.** Exact retries of retirement return the original result.
- **No regressions.** Duplicate provision and the existing single-launch claim are unchanged.

## Verification and evidence

Add fault-injected tests in `launch_tests.rs` (class a) and real-process tests (class b: real host, Node runner, SQLite and Git, faux provider) for every failure point listed above, followed by retirement and purge. Run all four repository gates, the runner typecheck and the Node tests. Record the results in `docs/research/pi-durable-t26/README.md`, stating what is not claimed.

## Dependency

Depends on T25. Code Stack predecessor: T25.
