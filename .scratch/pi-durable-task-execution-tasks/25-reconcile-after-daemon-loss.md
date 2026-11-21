# T25 — Reconcile durable Assignments after daemon loss or lease expiry

## Delivered outcome

A durable Assignment whose daemon restarted, or whose lease expired while the runner was live, can be reconciled. Once proof of quiescence exists it can be recovered on the same attempt or retired, and its capacity is then released. Without that proof it stays blocked, and the reason is visible. No restart strands an Assignment with no supported path.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`.

- **Criteria:** AC6, AC7, AC8, AC11, AC12, AC17, AC22 and AC23.
- **Spec Implementation Decision:** *"Keep runtime storage and ownership recoverable across daemon/runner restart and compatible Revision operations."*
- **Origin:** this is a correction Task for T18 second-pass finding **B3** ([review](../../docs/research/pi-durable-t18/README.md#b3--blocker-any-daemon-restart-permanently-strands-live-durable-assignments-m1-escalated)), which escalates first-pass M1.
- **Responsible deliverables:** T09 and T10, plus T23 for the production composition.

What happens today when a daemon restarts while a production-launched runner is live:

- **The new daemon cannot see the old host.** The `Launcher` starts empty and never adopts (`crates/server/src/durable_authorization/launch.rs`).
- **The old host fences itself but never exits.** It loses its stdin, so renewal stops. At lease expiry the watchdog (`crates/runner-host/src/watchdog.rs`):
  - writes `watchdog.json`;
  - sends TERM, then KILL;
  - then *retains ownership indefinitely*, holding the flock forever.
- **Automatic recovery refuses.** `reconcile_crash` (`crates/server/src/durable_authorization/retirement.rs`) returns `false` when it cannot take the exclusive lock, or when `watchdog.json` or `retirement-intent.json` exists. Recovery quiescence (`crates/runner-host/src/quiescence.rs`) rejects the same markers.
- **Explicit retirement fails.** `Controller::retire` has no owned Child or pipe after a restart. It then requires the exclusive lock and an exact retirement receipt, and neither can ever exist.
- **No other path exists.** `resolve_unknown_agent_resources` retires only `provision-intent`.

The same dead end follows any lease expiry that the watchdog fences, even without a daemon restart.

### What to deliver

Deliver a watchdog-fenced reconciliation path that keeps the settled safety policy (SAFE-2, SAFE-3):

- **Host-side quiescence proof after a fence.** After the watchdog fence, the original host keeps its subreaper role. Once it has proven that no descendants remain, it writes a protected, exact receipt. The receipt is bound to:
  - the Launch;
  - the owner identity;
  - the last lease;
  - the watchdog marker.

  The host then releases the lock or permits exclusive inspection. If it cannot establish absence, it retains the receipt-less fence. Examples are an escaped or uninterruptible child, or a KILL it could not verify. In that case it keeps the existing indefinite hold and records a diagnostic.
- **Forge-side acceptance.** Forge accepts that receipt under `ExclusiveInspection`. The receipt can then serve two paths:
  - same-attempt recovery (T10 path), when effect inspection and authority allow it;
  - retirement reconciliation (T09 path), followed by explicit purge eligibility (T21).

  Capacity is released only in the transaction that accepts the proof.
- **Recovery after a daemon restart.** A restarted daemon can drive the paths above for Assignments it did not spawn, without adopting by PID or label (AC6). This includes `assignment-retire` reaching a fenced runtime through the protected receipt.
- **Operator visibility.** When no proof exists, status and retirement report a specific retained-uncertainty code, not a generic `uncertain`. The operator guide documents the manual investigation.
- **Documentation.** Correct `docs/durable-execution.md` ("plan for … recovery or retirement after any daemon outage") and the related recovery and retirement docs to match the delivered behavior.

### Must not

Do not:

- adopt or signal a process by PID alone;
- treat lease expiry, process exit or a missing heartbeat as proof of absence;
- release capacity without proof;
- weaken interrupted-effect blocking;
- add a command that enables durable launch.

If no safe proof mechanism exists for some case, return it for operator decision. Do not relax policy. An audited operator override is an approved alternative only if the operator explicitly selects it.

## Acceptance checks

- **Daemon restart with a live runner.** A real daemon-process restart while a production-launched runner is live leads to watchdog expiry and fence, then host quiescence proof. After the restart, the daemon either recovers the same attempt to an accepted Outcome, or retires and reconciles the Assignment, releasing capacity. No second writable owner ever exists.
- **Lease expiry without a restart.** This case reaches the same reconciliation paths.
- **Unprovable absence keeps the hold.** An escaped child or an unverified KILL keeps the hold and capacity, with a specific diagnostic visible through CLI/RPC status.
- **No relaxed inspection.** Stale, foreign or tampered receipts are refused. So are a watchdog marker without a receipt, and a receipt for another owner or lease.
- **Effects still block.** Interrupted-effect inspection still blocks recovery after the fence.
- **Retries are exact.** Exact retries of retirement and reconciliation return the original result.
- **Purge works afterwards.** Purge succeeds for a runtime reconciled through this path, and every new artifact is recognized (T21).

## Verification and evidence

Run real-process tests: a real daemon process, the packaged or test host, the Node runner, SQLite and Git, with a faux provider. Cover:

- daemon process kill and restart, both during model generation and during a long-running tool;
- lease expiry with the daemon alive;
- an escaped child;
- an unverified KILL;
- receipt tampering;
- recovery and retirement after restart;
- purge afterwards.

Add a `durable_supervised_tracer` scenario for the daemon-restart journey.

Run all four repository gates, the runner typecheck and the Node tests. Record the results in `docs/research/pi-durable-t25/README.md`, stating what is not claimed.

## Dependency

Depends on T24. Code Stack predecessor: T24.
