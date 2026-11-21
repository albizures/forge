# T26 — Make failed durable launches reconcilable and retirable

Corrective Task for T18 second-pass **B4**
([review](../pi-durable-t18/README.md#b4--blocker-a-launch-that-fails-after-its-claim-can-never-be-retired)).
Responsible deliverable: T23. Controlled deliverable: new durable launches stay **withheld
by default** behind the T16 release gate. No command that enables durable launch was added.

## Problem

- **Claim and reservation were two transactions.** `begin_agent_provision` committed the
  `provision-intent` claim, then `reserve` re-authorized in a new transaction. A close, a
  generation bump or a daemon restart between them left a permanent claim with no runtime
  row. Provision then refused (`durable-launch-claimed`), observe and recover were
  Transient, and `Adapter::retire` failed on the missing runtime.
- **Failures after the reservation stranded the Assignment.** A failed runtime write, a
  refused launch intent, a spawn failure, invalid `owned` evidence and a failed bind each
  retained only a generic failure. After `owned` or bind failures the `Child` was dropped
  unreaped, orphaning the host. Retirement then found neither an owned entry nor a revoked
  lease and returned `uncertain` forever. Retirement also refused every `Pending`
  admission (`durable-retirement-admission-unknown`) and every owner without a committed
  grant (`durable-retirement-owner-unverified`).

## Delivered

### Atomic claim (TX-1, store)

`durable_execution::Repository::claim` inserts the `provision-intent` marker, the
`durable_assignment_runtimes` row and the `durable-runtime` capacity resource in **one**
immediate transaction, after the Assignment execution gate. A refused precondition
(stale generation, closed Assignment, conflicting identity) rolls all of them back. An
existing claim or a closed Assignment returns `None` and writes nothing. The launcher mints
the Launch first and calls `claim` instead of `begin_agent_provision` + `reserve`.

A claim with no runtime written before T26 (the legacy two-step) is discharged by
`reconcile_unreserved_claim` only once the Assignment is closed. At that point neither
`claim` nor `reserve` can add a runtime, so the claim launched nothing. `Adapter::retire`
uses this path when no runtime row exists.

### Specific diagnostics (RUST-3)

`launch_failure::Failure` (app policy) names each point; the store retains one
`durable-launch-failure` resource per point. Status lists the code under
`retirement_blockers` and `launch-failed` under `uncertainty`:

| Code | Point |
| --- | --- |
| `durable-launch-runtime-input-failed` | runtime directory or `check-runtime.json` |
| `durable-launch-intent-refused` | `begin_agent_launch` refused |
| `durable-launch-spawn-failed` | host spawn |
| `durable-launch-owned-invalid` | missing/invalid `owned` evidence |
| `durable-launch-bind-failed` | bind through the effect gate (or driver runtime) |
| `durable-launch-grant-failed` | first grant |

### No live `Child` is dropped (server)

- **Before any grant** (`owned`, bind, driver setup), the driver records the failure and
  `abandon`s: it closes the private pipe, drops its stop channel (so retirement never waits
  on it) and **blocks in `child.wait()`**. The old 30-second `reap` deadline, which could
  drop a live host, is gone from every exit path.
- **First-grant failure** keeps the pipe. Retirement then stops the driver and
  `retirement::Controller::register` receives the original Child and pipe.
- **A rejected handoff** never drops a live host. `register` closes the pipe and reaps the
  Child on a dedicated thread (`reap_detached`).

### Host: pre-grant issuer loss and the ungranted receipt (runner-host)

- **Fence on closed pipe.** Before any accepted grant, the private pipe is the only channel
  a grant could arrive on. On stdin EOF with no accepted lease and no shutdown requested,
  the host fences with `runner-issuer-lost`. Its `watchdog.json` names no `last_lease`.
- **Receipt.** `quiesce_after_fence` now proves a fence that has no lease the same way T25
  proves a leased one:
  - the worker was positively reaped;
  - the kernel reports ECHILD;
  - the captured storage is unchanged.

  It then writes `ungranted-receipt-<host pid>-<host start>.json` (`fence::UngrantedReceipt`)
  and exits. The receipt binds the marker's exact bytes, and that marker must record no
  lease. It also binds the owner, which `fence::ungranted_receipt` checks against
  `owner.json` (host and worker identity, boot ID and lock identity).
- **Other pre-grant faults.** A worker that exits before any grant, or another pre-grant
  fault, gets the same proof. This used to hold the lock indefinitely; see the T04 test
  change below.
- **Settled.** `fence::settled` accepts either receipt kind for a marker.
- **Runner check.** The runner's defense-in-depth `unblocked` check accepts an ungranted
  receipt only for a marker whose `last_lease` is `null`.
- **Seal** (`seal.rs`). `seal` creates `owner.lock` only if it is absent and takes it
  exclusively. It refuses on `owner.json` or a pending owner write, then writes the
  protected `launch-sealed.json`. `supervisor::run` refuses a sealed directory after taking
  the lock and before recording ownership or spawning its worker.
- **Purge** recognizes `ungranted-receipt-*` (receipt rank) and `launch-sealed.json`
  (marker rank). Near misses are still refused.

### Forge: reconciling a never-granted launch (app policy, store, server)

`launch_failure::validate` requires all of the following:

- a closed Assignment;
- a `Pending` admission (never sent: `begin_send` commits `Unknown` first);
- no committed grant;
- no begun host retirement.

The `never-launched` proof additionally requires that no launch intent was committed.
`Controller::retire` takes this path when it holds no live handoff for the Assignment and
there is neither a committed grant nor a begun retirement. It uses the strongest available
proof:

| Proof | Evidence |
| --- | --- |
| `never-launched` | No launch intent: the store refuses one for a closed Assignment, so no production launch spawned a host. Defense in depth: prepared storage must also **seal**, otherwise the code is `durable-launch-ungranted-unproven` (or `-owner-active` while a host holds it). |
| `ungranted-quiescence` | Under `ExclusiveInspection`: `owner.json` exists, the exact ungranted receipt verifies and every marker is settled. |
| `never-owned` | The directory seals: no owner evidence, lock free (for example a failed spawn after the launch intent). |

**Journal (schema revision 25).** `durable_launch_reconciliations(assignment_id, proof,
reconciled_at_millis)` journals the proof. Rows are immutable and retained, and an exact
retry keeps the original row and resource timestamps. The `durable_runtime_capacity_retained`
trigger now releases `durable-runtime` with either a reconciled host retirement or this row,
for a closed Assignment. Without it the trigger still aborts, which is tested.

**Explicit offline upgrade.** The 24 → 25 upgrade adds the empty table and replaces the
trigger. A stamp-failure rollback test and the frozen upgrade ladder cover it.

**Related changes:**

- `retirement::validate_begin` now accepts `Pending`; `Unknown` is still refused.
- `validate_owner` accepts a requested owner without a recorded one only when the launch is
  ungranted. That owner comes only from the directly spawned Child, and only the host's
  exact receipt can reconcile it.
- A begun retirement for such a host whose request was lost with its daemon also accepts
  that host's ungranted receipt for the operation's owner.
- `reconciles_resource` adds `provision-intent` and `durable-launch-failure`.
- Retirement effect inspection for a `Pending` admission with no runtime database uses the
  same `require_fresh` check that gates a first grant.
- Purge eligibility accepts a `Pending` admission once every resource, `durable-runtime`
  included, is retired.

Without proof, retirement reports one of three codes:

- `durable-runtime-owner-active`;
- `durable-runtime-fence-unproven`;
- `durable-launch-ungranted-unproven`: a host owned the directory before any grant but did
  not prove quiescence, for example because it was killed.

A new test fixed a bug before commit. The diagnosis probed the inspection lock while still
holding it, so it always reported `owner-active`.

### Test seam

`Launcher::inject_fault(Fault)` (`#[doc(hidden)]`) forces one failure point. It can only
make a launch fail or stall the driver (`Stall`, a hung daemon after `owned`). It never
enables, skips or advances anything. Production composition never calls it.

## Evidence

### Class (a): store and policy

- `crates/app/src/durable_execution/launch_failure.rs`, 3 tests:
  - codes round-trip;
  - only revoked, ungranted, unsent launches reconcile, and `never-launched` refuses a
    committed intent;
  - proof discharges only launch resources.
- `crates/store/src/durable_execution/launch_failure_tests.rs`, 5 tests:
  - **Atomic claim.** A stale generation leaves no claim. A closed Assignment writes
    nothing. A second claim with fresh identities never replaces the first.
  - **Legacy claim.** Refused while open. Once closed, `reserve` is refused and the claim is
    discharged. The retry is exact after reopen, and a reserved runtime is refused.
  - **Journaled reconciliation.** Refused while open, and the trigger blocks a direct
    capacity release. After closure it reconciles, the exact retry keeps every timestamp,
    and journal rows cannot be deleted.
  - **Host proof required.** A committed launch intent refuses `never-launched`. A committed
    grant refuses `never-owned`/`ungranted-quiescence`, and capacity stays reserved. A sent
    admission is refused.
  - **Purge.** Purge is refused until the never-sent launch is reconciled, then allowed.
- `upgrade_tests.rs`: 24 → 25 expansion is empty and a denied stamp preserves revision 24.
  The fixture ladder in `tests/storage_upgrades.rs` gains revision 25.
- `runner-host` unit tests: `seal` is idempotent (lock inode unchanged on retry) and
  refuses owner evidence and a held lock. Purge classifies the new names and refuses near
  misses.
- `runner/test/fence-settlement.test.mjs`: an ungranted receipt settles only its own
  owner-named, lease-free marker.

### Class (b): real host, Node runner, SQLite, Git and faux provider

Host and Forge tests (`crates/runner-host/tests/`):

- **`lost_issuer_before_any_grant_proves_ungranted_quiescence_and_releases_ownership`.**
  A real host whose pipe closes before any grant:
  - fences with `runner-issuer-lost` and no lease;
  - its worker's `/proc` entry is gone;
  - the receipt verifies, settles and diagnoses `Proven`;
  - `seal` refuses the owned directory;
  - tampered marker bytes or owner evidence are refused, and the restored evidence is
    accepted again.
- **`sealed_runtime_directory_refuses_every_later_host`.** The real host exits non-zero and
  writes no `owner.json`.
- **`retirement::never_granted_launch_retires_only_with_the_hosts_ungranted_receipt`.**
  Through the real application:
  - a killed host, with its worker gone, refuses with `durable-launch-ungranted-unproven`
    and capacity stays reserved;
  - a host that proved quiescence retires, releasing capacity, with an exact retry.
- **`retirement::unreserved_launch_claim_is_retired_through_the_adapter`.** The legacy claim
  is discharged and the Dispatch Hold is kept.

Ten new production-composition tracer scenarios (`crates/cli/tests/durable_supervised_tracer.rs`,
`support/supervised_launch_fault.rs`). Each scenario runs dispatch → injected failure → CLI
`assignment-status` shows the specific code and `launch-failed` → a second dispatch does not
respawn → CLI `assignment-retire` releases capacity with every resource retired → an exact
retry returns identical state → `resume` → CLI `assignment-purge-runtime` removes the
directory:

| Scenario | Proof used | Lifecycle artifact |
| --- | --- | --- |
| `launch-fault-runtime-input` | never-launched (+ seal) | `launch-sealed.json` |
| `launch-fault-launch-intent` | never-launched (+ seal) | `launch-sealed.json` |
| `launch-fault-spawn` | never-owned | `launch-sealed.json` |
| `launch-fault-owned` | ungranted-quiescence; host reaped (no `/proc` entry) | `ungranted-receipt-*` |
| `launch-fault-bind` | ungranted-quiescence; host reaped | `ungranted-receipt-*` |
| `launch-fault-grant` | handoff of original Child/pipe → host retirement receipt | `retirement-receipt.json` |
| `launch-fault-restart-runtime-input` | daemon process SIGKILLed after the failure; never-launched | `launch-sealed.json` |
| `launch-fault-restart-spawn` | daemon SIGKILLed after the failure; never-owned | `launch-sealed.json` |
| `launch-fault-restart-stall` | daemon SIGKILLed while its host waits for a first grant; orphaned host proves itself and is reaped by init | `ungranted-receipt-*` |
| `launch-fault-restart-grant` | daemon SIGKILLed after a failed first grant | `ungranted-receipt-*` |

The `restart` modes launch from a separate daemon process (`--daemon`). The test process's
own composition never launched the Assignment, and asserts it has no driver for it.

Existing scenarios (duplicate and fresh-composition provision, `revoked`, every T25 restart
scenario, `packaged`) all still pass on the new claim.

### Changed existing tests

- **`authority.rs::exited_worker_is_not_permission_and_ownership_ends_only_with_ungranted_proof`**
  (formerly `…even_while_supervisor_retains_lock`). A worker that exits before any grant no
  longer holds ownership forever. The host exits only after the exact ungranted receipt, and
  a free lock always comes with that receipt and a retained marker. Capture still fails and
  the evidence is unchanged.
- **`retirement::unknown_launch_and_missing_supervisor_keep_revocation_capacity_and_hold`**
  is unchanged and still passes. Its fixture spawns a host without a launch intent, which
  production never does. It first failed when `never-launched` trusted the store alone;
  `never-launched` now also requires the prepared storage to seal, so a live host blocks it.
- `upgrade_tests.rs` and `tests/storage_upgrades.rs` expect revision 25.

### Gates

Run with a real Node 24.18.0 first on `PATH`:

| Gate | Result |
| --- | --- |
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --no-fail-fast` | pass: 925 passed, 0 failed, 1 ignored (pre-existing opt-in probe) |
| `durable_supervised_tracer` | pass: 25 scenarios (15 earlier + 10 T26) |
| `git diff --check` | pass |
| `runner`: `npm run check` / `npm test` | pass / 99 of 99 |

**Transient failure.** One earlier full workspace run failed
`committed_model_authority_expires_during_renewal_outage_with_retained_capacity`: its
stderr closed before an awaited diagnostic. This is the T05/T25 real 30-second leased-expiry
test, on a path T26 does not change. T26 only acts on EOF *without* an accepted lease.

- The test passed alone twice.
- The full `authorization_session` suite passed three consecutive times.
- The final complete workspace run passed.

It is not claimed fixed. Its `diagnostic` helper now names the awaited code on timeout.

## Not claimed

- No real provider (class c).
- No Forgeup-managed daemon (M4 remains).
- No real separately started daemon after a restart. "Restart" is a SIGKILLed launching
  daemon process plus a different composition in the test process, as in T25.
- **Defense in depth for `never-launched`.** It relies on the production invariant that a
  host is spawned only after its committed launch intent. The added seal requirement blocks
  a host that exists anyway, but it cannot catch a host that a non-production path forked
  and that has not yet taken its lock. Such a host still refuses a sealed directory.
- **An `Unknown` admission is out of scope.** It is still refused by retirement and purge
  and needs its launch observed. B4 concerns failures before admission was sent.
- A host killed by SIGKILL before any grant cannot prove quiescence. It remains visibly
  blocked (`durable-launch-ungranted-unproven`) by design.
- T25's unhandled rejection in the shipped runner's EOF shutdown is still only recorded,
  not changed.

Production durable dispatch stays withheld; T18 re-reviews the corrected snapshot (third pass).

## Errata (T18 third pass)

Some statements above were corrected after review; see [third-pass corrections](../pi-durable-t18/third-pass-corrections.md#errata-to-earlier-evidence).
