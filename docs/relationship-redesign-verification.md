# Relationship redesign: final local verification

Task #26, Implementation Check 81, verifies the cumulative published Spec #19
implementation, including Task #25's Manual-attempt correction. This is local
acceptance evidence, not release authorization or Spec completion.

## Exact scope and authority

- First Task #20 retained root Base Commit:
  `0beae0d403c1b322fccbb595f8a97bb75ba7fa6f`.
- Completed, verified Task #25 output and Task #26 retained input pin:
  `11cebb192c3aa37c08e938e435a708df9229a249`.
- Inherited cumulative verification range:
  `0beae0d403c1b322fccbb595f8a97bb75ba7fa6f..11cebb192c3aa37c08e938e435a708df9229a249`.
- Final tested output is the enclosing verification commit, identified by its full
  hash in Task #26's authenticated handoff. Final cumulative range is the same
  root through that commit, not merely this Task's test/documentation diff.

Authority: own Comment 81 contains operator-collected retained Stack evidence for
Tasks #20–#24, deliberately delivered within this Assignment's bounded access.
Task #25 Comments 82/83 report its exact committed correction and verified PR 21;
its authoritative Record is Done. Own Comment 84 and `assignment stack --json`
agree on the exact inherited commit, predecessor 25 and base `feat/forge-25`.
Root destination was `albizures/forge`, remote `origin`, branch `main`.
Git root-to-head ancestry independently corroborates this evidence; it does not
establish the root pin or grant transitive Record access. The full feature inventory
was inspected from the root, including the earlier cumulative review report.

Read full Spec #19 definition, Comments (none), Update Log; assigned Task and
predecessor definitions, comments and histories; bound Check; coding standards;
relevant glossary/ADRs, public contract fixtures, Stack and upgrade guidance.
The old live planning vocabulary is orchestration evidence, not the target API.
No live plan edits, daemon replacement, dispatch or conversion were performed.

## Integrated verification addition

Extended `derived_chain_prepares_non_default_root_and_carries_multiple_unchanged_results`
in `crates/store/src/derived_execution_tests.rs`. One isolated fixture now combines:

- Builds-On alone creates root → review → verification → changed consumer;
- independent consumer Waits-For decision, with no Stack configuration;
- non-default `feature/payments` root and exact inherited commit through two
  unchanged results, neither requiring a branch or PR;
- preparation/start denial while the decision is unfinished, even with verified
  code; rejected calls preserve consumer history;
- decision completion using ordinary Checks without Stack publication, and no
  automatic successor start;
- inherited PR base carried from the changed ancestor without merge;
- deleted/moved branch and missing PR block later start without repinning;
- changed consumer publication/completion against the carried base.

This strengthens combined-scenario coverage; no production defect was found or
production code changed. The targeted test passed on its first run. Existing
real-Git `cumulative_exact_git_input_and_publication_lineage_survive_database_reopen`
separately verifies exact commits (even with equal trees), non-default branch,
unchanged publication and reopened-database handoffs. Hosting is controlled test
evidence, not a real GitHub exercise.

## AC1–AC15 acceptance matrix

All test references below ran in `cargo test --workspace` with default parallel
execution. Paths are repository-relative; named tests identify concrete evidence.
“Pass” is bounded by the unrun/operator limits below.

