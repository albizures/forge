# Relationship redesign: cumulative acceptance and standards review

## Evidence and scope

Task #25, Implementation Check 77, reviews published Spec #19 in `factory/forge`.
This is a cumulative feature review, not Task #24's local diff.

- Authoritative root Base Commit: `0beae0d403c1b322fccbb595f8a97bb75ba7fa6f`.
- Inherited, completed Task #24 output: `7b22d69037cda4da280619ad89c1c4a8c3a1baee`.
- Exact inherited review range: `0beae0d403c1b322fccbb595f8a97bb75ba7fa6f..7b22d69037cda4da280619ad89c1c4a8c3a1baee`.
- Reviewed corrections: the Manual-attempt gate and regression described below,
  plus this report and the corresponding Stack documentation clarification.
  The enclosing commit and Task #25's authenticated handoff identify the corrected
  output; the report does not substitute a guessed future commit.

Source authority is Task #25 Comment 80: operator-collected public Forge
preparation/publication evidence for Tasks #20–#24, including the first Task's
retained root pin. Its verified final commits are, in order:
`2248cbb9fa8e83e7878d8211b156557cb5cf3736`,
`e02785b479e7d19165acd9500a950fb92a3ea665`,
`2ff0c7b059d72ee87f28ae1e21c91ac5fa12f4b2`,
`8399f5d006b999ff2fadec9219490d2d784df854`, and the inherited head above.
Each successor pin equals its predecessor's verified result. Assignment Stack
inspection independently reports Task #25's retained pin as the inherited head;
Git confirms root-to-head ancestry. Record IDs and Git history were not used to
infer the root pin. Task #24 is Done and its handoff records its exact output.

Inputs inspected: full Spec #19 Body, Comments (none), Update Log; Task #25 and
Task #24 context, Comments and preparation/publication handoffs; full cumulative
change inventory; runtime glossary and Context Map; ADRs 0003, 0009 and 0011;
`CODING_STANDARDS.md`; affected application, transactional storage, upgrade,
CLI/RPC/server contracts, tests, documentation and shipped skills. Applied the
`review-implementation` and `review-coding-standards` workflows. The bound Check
is Implementation, so focused corrections and this durable deliverable are
permitted; this is not an attempt to edit evidence during a review Check.

## Implementation review findings

**R1 — corrected: Manual attempt entry could admit stale hosting evidence.**
Spec AC10 requires: “A Done predecessor with missing, moved, stale, or
contradictory required evidence blocks successor execution.” Task #25 explicitly
requires auditing “all manual/assigned execution authorization paths.”
`Application::begin_record_attempt` in `crates/app/src/outcomes.rs` delegated
straight to storage. Storage revalidated retained lineage, but could not observe
Git/hosting. After successful preparation and Manual start, deleting/moving the
predecessor branch or losing/closing/drafting its PR still admitted a new Manual
attempt. Earlier successful start was being treated as continuing authorization.

The correction calls the existing application-owned `verify_stack_execution`
before delegating. Hosting observations remain outside the database transaction;
the existing writer gate still checks scope, lifecycle, ownership, immutable
Checks, pins, prerequisite completion and evidence generation atomically. This
adds no fallback, repin, Git mutation or recovery permission. Outcome acceptance
and its exact durable replay path are unchanged.

Regression:
`manual_attempt_reobserves_predecessor_hosting_after_successful_start` in
`crates/store/src/derived_execution_tests.rs`. With the regression alone, the
command failed because a deleted branch returned an accepted `Attempt`. With the
fix, deleted/moved branches and missing/closed/draft PRs all reject without
attempt rows, work-state changes, pin changes or history. Restored evidence admits
the same requested attempt. The first post-fix run exposed an overly strict test
expectation for the preserved hosting diagnostic code; that assertion was fixed
before the passing run.

Summary: one acceptance gap found and corrected; no unresolved material implementation finding.

## AC1–AC15 coverage matrix

“Pass” below means reviewed implementation plus passing local deterministic
coverage, not production migration or hosting sign-off. Test references are
repository test files; the full workspace run includes them.

