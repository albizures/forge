# Safe Implementation Continuations — full-feature verification

## Scope and provenance

This report verifies the [local Spec](implementation-continuations.md), AC1–22,
for [T07](implementation-continuations-tasks/07-verify-full-feature.md).
It is local implementation evidence, not release or live recovery authorization.

- Actual T01 input: `b1f900a26fb2c3f74245e0f10e94b1a970732325`.
  T01's evidence records a user-authorized checkout, **not** a prepared managed
  root or Assignment. The parent of T01's implementation commit confirms this input.
- T01 implementation: `78eecb352fe3353cd26e22074222163845f6c061`.
- T02 implementation: `18dedd6423f76f541410d6059469f47d5bbedbe9`.
- T03 implementation: `41d4054f2f6a0e4631c30eb2739e318590fbed95`.
- T04 implementation: `44e019ae0d9b4af58a2a0efff68a354b7c193baf`.
- T05 implementation: `5f5ac6ec73369441a95439fefaffbf03f7cb018b`.
- T06 implementation: `bdc4c5093a272a4637a5b30787c44ebfa8415a1b`.
- T06 verified handoff / T07 input: `06ea109f531897634e9d16ebac20e3393732d24b`.
- Inherited review range, explicitly including implementation and evidence commits:
  `b1f900a26fb2c3f74245e0f10e94b1a970732325..06ea109f531897634e9d16ebac20e3393732d24b`.

T07 correction commits:
`4d3927095f26a7c06f7765a1332cc266bd7a4754` (safety fixes, regressions,
schema 19 and documentation) and
`04e867eacb416335d46c55a81aeb308df6640fcf` (validated Stack fixture).
The final executable feature review range is therefore
`b1f900a26fb2c3f74245e0f10e94b1a970732325..04e867eacb416335d46c55a81aeb308df6640fcf`,
plus this report and T07 completion evidence.

The checkout was clean at entry. No incident commit, moving branch name, or
Task-local diff substitutes for those ranges. The report's output commit is recorded
in its Task file after committing it; it is not self-referentially embedded here. T01–T06 evidence files were read alongside the Spec and runtime contracts.

## Reproduce the verification