| AC | Concrete coverage inspected and executed | Outcome |
| --- | --- | --- |
| 1 | `cli/tests/builds_on.rs`: CLI/RPC declaration, human/JSON inspection, derived predecessor, removal and explicit replacement; `rpc/tests/ordering_dependencies.rs`: canonical wire kinds. | Pass |
| 2 | `store/tests/workspace_records.rs`: `waits_for_is_explicit_atomic_and_part_of_the_shared_prerequisite_graph`, `waits_for_spec_completion_unblocks_without_starting_and_lifecycle_edits_fail`; `store/src/builds_on_tests.rs`: mixed cycles; integrated decision test above. | Pass |
| 3 | `waits_for_preserves_endpoint_and_project_scope_eligibility`, Workspace confinement tests; `builds_on_rejects_projectless_cross_project_and_non_task_endpoints`. | Pass |
| 4 | `topology_rejects_forks_multiple_inputs_mixed_cycles_and_conflicting_intents`; CLI explicit removal/replacement; lifecycle and rollback fixtures. No duplicate or silent conversion. | Pass |
| 5 | `builds_on_alone_derives_linear_transitive_topology_and_keeps_waits_separate`, topology rejection test, `unchanged_code_consumers_carry_transitive_lineage_without_extra_builds_on_edges`. | Pass |
| 6 | CLI fixture uses `release/2.x`; integrated test uses `feature/payments`; real-Git cumulative fixture uses `starting`. Successor competing root configuration rejects. | Pass |
| 7 | `same_spec_waits_for_requires_completion_but_neither_stack_nor_automatic_start`; integrated unchanged review chain; same-Spec explicit-wait planning fixtures. | Pass |
| 8 | `incomplete_derived_spec_cannot_start_or_rewrite_checks`, `missing_root_blocks_both_endpoints_without_fallback_or_partial_preparation`, `incompatible_immutable_checks_block_standalone_authorization_and_preparation`; `spec_plan_race_tests.rs` covers child start, Assignment claim and Manual begin. | Pass |
| 9 | `inherited_result_and_downstream_pins_restrict_topology_transactionally`, started-Spec structural/race tests, failed relationship/removal audit rollback tests; existing running/prepared Stack restrictions. | Pass |
| 10 | Integrated chain; `cumulative_stack_tests.rs` drift/stale unchanged tests; `cumulative_gate_tests.rs` observation-to-write invalidation; `manual_attempt_reobserves_predecessor_hosting_after_successful_start` covers deleted/moved branch and missing/closed/draft PR after successful start; dispatched changed/unchanged restart tests. | Pass |
| 11 | Same-Spec waiting test and integrated non-stacked decision completion above; ordinary Workspace execution regressions. No publication evidence invented for waits. | Pass |
| 12 | `upgrade/intent_tests.rs::historical_matrix_converts_only_explicit_or_provable_intent_and_preserves_history`, mixed schema-12 topology and incomplete-plan fixtures; mapping matrix below. | Pass |
| 13 | Semantic-stamp rollback/exact retry, conflicting decisions/cycles/scope failures, `stack_upgrade_tests.rs` completed/prepared evidence retention, `storage_upgrades.rs` source-byte/WAL/history/blocker tests; Forgeup interruption/non-activation fixtures and private-home subprocess test. | Pass locally |
| 14 | CLI/RPC legacy-name rejection and hidden `stack after` tombstone, no-write assertions, recursive help and `documented_skill` tests; searches of current docs/skills found old terms only in rejection/migration/historical context. | Pass |
| 15 | Compared glossary Dependency/Waits-For/Builds-On/Stack/Stack Predecessor blocks with approved Spec text; ADR 0011 and supersession notices in 0003/0009 agree. Records/Stacks/planning skill teach canonical declarations. Parser and portability gates pass. | Pass |

## Historical-state mapping and failure matrix

Executed source schemas 7–12 in the intent fixture and 7–13 in the real subprocess
transfer fixture. These are constructed historical contracts, not actual old binaries.

| Source condition | Required/observed result |
| --- | --- |
| Explicit legacy predecessor | Builds-On; absorbs associated completion edge, preserves linear topology. |
| Explicit ordering-only dependency | Waits-For. |
| Ordinary dependency on indirect Stack ancestor | Waits-For, retaining direct completion constraint. |
| Eligible standalone, cross-Project, Projectless, Spec or Task-to-Grilling ordinary dependency | Waits-For. |
| Same-Spec ordinary Task dependency without required ancestry | Unresolved with affected pair diagnostics; no inferred intent or activation. Schema 7 has no Stack evidence and requires decisions for those pairs. |
| Mixed schema-12 canonical and legacy links | Preserve canonical links and absorb provable legacy predecessors. |
| Incomplete Open chain / same-schema transfer | Preserve missing configuration without inventing preparation; execution still blocks. |
| Started/prepared/completed decision override; cycles/scope/conflicting designations | Reject without partial conversion. |
| Conversion failure or injected schema-stamp failure | Transaction rollback/source preservation, no accepted partial target; explicit decisions and exact retry tested. |
| Completed/prepared chain, publication, unchanged and forced-completion evidence | Retain pins, result identity, history and evidence, with no fabricated branches or execution. |

