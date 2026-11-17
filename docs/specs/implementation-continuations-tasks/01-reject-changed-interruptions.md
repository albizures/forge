# T01 — Reject changed-commit interruption outcomes safely

## Authority and input

Parent: [local Spec](../implementation-continuations.md). Read the [shared execution contract](README.md#shared-execution-contract). Project: `factory/forge`. Root Task; no predecessor. Use this plan's freshly authorized root input, not #37's retained work or historical pin. No Forge Record or Assignment exists for this local ID.

Spec AC: **15**, existing-outcome regression aspects of **14**, compatibility/verification aspects of **20–22**.

## Outcome

An agent cannot submit a changed repository commit as an interrupted outcome, receive acceptance, and leave the old expected input authoritative. The public interface rejects that outcome with an actionable diagnostic and preserves the checkout. This slice is independently demoable without waiting for Continuation support.

## Work

- Reproduce #37's sequence using isolated fixtures: attempt bound to commit A, agent commits B, agent submits B as interrupted.
- Reject changed-commit interruption submissions before accepted-result, counter, Assessment, Finding, or work-input effects. Preserve legitimate unchanged-evidence interruption and existing authorization/replay rules.
- Explain that deliberate checkpoint handoffs require explicit Continuation support, while genuinely interrupted retained work requires operator recovery. Do not claim either new operation is already available in this intermediate slice.
- Propagate structured diagnostics through the public Assignment RPC/server/CLI boundary without mixing diagnostics into successful JSON output.
- Preserve B, tracked/untracked files, immutable Base Commit, historical outcomes, and existing safety state. No automatic commit, reset, adoption, or destructive repair.
- Keep substantive failed Implementation and human-input behavior distinct; do not reinterpret outcome prose.

## Observable acceptance and verification

1. The incident reproduction fails against the old behavior and passes after this fix: B is rejected as interrupted, never acknowledged as an accepted interruption, and remains on disk.
2. Work input stays A; no interrupted-retry increment or other partial acceptance effects occur from this rejected submission.
3. Legitimate unchanged interruption remains accepted under its established allowance and retirement rules; substantive Implementation failure is unchanged.
4. Public structured diagnostic identifies the error and safe choices; rejected or foreign capability requests cannot gain recovery authority.
5. Existing accepted historical outcome replay remains stable. Rejected submissions do not rewrite accepted history.

Run focused application/store/server/CLI regression tests, then the shared root gates. Report actual commands, results, unrun gates, commit, and whether compatibility changes were required. Document the new rejection in the affected outcome contract.

## Implementation evidence

- Status: complete (local source implementation only).
- Input commit: `b1f900a26fb2c3f74245e0f10e94b1a970732325` in the user-authorized `forge/` checkout; no managed root or Assignment was prepared.
- Output commit: `78eecb352fe3353cd26e22074222163845f6c061`.
- Application policy rejects changed interruption evidence as
  `interrupted-evidence-changed`. Persistence invokes it after authorization,
  replay and freshness checks, before drift holds or acceptance effects. RPC
  preserves its structured category; CLI supplies static, secret-safe guidance
  on stderr with no successful JSON. Both explain that Continuation and the new
  recovery operation are not yet available.
- Real-Git Assignment RPC fixtures reproduce A → committed B → interrupted B,
  reject clean and dirty submissions, and assert retained HEAD, tracked/untracked
  content, work input, counters, Check history, Findings, updates, Worktree Claim
  (including Base Commit), ownership and holds are unchanged. Foreign/invalid
  credentials remain denied before the new diagnostic. The same fixture then
  accepts legitimate completion or human-input outcomes.
- Additional tests cover application disposition policy, atomic Manual rejection,
  exact/conflicting replay, isolated historical changed-interruption replay after
  reopening, unchanged dispatched Implementation interruption with one retry
  increment and safe same-Check replacement, and CLI guidance/redaction.
- Red/green evidence: with only the persistence guard temporarily removed,
  `cargo test -p forge-store --test manual_outcomes changed_interruption_is_atomic_and_unchanged_interruption_replays`
  failed because the old path accepted B while retaining A. The real-Git
  `cargo test -p forge-server check_bound_rpc_context_outcome_and_normal_dispatch_share_same_owner`
  likewise failed by returning an accepted interruption. Both pass with the guard
  restored; no live incident resources were used.
- Focused verification passed:
  - `cargo test -p forge-store --test manual_outcomes --test task_check_continuation`
  - `cargo test -p forge-server check_bound_rpc_context_outcome_and_normal_dispatch_share_same_owner`
  - `cargo test -p forge-server task_inspection_after_accepted_human_input_and_clean_retirement`
  - `cargo test -p forge-cli --test assignment_outcomes`
- Final root gates passed: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (535 passed, 0 failed, 1 existing ignored), and
  `git diff --check`. Workspace testing includes the final added unchanged
  Implementation and application tests. An initial server module-name filter
  selected zero tests; the explicit function filters above and full suite ran
  the actual tests.
- Compatibility: intentional validation tightening for new submissions, additive
  RPC diagnostic category, unchanged request/result/storage shapes. No schema
  revision, upgrade, backfill or rewriting accepted history was required.
- Self-review against T01 and `CODING_STANDARDS.md`: no remaining findings.
- Blockers: none. No shipped skills changed, so skill parser/portability gates
  were not applicable. One existing ignored workspace test was not run. No live
  activation, profile changes, migration, dispatch or recovery was performed;
  local verification is not release readiness. T02–T07 remain unchecked.

## Routing hints

`crates/app/src/outcomes.rs`, `crates/store/src/outcomes.rs`, public Assignment outcome adapters, outcome/review tests, `docs/assignment-context.md`, and `docs/dispatch.md`. These are routing hints, not a requirement to extend unrelated `lib.rs` responsibilities.

## Boundaries

No Continuation acceptance yet; no automatic recovery or budget redesign. Do not attempt live reproduction on #37. The remaining shared boundaries apply.