Run from the Forge repository root (no live daemon/provider configuration needed):

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
cargo test -p forge-app --test implementation_continuations_documentation
cargo test -p forge-cli --bin forge implementation_continuations
cargo test -p forge-cli --bin forge operator_guide_commands_parse_without_execution
cargo test -p forge-cli --bin forge documented_skill
python3 -m unittest discover -s tests -p 'test_skill_*.py'
```

Initial inherited-range root gates passed: formatting, workspace/all-target Clippy
with warnings as errors, and **559 passed, 0 failed, 1 ignored** across 72 workspace test suites
(including zero-test/doc-test suites). Tests ran with default parallelism, not a
serial workaround. `git diff --check` passed. Focused runs passed: profile/JSON/
glossary tests (3), profile inline CLI commands (1), operator-guide commands (1),
documented-skill parser tests (2), Python skill portability tests (4).
An initial `documented_operator_guide` filter selected zero tests; that is **not**
verification evidence. The corrected explicit operator-guide filter above ran one.
No shipped skill changed in the feature range; the skill gates passed nonetheless.

The existing ignored real-Pi loopback runtime test in
`crates/server/src/herdr/pi_flags_tests.rs` was not run. No stochastic agent trial,
release-binary installation/activation, cross-platform crash/power-loss test, or
live provider/incident rehearsal was run. The optional release-build Python
`test_state_upgrades.py` subprocess rehearsal was not run in T07; executed Rust
storage/Forgeup tests establish isolated conversion and recovery behavior, not
production migration sign-off. These limitations do not waive operational gates.

## Acceptance evidence

**Pass** below means observable source/test delivery under the stated local test
boundary, not effective live behavior or semantic certification of checkpoints.
All named Rust tests below ran in the successful workspace suite. Paths and test
identifiers are supplied so evidence can be inspected or rerun without this session.

| AC | Result | Observable evidence |
| --- | --- | --- |
| 1 | Pass | `crates/app/tests/implementation_continuations_documentation.rs` validates explicit object disposition, Fail sentinel, authenticated Implementation-only authority and no Finding effects. Store `manual_continuation_cannot_adopt_checkpoint_or_spend_budget` and `review_cannot_submit_implementation_continuation` deny other roles; real-Git RPC rejects foreign credentials/attempts. No prose inference. |
| 2 | Pass | Store `deliberate_continuations_are_bounded_and_atomic` asserts unchanged Check list, no output/pass, In Progress status, identical next Check ID and captured handoff through repeated actual application dispatch. RPC `implementation_continuation_rpc_retains_clean_checkpoint_and_handoff` independently checks no Assessment. |
| 3 | Pass | Continuation fixture independently blanks all four fields and compares complete WorkState before/after rejection. RPC independently omits each field. Application documentation tests reject malformed examples. Verification is required nonblank truthful prose, not an automated certification that every gate was run. |
| 4 | Pass | RPC fixture accepts committed B from A, rejects dirty/untracked checkpoints and invalid/foreign credentials/attempt IDs. Store continuation fixture rejects dirty, mismatched commit and stale generation without input/counter/hold adoption. Persistence checks active Managed Worktree and bound execution directory; Repository identity must match bound evidence. |
| 5 | Pass | Store continuation fixture injects a failing final `record_attempts.outcome_json` update at initial and limit-reaching acceptance. Reopened WorkState and holds remain unchanged on failure. Successful acceptance records B as input, handoff, counters and replay result together. Later same-Check dispatch accepts that unchanged checkout. |
| 6 | Pass | Same fixture makes provider cleanup unavailable, then adds an unknown retained resource: no successor launch, retained capacity across fresh application connections, exact replay still read-only. Existing `controlled_checks_continue_only_after_retirement_and_survive_restart` and provisioning/crash-window regressions preserve uncertainty. |
| 7 | Pass | Continuation fixture uses ordinary daemon reconciliation and same-Check bootstrap context; separately denies dependency, provider/profile readiness and hold gates. Existing `normal_dispatch_prepares_worktree_only_after_readiness_and_capacity` and execution-mode tests cover shared gates. Continuation does not add an alternate dispatch policy. |
| 8 | Pass | Concurrent exact submissions, fresh connections, closed/restarted replay, conflicting message replay, body-edit/reset replay all retain the original result and one increment in store fixtures. RPC and CLI tests cover foreign credentials and protected delivery. Recovery fixture proves revoked source context remains inaccessible: replay never reopens an Assignment. |
| 9 | Pass | Store `deliberate_continuations_are_bounded_and_atomic`, `continuation-total` case: 19 changed checkpoints continue; twentieth accepted input remains, hold blocks the next claim and launch count stays 20. |
| 10 | Pass | Same test's unchanged/unchanged and unchanged/changed/unchanged cases assert counts against each bound input, changed reset of only the unchanged count, second-unchanged hold, retained checkpoint and stronger prior diagnostic. |
| 11 | Pass | Mixed-accounting fixture runs Continuation → interruption → substantive failure → pass → review handback and compares continuation accounting. Exhaustion tests cover restart/snapshot, body edits and freshness invalidation; ordinary resume/remediation reset cannot renew allowance. |
| 12 | Pass | Continuation `verify_reset` fixture rejects blank attribution, nonexistent/unrelated Checks and wrong Workspace; audit insertion failure rolls back counters; concurrent resets append separate durable rows. Holds, input/checkpoint and Assignments stay unchanged; separate resume/retirement gates remain required. |
| 13 | Pass | CLI `continuation_human_and_json_inspection_preserve_accounting_and_context` covers active, unchanged/total exhausted and reset histories. Real-Git RPC verifies selected work inspection/reset. Human renderer explicitly denies equating changed evidence with meaningful progress. |
| 14 | Pass | Mixed-accounting and existing failed-Implementation/remediation tests preserve failure accounting. Manual/review Continuation denial and existing review-mutation, Findings, cumulative publication and ordered-Check regressions keep review/publication authority unchanged. |
| 15 | Pass | Real-Git `check_bound_rpc_context_outcome_and_normal_dispatch_share_same_owner` reproduces A → B → interrupted B, both clean and dirty. Structured `interrupted-evidence-changed` rejects before input, counters, Assessment, Finding, update, hold, claim or owner effects; retained files/HEAD match. CLI `changed_interruption_cli_reports_safe_choices_without_success_json` checks safe guidance/redaction. Manual historical replay and legitimate unchanged-interruption tests remain green. |
| 16 | Pass | Clean/dirty recovery fixtures preserve unreported HEAD, staged tracked content, unstaged tracked content and binary untracked content. Ordinary resume repeatedly rejects unexplained B without adopting it. Unknown cleanup blocks replacement; closure/missing heartbeat is not proof of absence. |
| 17 | Pass | `clean_retained_work_requires_atomic_operator_authority_and_safe_retirement`, `dirty_retained_work_survives_restart_and_launches_one_salvage_agent`, and real-Git clean/dirty recovery RPC tests deny live/uncertain ownership and wrong scope/Repository/branch/path. Authorization audit failure rolls back, concurrent retries agree, successor sees recovery context and can commit clean salvage. Fingerprint drift in tracked/untracked/index state blocks claim; consumed authorization cannot reserve another successor. |
| 18 | Pass | Continuation and recovery fixtures assert immutable Base Commit and retained historical handoff/counters/outcome replay; no pass/output is fabricated by recovery. Full workspace regressions retain Findings, publication gates, cumulative evidence, exact pins and final-retirement completion gates. No branch-reset/force-push/merge path was added. |
| 19 | Pass | Three application documentation tests resolve candidate and tracked origin through real ResourceCatalog, compare runtime policy, validate examples, exact glossary block and stopping/outcome instructions. CLI parser tests validate fenced/inline candidate and operator commands without execution. Source delivery remains separate from immutable registered profile pins. |
| 20 | Pass | `crates/store/tests/storage_upgrades.rs` tests revisions 14→current, 15→current, 16→current, and semantic 17/18→19: old ordinary-open rejection without writes, isolated explicit upgrade, exact preservation and no invented events/resets/fingerprints. Store upgrade unit tests inject marker-write failures and assert rollback. Historical optional fields default empty/null; original accepted JSON is retained. CLI/RPC/documentation tests cover new contracts and diagnostics. See compatibility limits below. |
| 21 | Pass | Public Assignment/operator RPC, CLI protected-delivery and application/store fixtures together cover the incident, checkpoints, acceptance failure/replay, bounds, restart and salvage. Required default-parallel root gates passed; supplementary/unrun gates are identified above, not claimed. |
| 22 | Pass | Work consists of local source/tests/docs and isolated fixture operations. No Revision installation/activation, live migration, profile registration/selection, Check repin, #37 recovery/dispatch, merge, force-push, branch reset or parent-Spec completion occurred. External profile authoring source was not overwritten. |

### Evidence locations and failure boundaries

Continuation scenarios are in `crates/store/tests/support/implementation_continuations.rs`,
called by `crates/store/tests/task_check_continuation.rs`; recovery scenarios are in
adjacent `support/retained_work.rs`. Public real-Git tests live in
`crates/server/src/task_check_attempt_tests.rs` and `retained_work_tests.rs`.
Fingerprint-specific tests are in `crates/server/src/retained_state.rs`.

Retained fingerprints bind HEAD, staged entries/flags, names, modes and bytes
(including ignored/untracked files). Links, submodules, unreadable or unsupported
state fail closed; they are not silently treated as clean salvage. Operators must
exclude arbitrary external writers during authorization and dispatch: repeated
observation and transactional Forge ownership do not lock out same-user Git or
filesystem mutation. No test here claims universal process-death detection.

## Corrections and independent reviews

Separate read-only Pi processes performed implementation and coding-standards
reviews using the named repository skills. Both initial requests explicitly named
the entire inherited range, the Spec and T07, not merely this Task's local diff.
They identified four concrete safety gaps:

1. Clean salvage did not repeat Repository/branch/Base Commit checks before the
   first successor. `observe_recovery_input` now validates clean **and** dirty
   pending recovery before/after provider preflight; the writer claim compares
   either kind with the latest source authorization.
2. `git ls-files --stage -v` omitted intent-to-add. V2 fingerprinting adds extended
   index flags from fail-closed debug parsing, excluding mutable stat-cache data.
3. Ordinary checkpoint observation labeled a HEAD with the configured Repository
   without independently checking actual Git identity. Shared observation now uses
   the adapter's Repository/branch/Base Commit validation.
4. Git configuration could hide untracked files, and index trust flags could hide
   tracked modifications. Assessment explicitly requests all untracked files and
   submodule dirtiness; assume-unchanged/skip-worktree conservatively require dirty
   salvage, whose observer hashes actual bytes independently of those flags.

The clean-branch and intent-to-add regressions were run **before** correction:
`cargo test -p forge-server clean_retained_work_recovery_rpc` and
`cargo test -p forge-server observation_is_read_only` both failed (exit 101).
The former reached provider/profile preflight instead of rejecting identity drift;
the latter produced identical digests for different index semantics. Both pass with
the fixes. Added public RPC assertions reject foreign-Repository checkpoints and
clean-salvage substitution even at the same HEAD, config-hidden untracked work, and
hidden tracked changes under either trust flag, without adopting input or launching.
Existing dirty RPC tests now also run with untracked files hidden in Git config.

Because the changed fingerprint/identity contract is persistent safety policy,
T07 advances schema 18 → 19 explicitly rather than allowing an older dispatcher
to ignore it. New isolated upgrade tests verify ordinary-open rejection, byte-exact
historical authority preservation and marker-write failure rollback. Existing test
fixtures now supply actual persisted Base Commit input and explicit fake identity
validation; production checks were not weakened to preserve incomplete fixtures.

Both independent follow-up reviews inspected tracked T07 corrections atop the
same inherited range. Implementation review reported all four issues resolved,
no remaining concrete defect/scope creep, and 47 passing focused tests. Standards
review reported no introduced documented-standard violations, with prior findings
resolved by inspection; it ran diff checks but not Rust gates. Their coverage did
not include the then-untracked draft report, full release orchestration, or live
provider execution. The initial and follow-up reviews are local reviews, not Forge
Check results. No Forge Records, bound review Checks or reviewer-owned Findings
exist for this plan; none were invented or self-cleared.

Final required gates passed after corrections and fixture updates:
`cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace` (**561 passed, 0 failed, 1 existing ignored**, 72 suites,
default parallelism), and `git diff --check`. The two new tests cover schema-18
ordinary-open/upgrade preservation and schema-19 stamp rollback. Focused Stack
execution (2), continuation dispatch/recovery (39), profile documentation (3),
operator-guide parser (1), and Python skill portability (4) runs also passed.
Intermediate fixture failures (missing Base Commit/fake identity implementation),
one immutable-audit fixture setup error, and one Clippy semicolon diagnostic were
corrected before this final green run; they were not waived or hidden by serial
execution. The previously listed optional/unrun gates remain unrun.

Final independent read-only implementation and coding-standards review covered
`b1f900a26fb2c3f74245e0f10e94b1a970732325..04e867eacb416335d46c55a81aeb308df6640fcf`,
including later identity/Base Commit fixture adjustments and this report. Results:
**no actionable implementation findings; no introduced documented-standard
violations**. It confirmed all four earlier gaps resolved and the report's AC1–22
claims/limitations consistent with inspected tests and final gate logs. It ran
range `git diff --check`, but did not independently rerun Rust gates. No files,
managed Findings or live state were mutated by review.

T07's local implementation/verification deliverables are complete with no unresolved
source-review findings or required-gate failures. The ignored runtime test, optional
operational rehearsals, and historical-v1 recovery limitation remain explicitly
outside a release-readiness or live-recovery completion claim.

## Compatibility and separate operational handoff

- Storage target is **19**, from actual T01 input schema 14: 15 adds continuation
  accounting, 16 adds explicit zero/reset audit, 17 adds recovery authority and
  removes automatic unexplained-commit adoption, 18 enforces content-bound dirty
  salvage, and 19 fences corrected identity and extended-index binding. Ordinary
  opening never converts older stores. Explicit supported isolated upgrades
  support schemas 7–19, retain history/counters, and invent no
  historical Continuations or recovery authority. Management Protocol is unchanged.
  Existing v1 dirty fingerprints remain historical and fail closed at dispatch.
  Exact retry returns the original audit, not v2 authorization: there is no supported
  in-place reauthorization of an unconsumed old dirty recovery. This is an explicit
  **operational blocker** for such state, not a reason to edit persistence or
  discard work. Preserve it for separately approved recovery design.
- Wire changes include an object-form `continuation` disposition and additive
  attempt/work-state context, reset and recovery contracts. Existing disposition
  spellings remain valid. Strict older peers can reject new fields/variants;
  coordinate CLI/server versions. Historical absent fields decode via defaults,
  without rewriting original accepted results. Changed interruption rejection is
  intentional validation tightening, not retrospective outcome reinterpretation.
- The tracked profile candidate/origin/handoff under `docs/profiles/` are delivery
  artifacts only. Existing Check pins and registered profile revisions remain
  immutable. Workspace selection/template edits affect future Tasks, not existing
  Checks; no supported existing-Check repin operation is asserted.
- Separately approve revision installation/activation, explicit offline state
  transfer with backups/rehearsal and safe agent/resource exclusion, profile source
  copying/preview/registration/selection, handling existing pins through supported
  mechanisms, and eventual #37 inspection/recovery/dispatch. Follow
  [the profile handoff](../profiles/implementation-continuations-handoff.md),
  [state upgrades](../state-upgrades.md), and
  [Assignment recovery](../assignment-recovery.md). Source tests are not rollout
  approval and historical incident evidence is not current ownership authority.