The Python test uses private temporary `FORGEUP_HOME`, strips inherited Forge
endpoint/Assignment environment variables, registers locally built matching binaries,
starts only private daemons, checks running-daemon rejection, stops them, constructs
private historical fixtures, checks unresolved non-activation, recovers/retries with
explicit decisions, checks source bytes and exact Records/Comments/Updates, then
activates only the disposable target. Dispatch leases expire normally, not by reset.
Its `finally` stops private daemons. This is not activation of any installed/live state.

## Executed gates and limitations

On inherited output all six required gates passed: fmt, strict workspace/all-targets
Clippy, default-parallel workspace tests, diff check, documented-skill parser (2),
and Python skill portability (4). Workspace: 521 passed, 0 failed, 1 ignored across
69 test/doc-test invocations. The ignored real-Pi integration test
`herdr::pi_flags_tests::pi_flags_runtime_selected_and_empty_resources` requires
`FORGE_TEST_PI`; it was not run. No serial-suite substitution was used.

Also executed successfully:

```sh
cargo build --release -p forgeup -p forge-cli -p forge-server
python3 -m unittest discover -s tests -p 'test_state_upgrades.py'
cargo test -p forge-store derived_chain_prepares_non_default_root_and_carries_multiple_unchanged_results
```

The Python upgrade suite is one test with seven schema subtests. After the test
addition and report, all six gates, matching release build and private-home
upgrade suite were rerun on the final committed output; exact results/hash are in
the authenticated Task handoff. Logs are local temporary diagnostics; this report
and the Record handoff are durable evidence.

No unresolved local acceptance failure is known. Not run: real Pi/provider launch,
live GitHub/credential/publication operations, actual historical-binary rollback,
production backup/restore, cross-platform or crash/power-loss certification. Local
fixture success does not establish recovery readiness.

## Operator handoff: installation is not activation

1. Follow release/integration policy separately. Select compatible immutable
   Forgeup, CLI and daemon artifacts; install the updated manager first. Schema 13
   and offline upgrade interface 2 require it; Management Protocol remains 1.
2. Back up retained source, registry and recovery evidence; rehearse restore and
   actual old-binary rollback. Independently establish old-agent/resource safety,
   resolve ownership blockers, stop both daemons and wait for leases. A missing
   heartbeat or stopped daemon is not proof that agents cannot act.
3. Use the explicit `upgrade plan` / `prepare` / `status` workflow documented in
   [Revision State upgrades](state-upgrades.md). Resolve each ambiguous pair through
   approved source planning or explicit eligible Waits-For decisions. Preserve
   failed staging/receipt evidence and use public recovery/retry operations;
   never edit live SQLite, markers, pins, journals or ownership facts.
4. Validate the destination and source freshness before separately authorizing
   activation. Preparation neither selects nor starts the target. Rollback is an
   acknowledged fork, not reverse migration or automatic reconciliation.
5. Keep predecessor branches/open PRs available for downstream verification. Live
   execution must re-observe hosting after activation. Do not reset evidence,
   bypass recovery safeguards or treat these tests as permission to dispatch.

No push, PR creation, integration, installed Revision selection, live upgrade,
Check/profile repin, destructive administration or Spec completion was requested
or performed by this verification Assignment. Forge owns the subsequent review,
Publication and lifecycle gates; the operator owns rollout and Spec completion.
