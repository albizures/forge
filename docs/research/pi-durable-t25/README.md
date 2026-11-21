# T25 — Reconcile durable Assignments after daemon loss or lease expiry

Corrective Task for T18 second-pass **B3** (first-pass M1, escalated)
([review](../pi-durable-t18/README.md#b3--blocker-any-daemon-restart-permanently-strands-live-durable-assignments-m1-escalated)).
Responsible deliverables: T09, T10 and T23. Controlled deliverable: new durable launches
stay **withheld by default** behind the T16 release gate. No command that enables
durable launch was added.

## Problem

Lease expiry with a live runner had no supported path. The usual cause is a
daemon restart that drops the host's renewal pipe. The original host's watchdog
fenced (`watchdog.json`), sent TERM and KILL, and then *retained ownership
indefinitely*. Three paths all refused because of the held flock or the marker:

- recovery (`reconcile_crash`, recovery quiescence);
- retirement (no owned Child, and no retirement receipt could ever exist);
- purge.

Capacity therefore stayed reserved for good.

## Finding: two real outcomes of a daemon kill

The new real-process tracer shows that killing the daemon does not always reach the
watchdog:

- **Ordinary exit.** With the shipped runner at a stuck generation, the runner sees stdin
  EOF. Closing its Harness under the stuck submission leaves an unhandled `Harness is
  closed` rejection, and Node exits. The original host then records ordinary crash proof
  (`recovery-crash-<seq>.json`). That path was already recoverable and retirable.
- **Fenced.** A runner whose shutdown hangs (a stuck tool, ignored EOF) reaches lease expiry
  and the watchdog fence. This is B3, and the case T25 fixes.

The `daemon-restart-exit` scenario covers the first outcome. The other three restart
scenarios inject a hang into the test worker fixture only: they keep it alive and ignore
EPIPE, uncaught errors and unhandled rejections. This makes the fence path deterministic.
The unhandled rejection in the shipped runner's EOF shutdown is recorded here as an
observation, not changed: process exit after EOF is safe, because the host still proves
it before anything is released.

## Delivered

### Host: post-fence quiescence receipt (`crates/runner-host/src/fence.rs`, `watchdog.rs`, `supervision.rs`)

- **Marker.** The fence writes `watchdog.json` as before. In a directory whose
  `watchdog.json` is retained history from an earlier, recovered episode, it writes its own
  `watchdog-<host pid>-<host start>-fence.json` instead.
- **Signals.** TERM and KILL go through `Supervised::signal_or_exited`. A failed signal
  that races the worker's own exit is classified as *exited* only after a positive Child
  reap within the same bounded 100 ms sample, with captured storage unchanged. Any other
  failure leaves the handle fenced.
- **Proof.** `Supervised::fenced_quiescence` requires, in order:
  - the handle is not fenced;
  - the direct worker was positively reaped;
  - `wait(NOHANG)` drains to `ECHILD`, which covers reparented and escaped descendants;
  - the captured directory, owner record and lock identity are unchanged.
- **Receipt.** Only then does `fence::record` write `watchdog-receipt-<lease sequence>.json`.
  The receipt is owner-only, `O_EXCL`, synced and immutable, and contains:
  - `marker`: the marker file name;
  - `marker_contents`: the marker's exact bytes;
  - `lease`: the last accepted lease, which carries the Launch and the owner;
  - `children_absent: true`.

  The marker itself must name the same owner and `last_lease`. The host then returns from
  supervision and exits, releasing the flock with its last holder.
- **No proof.** No receipt is written, and the hold is kept indefinitely, in these cases:
  - a fence with no marker or no accepted lease;
  - an unverified TERM/KILL;
  - ownership drift;
  - a surviving or escaped descendant.

  The host writes `watchdog-<pid>-<start>-quiescence-unproven.json` once: after the KILL
  attempt when proof already failed, or two seconds after a sent KILL with descendants
  remaining. If an escaped child really exits later, kernel proof follows and the receipt
  is written then.
- **Startup.** A fence marker of any episode makes a new host require exact recovery
  preparation, as crash receipts already did.

### Verification (`forge_runner_host::fence`)

- `receipt(dir, lease)` reads the exact receipt for this lease. It checks the lock identity
  against `lease.owner`, the receipt's lease, the marker's bytes and the marker's
  owner/last lease.
- `settled(dir)` requires every fence marker to be explained by a valid receipt.
- `Diagnosis::of(dir)` is a descriptive classification (`Unfenced`, `Proven` or
  `Unproven`). It never grants anything.

### Forge acceptance

- **Recovery** (`quiescence::waiting_receipt`, used by the recovery controller, the
  quiesced inspection, waiting-owner inspection and handoff). The unconditional
  `watchdog.json` refusal is replaced by `fence::settled`. Predecessor proof can now also be
  `PredecessorProof::Fenced` (the exact fence receipt for the candidate's last lease),
  beside quiesced and crash proof. A restarted daemon's ordinary dispatch scan claims and
  resumes the same attempt. All existing effect, authority, worktree and handoff checks are
  unchanged.
- **Retirement** (`retirement::Controller::reconcile_proof`, formerly `reconcile_crash`).
  Under `ExclusiveInspection`, the following must all hold:
  - no `retirement-intent.json`;
  - every fence marker settled;
  - crash proof *or* fence proof for the exact revoked last lease;
  - the matched effect inspection.

  Then the same transactional reconciliation runs. No host retirement receipt is
  manufactured. Capacity is released only in that transaction, and only when no other
  blocker (such as an interrupted effect) remains.
- **Specific codes.** An unowned runtime without proof reports one of two codes.
  `durable-runtime-owner-active` means the original host still holds the lock.
  `durable-runtime-fence-unproven` means a fence exists without valid proof. Both replace
  the generic `durable-retirement-unverified`.
- **Status.** RPC `assignmentDurableStatus` and CLI `assignment-status` add `runtime_fence`
  (`runtime-fence-unproven` or `runtime-fence-proven`). The value is read from host files
  without taking the lock. An unproven fence is also listed under `uncertainty`. Human
  output prints a `Runtime fence:` line.
- **Runner** (`runner/src/recovery-consumer.ts::unblocked`). This is defense in depth: a
  fence marker is accepted only when a receipt binds its exact bytes. The host remains
  authoritative.
- **Purge** (T21) recognizes episode markers (fencing rank), `watchdog-receipt-<seq>.json`
  (receipt rank) and the `quiescence-unproven` diagnostic. Near misses are still refused.

### Documentation

- `docs/durable-execution.md`: the authorization lease/outage section is rewritten. It
  describes ordinary exit, proven fence and unproven fence, the restarted daemon's recovery
  and retirement, the two specific codes and the manual investigation. Purge and trust
  bullets are updated.
- `docs/assignment-recovery.md`: new *Watchdog fence and daemon loss* runbook section, and
  the purge file list.
- `runner/README.md`: the watchdog section.
- ADR-0012: the lease-expiry fence follows the same proof rule.

## Policy kept

These rules are unchanged:

- No PID or label adoption, and no signal by PID alone.
- Lease expiry, process exit, a missing heartbeat or a free lock is never proof.
- Capacity is released only in the transaction that accepts the proof.
- Interrupted effects still block recovery, and block retirement finalization until they
  are resolved.
- An unproven case keeps the indefinite hold, plus a specific diagnostic.

## Evidence

### Real-process tracer: `durable_supervised_tracer` (four new scenarios)

A separate **daemon process** composes production (`launch::compose`), dispatches the Task
and owns the host's pipes, driver and registry. It is this test binary re-executed with
`--daemon`. The test SIGKILLs it while the runner is live. The test process's own
production composition never launched the Assignment, so it acts as the restarted daemon.
The RPC listener stays in the test process; only the launching daemon dies.

| Scenario | Fault point | Journey asserted |
| --- | --- | --- |
| `daemon-restart` | stuck before the first generation commit (hang injected) | While the orphaned host is live, recovery refuses. Then: lease expiry → fence → `watchdog-receipt-1.json` (no crash receipt) → status `runtime-fence-proven` → the ordinary dispatch scan recovers the **same attempt and Launch** to an accepted Outcome. |
| `daemon-restart-retire` | same | `assignment-retire` refused with `durable-runtime-owner-active`, capacity kept → fence receipt. A tampered receipt gives CLI JSON/human status `runtime-fence-unproven` (also under uncertainty), retirement `durable-runtime-fence-unproven` and capacity kept. Restored → retirement reconciled, capacity released → exact retry unchanged → `resume` → `assignment-purge-runtime` **removed**, with the fence marker, receipt and owner evidence all recognized. |
| `daemon-restart-tool` | long-running Bash tool that ignores cancellation (hang injected) | Fence receipt → recovery refused (the committed `interrupted-effect` is retained, the effect is never replayed). Retirement reconciles the runtime through the receipt but finalization is refused (`assignment-retirement-unreconciled`). After the operator's audited `assignment-resolve-effect` → retirement completes, capacity is released, and the exact retry is unchanged. |
| `daemon-restart-exit` | same generation point, shipped runner behavior | The runner exits on EOF → crash proof (no watchdog marker) → same-attempt recovery to an accepted Outcome. |

The test fixture changes are limited to `runner/test/supervised-check-worker.mjs` (the
boundary and hang injection for these modes) and `supervised-check-effect.mjs` (the
hanging tool). The harness accepts scenario-name filters
(`cargo test -p forge-cli --test durable_supervised_tracer -- daemon-restart`).

### Host real-process tests (`crates/runner-host/tests/authorization_session.rs`)

- **`committed_model_authority_expires_during_renewal_outage…` and
  `committed_tool_authority_cancels_real_child_during_outage…`** (real 30-second expiry,
  daemon alive). These now also assert:
  - the quiescence-verified diagnostic, a clean host exit and a free lock;
  - an exact receipt, `settled` and `Proven`.

  Each of the following is refused, and the restored evidence is accepted again:
  - a stale lease and a foreign owner;
  - a tampered `children_absent`, and a receipt for another lease;
  - changed marker bytes (`Unproven`);
  - a moved-away receipt.

  A restarted ordinary grant is still denied (a settled fence is history).
- **`escaped_descendant_keeps_the_fence_hold_until_it_really_exits`.** A detached child
  in its own process group survives the group KILL. The test asserts:
  - `quiescence-unproven`, with no receipt;
  - the lock is held, the diagnosis is `Unproven` and the host stays alive.

  After the test kills the escaped child, the kernel proof follows: receipt, host exit
  and free lock.
- **`refused_cancellation_persists_uncertainty…`** (an unverified KILL from changed owner
  evidence). It now also asserts `quiescence-unproven`, no receipt, a held lock and
  `Unproven`, even after the owner evidence is restored.
- **`retirement::lease_expiry_without_restart_reconciles_retirement_through_fence_receipt`.**
  The daemon stays alive and renewal stops. The test drives the actual application
  retirement through a controller that never owned the host:
  1. Before proof, `durable-runtime-owner-active`, with capacity kept.
  2. A tampered receipt gives `durable-runtime-fence-unproven`, with capacity kept.
  3. With the restored receipt, retirement is reconciled for the lease owner, capacity
     is released and no host retirement receipt is manufactured.
  4. Exact retries return the original result.

### Other tests

- `purge.rs`: the new names are classified, with near misses (bad episode markers,
  receipt sequence `0`/leading zero/overflow, wrong diagnostic pairs).
- `runner/test/process.test.mjs`: the SIGSTOP watchdog test now asserts the receipt (exact
  marker bytes), a clean host exit and the denied restarted grant. Its "contender busy"
  check moved to before KILL.
- `runner/test/fence-settlement.test.mjs` (new): `unblocked` with settled, tampered,
  misnamed, false-children, episode and unparsable receipts.

### Gates

`PATH` had a real Node 24.18.0 first (the Volta shim cannot run the cleared-environment
tests).

| Gate | Result |
| --- | --- |
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --no-fail-fast` | pass: 910 passed, 0 failed, 1 ignored (pre-existing opt-in probe) |
| `durable_supervised_tracer` (within the workspace run) | pass: all 15 scenarios, including the 4 T25 restart scenarios |
| `git diff --check` | pass (after removing one trailing blank line it first flagged) |
| `runner`: `npm run check` / `npm test` | pass / 98 of 98 |

These are local deterministic gates, not release evidence.

## Not claimed

- **No real provider.** All model output is faux; T18 M4 (the managed-daemon credential)
  remains.
- **A real daemon *process* is killed**, but the restarted daemon is the test process's own
  fresh composition, not a separately started `forge daemon` binary. The RPC listener is
  not restarted. A Forgeup-managed `daemon restart` is not exercised.
- **Restart during model generation versus a long tool.** The generation case stops at an
  injected pre-commit boundary with a faux model, not an in-flight provider stream. The
  fenced variants inject a hang into the test worker; the shipped runner exits on EOF
  (`daemon-restart-exit`).
- **A narrow window remains.** If the daemon commits a renewal lease N+1 and dies before
  delivering it, the host's last accepted lease is N. Its receipt then does not match
  Forge's last lease, so the Assignment stays held. This is fail closed: the receipt must
  match the exact last lease.
- **Pre-grant failures.** A fence before any accepted lease (for example an unarmed
  waiting owner or a failed first grant) writes no receipt. Making failed launches
  retirable is T26 (B4).
- **Root cause left in place.** The shipped runner's EOF shutdown ends through an
  unhandled rejection. This was observed and is not changed here.
- Escaped-child absence beyond this host's subreaper, remote effects and hostile
  same-user processes remain outside the boundary, as before.
- Production durable dispatch stays withheld; no rollout claim.