| AC | Reviewed policy/contract and observable evidence | Result |
| --- | --- | --- |
| 1 | CLI parser and server adaptation accept explicit canonical kinds; `ordering_dependencies::relate_with_intent` atomically stores prerequisite, designation, evidence row and audit. `cli/tests/builds_on.rs` round-trips declaration, human/JSON inspection, derived predecessor and removal; RPC serialization tests cover both kinds. | Pass |
| 2 | Both intents share persisted prerequisite rows and recursive cycle/readiness SQL; application readiness uses typed prerequisite intent. `store/tests/workspace_records.rs` and `store/src/builds_on_tests.rs` cover unfinished prerequisites, mixed cycles, rollback and completion without automatic start. | Pass |
| 3 | `app::prerequisites::validate_endpoints` owns the waiting matrix; `app::stacks::validate_stack_predecessor` owns same-Project Task inheritance. Storage checks selected and shared Workspace inside the transaction. Endpoint/scope matrices cover Projectless, cross-Project waits, Task-to-Grilling and invalid Builds-On endpoints; Workspace isolation suites remain green. | Pass |
| 4 | Shared row uniqueness and explicit designations prevent two kinds on one pair. Removal validates actual intent before deletion. `builds_on_tests::topology_rejects_forks_multiple_inputs_mixed_cycles_and_conflicting_intents` covers explicit replacement, duplicate rejection and non-mutation. Prepared/started-Spec edits reject. | Pass |
| 5 | `builds_on_dependencies` has one predecessor per source and a unique successor per target; shared cycles reject mixed graphs. Stack reads derive topology rather than retaining a second predecessor. Tests cover forks, multiple inputs, transitive unchanged consumers and absence of redundant ancestor links. | Pass |
| 6 | Root configuration owns repository/remote/starting branch. Canonical successors use empty source configuration and derive inputs. `stacks::load_member` rejects competing configuration. CLI integration uses `release/2.x`; derived execution tests prepare `feature/payments` and reject successor root configuration. | Pass |
| 7 | Same-Spec explicit waits bypass inheritance requirements, not completion. `app::spec_plans` uses typed relationships, not prose, shared parentage or file changes. Spec-plan, unchanged-Stack and cumulative-Stack tests cover waits without publication and code-consuming review results without empty PRs. | Pass |
| 8 | Open declarations/configuration permit incomplete plans without Check rewriting. `store::spec_plans::{validate,execution_plan,authorize}` and Stack compatibility checks reject missing root or incompatible immutable profiles/Publication at start/authorization. Derived execution, Spec execution and Spec race suites exercise these gates. | Pass |
| 9 | Writer transactions check affected suffixes across canonical links, Open/unprepared state, worktree claims, attempts, Assignments and started Specs. Builds-On audit/removal failure injection proves rollback; Spec race tests serialize structural writes versus start. Retained pins and explicit recovery restrictions remain intact. | Pass |
| 10 | Immediate verified result and publication base propagate through changed/unchanged chains without merge. Application hosting observations plus persisted generation/pin checks reject drift with no Project-checkout fallback. Cumulative/derived execution and gate suites cover deleted/moved branches, bad PRs, stale generations and observation-to-write invalidation. R1 closes the Manual-attempt entry gap. | Pass after R1 |
| 11 | No Builds-On or explicit root configuration means no Stack to prepare or publish. Waits alone do not create Stack evidence rows. Workspace, same-Spec waiting and CLI replacement tests retain non-stacked execution behavior; root configuration remains available before a future successor is declared. | Pass |
| 12 | `upgrade/prerequisite_intent.rs` converts legacy predecessors to Builds-On; explicit ordering designations, indirect-ancestor constraints and other eligible ordinary dependencies to Waits-For. Same-Spec missing ancestry is unresolved. `historical_matrix_converts_only_explicit_or_provable_intent_and_preserves_history` covers source schemas 7–12; mixed schema-12 topology is covered separately. | Pass |
| 13 | Staging conversion and schema stamp share one transaction. Validation precedes target publication; activation remains separate. Intent and storage-upgrade tests prove unresolved diagnostics, stamp-failure rollback, exact retry, source preservation, retained completed/publication/unchanged/forced-completion evidence, and incomplete Open-plan transfer without fabricated configuration. Forgeup tests reject partial activation and retain explicit decision inputs. | Pass locally |
| 14 | CLI/RPC/Application reject both old relationship names; hidden `stack after` tombstone rejects with replacement guidance. Public predecessor configuration is rejected. Current-schema legacy topology decoding fails closed. Canonical inspection removes the competing wire flag. Public CLI/RPC tests verify diagnostics, non-mutation and retained operational help. Current docs/skills searches found only historical/rejection uses of old declaration names. | Pass |
| 15 | `crates/CONTEXT.md` contains all five exact proposed definition blocks; Ordering-Only Dependency is removed. ADR 0011 contains the full approved architectural trade-off; ADRs 0003/0009 point to it while retaining readiness/Spec gates. Current records, Stack, upgrade and shipped planning guidance teach a single Builds-On declaration. Parser and portability tests pass. | Pass |

