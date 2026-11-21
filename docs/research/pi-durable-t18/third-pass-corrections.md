# Corrections for T18 third-pass findings

The third pass reviewed `943051c` with two independent reviewers: findings F1–F7 for the
T25/T26 code, plus residual M2′ and M6 gaps. Each correction below belongs to the
deliverable responsible for the finding:
- T25 for F1 and F3;
- T26 for F2, F5 and F6;
- T17 for the M2′ residuals;
- T12/T23 for the M6 gaps and RUST-3.

No Task was added. The corrected snapshot is re-reviewed before T19 consumes it.

## F1: a lease committed but never accepted stranded the Assignment

Forge commits a lease before delivering it. When the delivery or post-commit check failed,
or the daemon died, Forge's last lease ran ahead of the host's. Retirement and recovery
demanded proof for exactly Forge's last lease, so the case stranded the Assignment.

**Fix.** A new function, `forge_runner_host::quiescence::predecessor_proof`, accepts the
original host's own proof that nothing of the last lease's owner still runs. It is used
both by recovery (`waiting_receipt`) and by retirement (`reconcile_proof`). In order, it
accepts:
1. exact crash or fence proof for that lease;
2. that **same owner's** crash or fence proof for an **earlier lease of the same Launch**
   (`crash::receipt_for_owner`, `fence::receipt_for_owner`);
3. that owner's ungranted receipt.

**Why this is safe.** The fence marker records the last lease the host accepted, under the
same lock that sets the fault, and a fenced host's `accept_lease` refuses everything after.
Crash proof is written only after a direct reap plus ECHILD. The worker receives leases
only through the host. Effect inspection, the admission rules and the capacity rules are
unchanged.

**Tests.** Two class (b) real-host tests in `authorization_session/retirement.rs`, each
retiring through the application:
- `committed_but_undelivered_renewal_retires_from_the_earlier_lease_fence`: lease 2 is
  committed only in Forge, and the host proves lease 1. Despite its name, this test
  takes the *crash* path; fence-path proof for an earlier lease is covered by the unit
  tests below;
- `committed_but_undelivered_first_grant_retires_from_the_ungranted_receipt`: lease 1 is
  committed only in Forge, and the host loses its pipe and writes its ungranted receipt.
  Its exact retry is also checked.

In both, the pipe loss stands in for the daemon dying.

**Also fixed by these tests.** Retirement required configured inspection components even
when no runtime store existed. Components are now resolved only when there is something to
inspect.

**F4.** A Proven fence with a free lock but no matching proof now reports
`durable-runtime-proof-unmatched` instead of the generic `durable-retirement-unverified`.

**Delta re-review corrections (after `26dddfd`).**

- **Ungranted lookup by owner.** The ungranted lookup is now keyed by the lease owner
  (`fence::ungranted_receipt_for`): its name, owner and lock identity, then the marker
  binding. It no longer goes through `owner.json`, which a later recovery host
  overwrites. Before this, an automatic recovery after an undelivered recovery grant
  could re-strand the Assignment.
- **Negative unit tests.** `runner-host/src/quiescence.rs::predecessor_tests` (class (a),
  real protected files):
  - **fence:** rejects another owner, another Launch, an earlier lease than the proof, a
    tampered receipt and changed marker bytes;
  - **crash:** rejects another owner, another Launch and an earlier lease;
  - **ungranted:** the overwritten-`owner.json` regression (fails against the previous
    lookup); a receipt copied under another owner's name (owner equality); another lock
    identity; and a receipt whose exact marker bytes name a lease (`last_lease`).

  The fence and crash cases cover `receipt_for_owner` logic that this delta did not
  change. `reconcile_ungranted_owner` also uses the owner-keyed lookup now; the
  no-lease retirement path still reads `owner.json`, which is correct there because no
  later host can exist without a grant.

## F2: the recovery spawn path dropped live `Child`ren

Everything after the host spawn in `recovery::Worker::drive` now runs in
`supervise_spawned` and borrows the original Child:
- an error closes the private pipe and reaps the host;
- a crash waits for the host's own exit;
- a retirement handoff goes through `register`, which reaps the host if it rejects it.

## F3: a lease expiry without a restart blocked recovery

After a fence, the launch driver used to stay `Ended` for the rest of the daemon's
lifetime. Now:
- once the driver's host exits and is reaped, the driver state becomes `Reaped`;
- `Adapter::recover` releases a `Crashed` or `Reaped` slot (`release_finished`) and hands
  over to the proof-based recovery controller.

