# T10 — Same-Assignment recovery completion evidence

## Status: controlled T10 deliverable complete; rollout disabled

T10 connects the earlier read-only, reservation and authorization increments
into a full controlled path for same-Assignment recovery. Production durable
dispatch and automatic recovery stay **disabled** in the default composition. The
daemon recovery controller exists only when it is given explicit, trusted, matched
components. There is no operator CLI/RPC recovery operation. Packaging, scheduling
rollout and real-provider verification are T15/T16/T19.

The controlled path:

1. The original supervisor writes a protected exact-grant **crash receipt** after
   it reaps its worker directly and observes kernel ECHILD (`runner-host/src/crash.rs`).
   It can also write a claim-bound quiescence receipt. A free flock, a dead PID,
   an old lease or a restored file never substitutes for this proof. Receipts are
   kept per lease sequence.
2. The daemon recovery controller (`server/src/durable_authorization/recovery.rs`)
   preflights the original proof before it claims or spawns anything. It then
   reserves the content-bound Candidate claim and spawns a new waiting owner
   directly. Under that owner's lock it rechecks canonical Git/index/content,
   the original pending input and all effects.
3. The protected handoff persists immutable claim-bound proof
   (`runner-host/src/recovery_proof.rs`) and prepares the unarmed consumer. It
   rechecks runtime/effects, commits a fresh owner-bound recovery lease and
   delivers the exact private envelope. The lease is committed before delivery.
4. The writable recovery (`runner/src/resume-runtime.ts`, driven by
   `runner/src/recovery-consumer.ts`) bootstraps again through the original
   capability. **Correction (T22):** this evidence previously named
   `runner/src/check-recovery.ts` as the writable Check runner. That file was
   never executed and nothing wrote its `check-runtime.json` input; T10's
   recovery tests ran test-only compositions. T22 removed it. The shipped
   recovery entry is now `runner/src/check-runner.ts` (packaged as
   `check-entry.ts`), reading the protected input written by
   `forge_runner_host::check_runtime::write`; see the
   [T22 evidence](../pi-durable-t22/README.md). It revalidates cwd and the original admission and resumes **that
   submission**. It creates no new root, input, `continue`, Check attempt or
   Continuation. Model retries stay disabled, so an in-flight model request may
   settle unanswered. Arbitrary commands are never replayed.
5. A read-only answer inspector (`runner/src/inspect-answer.ts`, typed wire
   `rpc/src/durable_execution/answer_inspection.rs`) observes committed answers.
   A missing Outcome keeps a hold and reserved capacity and cannot create a
   successor (`app/src/durable_execution/reconciliation.rs`).
6. Each recovery is a separate immutable claim/grant episode with fresh issuer
   authority. Only a newly proven crash of the current exact grant can start
   another episode. Neither the daemon nor historical replay adopts a pending
   claim or uncertain delivery.
7. Operator retirement can reconcile an exact original crash receipt only after
   revocation, under exclusive inspection and committed-effect checks. It never
   writes or repairs a host retirement receipt, and an unrelated later host-fault
   marker still blocks it.

A deliberate accepted Implementation Continuation and an independent Review are
distinct from recovery. Each gets a fresh conversation and store, plus the
Forge-approved checkpoint or exact accepted output, after retirement.

## Corrections found during this work

- **Herdr-only handoff gate.** Durable Check handoff required Herdr agent-name
  metadata. `store::durable_execution::handoff_admitted` now requires known,
  accepted original durable admission for the exact attempt and profile revision.
  Regression: `durable_check_handoff_requires_known_original_admission_without_herdr_metadata`.
- **Exit-observation race.** A direct Child reap could race the current ownership
  sample. That sample may now spend at most 100 ms waiting for a positive reap,
  with captured storage revalidated. A previously failed sample is never revived.
  Regression: `reaped_child_can_complete_current_sample_but_never_restore_a_previous_fault`.
- **Historical crash proof versus later faults.** Crash-receipt retirement now
  refuses when a later `watchdog.json` or `retirement-intent.json` marker exists.
  Regression: `historical_crash_proof_cannot_waive_a_later_host_fault_during_retirement`.