## Standards review

No unresolved introduced violation of `CODING_STANDARDS.md` was found.
Rule-specific review:

- **ARCH-1/2/3/4/5:** endpoint and inheritance policy belongs in `app`, persistence
  and migration enforcement in `store`, wire conversion in `rpc`, adaptation in
  `server`, parsing/presentation in `cli`, and state orchestration in `forgeup`.
  Repeated checks protect distinct boundaries. No dependency was added. Glossary
  changes explicitly resolve the approved vocabulary transition.
- **RUST-1/2/3:** typed prerequisite alternatives preserve unknown/legacy intent;
  deserialization and external failures propagate rather than becoming defaults.
  Historical storage strings are boundary representations, not competing public
  policy. R1 reuses the established diagnostic-preserving observer.
- **TX-1/2/3/4:** relationship/designation/evidence/audit changes are transactional;
  unique constraints protect topology. External observations precede writer
  revalidation. Accepted-outcome replay is not routed through R1's live check.
- **SAFE-1/2/3/4, SEC-1/2/3:** no new launch/cleanup or capability delivery authority;
  uncertainty, resource ownership, holds and budgets remain retained. Assignment
  reads stay one-hop and server-authorized. No new shell/process or secret path.
- **CONTRACT-1/2/3:** revisions 12/13 explicitly fence storage/semantic changes;
  incompatible opens reject before initialization. CLI/RPC break and removed flag
  are intentional, documented and tested. Offline upgrade interface 2 is separate
  from unchanged Management Protocol 1; interface-1 receipt handling is tested.
- **TEST-1/2/3, SKILL-1, VERIFY-1/2:** R1 supplied the missing adverse entry-point
  regression. It failed before the fix and passes after it. Default-parallel gates,
  command-parser and portable-skill checks pass; limits are reported below.

Summary: R1's execution/negative-coverage gap is corrected; no separate outstanding coding-standards finding.

## Verification actually run

At the inherited head, before corrections:

- `cargo test --workspace`: pass, 520 tests, 69 test/doc-test invocations,
  default parallel execution.
- `cargo fmt --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test -p forge-cli --bin forge documented_skill`: pass, 2 tests.
- `python3 -m unittest discover -s tests -p 'test_skill_*.py'`: pass, 4 tests.
- `git diff --check`: pass.

After R1:

- `cargo test -p forge-store manual_attempt_reobserves_predecessor_hosting_after_successful_start`:
  failing-before (accepted attempt on deleted branch), passing-after (1 test).
- `cargo fmt --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test --workspace`: pass, 521 tests, 69 test/doc-test invocations,
  default parallel execution.
- `cargo test -p forge-cli --bin forge documented_skill`: pass, 2 tests.
- `python3 -m unittest discover -s tests -p 'test_skill_*.py'`: pass, 4 tests.
- `git diff --check`: pass. Parser/portability and formatting/diff checks were
  also rerun after this report was written.

No release executable build, Python state-upgrade subprocess rehearsal, real
provider/hosting exercise, installation, live conversion or production recovery
rehearsal was performed in this review. Earlier Task handoffs are historical
supporting evidence, not checks rerun here. No push, PR creation, integration,
Check repinning, live relationship edits or worktree administration was performed.

## Final verification handoff and boundaries

Task #26 must inherit Task #25's exact **completed, verified** output, not the
original Task #24 head. Keep the authoritative root pin above for the final
cumulative range. Re-run all four required Rust/diff gates and both shipped-skill
checks on that output. Also build matching release executables and run
`python3 -m unittest discover -s tests -p 'test_state_upgrades.py'` using the
private temporary Forgeup-home subprocess workflow in `docs/state-upgrades.md`.
Verify the corrected Manual-attempt regression remains in the passing suite and
exercise all ACs, including migration failure/non-activation and unchanged chains.

There are no known unresolved material acceptance defects after R1. This review
is complete as a local implementation/standards review, not integration or release
approval. Forge still owns subsequent independent review and Publication Checks.
Changed output requires normal verified publication, not an unchanged-result claim.

Operator-only boundaries remain: authorized push/integration and release policy;
matching installed manager/CLI/daemon selection; real provider and GitHub readiness;
backup/restore and actual historical-binary rollback rehearsal; verified agent
retirement/resource safety before migration; explicit ambiguous-intent decisions,
staging validation and separate activation; recovery of wrong prepared/started
plans without resetting evidence. Local deterministic tests do not prove universal
process death, production migration safety, or crash/power-loss recovery readiness.