**Not covered by a new test.** No new test drives this through the production driver
within one daemon lifetime. The existing class (b) tests cover the proof acceptance.

## F5: two smaller issues

- **Poisoned `slots` mutex after the spawn.** The host is now reaped through `abandon`,
  and the failure is recorded.
- **Stop/abandon race.** Left as is: only one retirement attempt fails, and the retry
  succeeds.

## F6: test-only fault injection in production code

`Launcher::inject_fault` is now compiled only with the new `forge-server` Cargo feature
`test-faults`. The CLI integration tests enable that feature as a dev-dependency, so
production builds do not contain it.

## F7: merged doc comment

The doc comments on `reconcile_recorded` and `reconcile_ungranted_owner` are split.

## RUST-3

- **Renewal clock.** An unreadable boot clock during renewal now fences and retains
  `authority-unavailable` instead of silently letting the lease lapse.
- **Follow-up store failures.** `send_followups` and `record_followup_event` now log a
  specific code (`durable-followup-*`) instead of discarding the store error. The state
  stays `unknown` or undelivered and is retried.

## M2′ residuals (docs)

- `durable-execution.md`: the key-bytes claim is qualified. A same-user tool can read the
  key file, and its output is stored unredacted.
- `durable-execution.md` and `assignment-recovery.md`: follow-up delivery covers the
  launching session too. The request ID deduplicates; the hash is integrity only.
  Recovery is composed. The follow-up-after-settled limitation is now documented.
- `assignment-recovery.md`: the retirement wording covers T25/T26 receipts, and the
  recovery-reservation sentence is corrected.
- `crates/store/README.md`: the verifier and controller exist, recovery runs as episodes,
  and retirement exists.
- `docs/development.md`: describes the key-file scope.
- `runner/README.md`: the history range now reads T04–T26, and the T10 heading is
  corrected.

## M6 gaps

The test now asserts two things:
- whenever only session 1's acceptance exists, Forge already holds `unknown`, and this was
  observed in practice;
- exactly two recovery preparations ran.

Querying the runtime store directly for a single `followup:` request is not done. The
runner's `replayed` flag and the equal admission IDs stand in for it.

## Errata to earlier evidence

- **T25 README, "specific codes replace the generic":** that was not true for a Proven but
  unmatched fence. It is now true (`durable-runtime-proof-unmatched`).
- **T25 README, "narrow window":** the committed-but-undelivered case was wider and
  stranded the Assignment. Fixed by F1.
- **T25 README, "lease expiry with the daemon alive reaches the same paths":** this held
  only for retirement through a non-owning controller. Fixed by F3, without a new
  production-driver test.
- **T26 README, "No live `Child` is dropped":** that was false for `recovery.rs` and for a
  poisoned `slots` mutex. Fixed by F2 and F5.
- **T26 README, the `launch-fault-grant` scenarios:** they inject the fault *before* the
  lease commit, so they cover a grant that was never committed. The post-commit case is
  covered by the F1 host tests.
- **T26 README, verification:** the Task's requested class (a) fault tests in
  `launch_tests.rs` were not added. The coverage is in class (b) tracer and host tests and
  class (a) store and policy tests.

## Gates

| Gate | Result |
| --- | --- |
| fmt, clippy (`-D warnings`), `git diff --check` | pass |
| `cargo test --workspace --no-fail-fast` | 931 passed, 0 failed, 1 ignored (pre-existing opt-in), after the delta re-review corrections (928 before) |
| `durable_supervised_tracer` | 25 scenarios pass |
| runner `npm run check` / `npm test` | pass / 99 of 99 |

## Final delta (after `7262396`)

The final small delta re-review found no blocker or major issue. It found three minors,
all corrected:
- two negative tests asserted less than they claimed;
- `reconcile_ungranted_owner` still used the `owner.json` lookup;
- the coverage and doc wording overstated what was tested.

**Two pre-T26 host tests made order-independent.** A full run then exposed that
`revoked_assignment_and_already_stopped_driver_never_deliver_initial_permission` (and the
equivalent wait in `failed_binding_…`) assumed one pre-grant path. Since T26, losing the
pipe before any grant races two valid host paths:
- the worker exits on EOF first (`runner-worker-exited…`);
- the host fences on the lost issuer first (`runner-issuer-lost`).

Both tests now accept either diagnostic. The revoked test additionally asserts a clean
host exit with its ungranted receipt.

**Gates on the final tree.** fmt, clippy and diff-check pass. Two consecutive complete
`cargo test --workspace --no-fail-fast` runs both gave 931 passed, 0 failed, 1 ignored,
with all 25 tracer scenarios passing.