## Acceptance-check evidence

| T10 acceptance check | Evidence |
| --- | --- |
| Safe restart resumes the same attempt/conversation with no duplicate admission | Supervised scenario `T07 supervised recovered`: real bootstrap, Bash and authenticated Outcome after interruption. Root and admission ID are unchanged, lease sequence is 2, no secret appears in events, Outcome is accepted. `recovery_after_owned_worker_death_and_store_restart_resumes_original_attempt` (real owned process death plus store restart). Node: `authorized writable recovery resumes the exact original admission/conversation without new input`. |
| Revocation, expired authority, competing ownership, stale bindings and uncertain effects block all scheduling | `recovery_controller_refuses_missing_foreign_and_revoked_proof_before_spawning_or_claiming`. `trusted_handoff_denies_changed_protected_proof_or_revocation_before_delivery`. `surviving_descendant_prevents_crash_receipt_and_preserves_capacity`. Node: writable recovery denies ordinary, unarmed, expired, bootstrap-failed and wrong-worktree before scheduling. Earlier increments cover effects, claims and predecessor validation. |
| Daemon and runner restart order cannot bypass gates | `daemon_reconciliation_recovers_once_and_keeps_missing_outcome_as_intervention`. `daemon_reconciliation_handles_a_second_proven_crash_without_new_attempt_or_input`. `trusted_handoff_persists_proof_and_commits_before_executable_delivery` (commit before delivery). Ordinary-restart denial and unarmed-owner fencing come from earlier increments. |
| Continuation and independent Review successors receive fresh conversations and exact approved context | Supervised scenario `T10 supervised continuation/review chain`: Continuation → Implementation → independent Review, each with a fresh conversation/store after retirement. |
| No dirty work is silently repaired, adopted or discarded | `recovery_progress_combines_real_proofs_without_adopting_or_changing_work`. `recovery_progress_failure_terminally_fences_even_after_original_bytes_are_restored`. `crash_receipt_retirement_requires_revocation_and_never_repairs_a_host_receipt`. |

Other evidence: Node `interrupted in-flight model remains unanswered with retries
disabled, never a synthetic continuation`, plus the two answer-wire tests for
committed-identity-only observation and rejection of foreign or oversized data.

Domain records: the **Assignment Execution Recovery** glossary entry is in
`crates/CONTEXT.md`. The recovery distinction is complete in actual ADR-0012; the
Spec's provisional ADR-0013 number was not used. Operator-facing behavior is in
`docs/assignment-recovery.md`.

## Actual verification (2026-11-20)

```sh
cargo fmt --check
git diff --check
cargo clippy --workspace --all-targets -- -D warnings
PATH="$HOME/.volta/tools/image/node/24.18.0/bin:$PATH" cargo test --workspace --no-fail-fast
(cd runner && npm run check && npm test)
```

- Format, diff and clippy checks pass.
- **824 Rust harness tests passed, 0 failed, 1 pre-existing opt-in ignored.**
  All **9** custom-harness supervised scenarios passed: bootstrap-denied,
  bootstrap-uncertain, revoked, missing, rejected, accepted, retired, recovered,
  and the T10 Continuation/Review chain.
- Runner typecheck passes and **85 Node tests pass**.

Environment note: the first `cargo test --workspace` run failed all five tests in
`durable_check_bootstrap`. That test's `node()` helper canonicalizes the first
`node` on `PATH`. On a Volta install, that resolves to `volta-shim`, which refuses
to run directly under a cleared environment. That file is unchanged by T10. With
a real Node 24 executable first on `PATH`, the full suite passes. The helper is
not fixed here. A later Task can adopt the `process.execPath` probe already used
in `runner-host/tests/authorization_session.rs`.

## Limits

- The model is a controlled fake. Real-provider evidence belongs to T19.
- This is trusted-local-host Linux evidence, not hostile same-user containment or
  universal process-absence proof. External writers can still race observations.
- Loss of the original supervisor before proof, prior host uncertainty, surviving
  descendants and unknown effects remain intervention states. There is no
  automatic adoption, repair or blind retry.
- Historical transient failures recorded in earlier increments are not claimed
  as resolved.
