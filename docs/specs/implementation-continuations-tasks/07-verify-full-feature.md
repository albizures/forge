# T07 — Verify the full feature and correct remaining gaps

## Authority and input

Parent: [local Spec](../implementation-continuations.md). Read the [shared execution contract](README.md#shared-execution-contract). Project: `factory/forge`. **Depends on T06; Builds-On T06.** Inherit the complete verified feature output.

Spec AC: **1–22**, full-range integration/acceptance verification. This Task is not authorization to activate the feature or recover the live incident.

## Outcome

A committed, reproducible verification report maps every Spec acceptance criterion to actual tests/evidence, identifies compatibility and rollout requirements, and resolves in-scope implementation gaps without weakening the agreed design.

## Required review range

Inspect **T01's actual prepared input through T06's verified output**, plus any corrections made by this Task. Record the full input/output commit identities and exact range in the report. Do not substitute #37's historical input, main checkout HEAD, or this Task's local diff for the inherited feature range.

This is a Task with verification/correction deliverables, not permission for a review-role Assignment to edit code. Corrections are performed only under this Task's Implementation authority; bound Review Implementation, Review Coding Standards and Publication roles retain their existing read-only/publication boundaries.

## Scope

- Audit AC1–22 against actual public behavior, focused regression tests and the full feature range. Use `review-implementation` and `review-coding-standards` workflows as applicable; include the explicit inherited range in every review handoff.
- Demonstrate the incident regression, successful same-Check checkpoint continuation, atomicity/replay, cleanup uncertainty, both limit boundaries, persistent counters/reset audits, clean salvage and dirty/untracked salvage.
- Recheck independent ownership, authorization, freshness, Workspace/Project identity and capacity safety; closed capability replay must not restore mutation authority.
- Confirm failures and ordinary resume do not bypass continuation exhaustion; rejected changed interruption is not an accepted result; recovery never silently converts unreported work into historical Continuation events.
- Verify storage/wire compatibility and isolated explicit upgrade paths. Report unsupported migration or rollout capabilities honestly rather than inventing one or editing persistence directly.
- Verify documented examples/profile artifacts and glossary against implemented contracts. Check that no live operations were bundled into implementation.
- Correct in-scope gaps and add regression tests in this Task's Implementation attempt. Material design contradictions return to the human. Do not claim passing review with unresolved blockers.
- Commit the verification report at `docs/specs/implementation-continuations-verification.md`, including per-criterion evidence, test commands/results, unresolved failures/unrun gates, full feature range and separate operational handoff.

## Observable acceptance and verification

1. Every AC has a pass/fail/unverified entry with precise observable evidence. All claims distinguish executed tests from recommendations; remaining required failures prevent a completion claim.
2. Tests reproduce the old interruption/evidence bug and exercise new continuation, bounds and recovery through public boundaries, including rejected/uncertain/crash/restart cases.
3. Full default-parallel workspace gates pass, or exact failures and unrun gates are reported without asserting completion. Include profile/documented-command checks and any applicable skill portability gates.
4. Independent implementation and coding-standards review cover the inherited feature range and this Task's corrections. Reviewer-owned Findings are verified by their originating Checks, not self-cleared by Implementation.
5. The report preserves actual source/input/output provenance, compatibility assessment and approvals needed for rollout; it states that existing Check pins do not change when profile source changes.
6. No revision activation, live state migration, profile registration/selection, Check repinning, #37 recovery/dispatch, merge, force-push, branch reset or parent-Spec completion occurred.

If no code corrections are needed, unchanged repository output is valid until the verification report is committed; do not manufacture empty implementation commits or PRs. Publication follows actual changed-versus-unchanged authoritative Stack evidence and configured Checks.

## Implementation evidence

- Status: complete (local source verification/correction delivery only).
- Input: `06ea109f531897634e9d16ebac20e3393732d24b` (T06 verified handoff),
  in the user-authorized checkout; no managed Assignment was created.
- Inherited full review range:
  `b1f900a26fb2c3f74245e0f10e94b1a970732325..06ea109f531897634e9d16ebac20e3393732d24b`.
  T01 actually used an authorized checkout, not a prepared managed root; this
  provenance is retained rather than invented.
- Correction output: `4d3927095f26a7c06f7765a1332cc266bd7a4754`, followed by
  Stack fixture correction `04e867eacb416335d46c55a81aeb308df6640fcf`.
  Final executable review range:
  `b1f900a26fb2c3f74245e0f10e94b1a970732325..04e867eacb416335d46c55a81aeb308df6640fcf`.
- Verification report output: `58aa0b7bc42db671032bed94f90c3fe2e70742ee`,
  [full report](../implementation-continuations-verification.md). It maps AC1–22
  to named public-boundary tests, records red/green evidence, review results,
  compatibility and separate operational handoff. This completion entry is
  metadata after that verified output, not a substitute for the feature range.
- Independent implementation and coding-standards reviews identified four gaps:
  actual Repository identity for checkpoints, config/index-hidden dirty work,
  clean salvage identity revalidation, and intent-to-add fingerprint binding.
  Corrected all four with regressions. Both follow-up reviews and the final
  fixed-range/report review found no remaining actionable implementation findings
  or introduced documented-standard violations. No managed Checks/Findings exist
  for this local plan, and none were invented or self-cleared.
- Shared observation now validates Repository/branch/Base Commit; Git configuration
  cannot hide untracked work and index trust flags require dirty salvage. Clean
  and dirty pending recovery revalidate before/after preflight and match durable
  source authority in the claim transaction. V2 fingerprints include extended
  index flags without mutable stat-cache fields. Fixture updates model the
  stricter input contract rather than weakening production checks.
- Compatibility: explicit semantic schema **19**, preserving SQL/JSON/history and
  counters with isolated upgrade support for schemas 7–19. Ordinary schema-18
  opening rejects before writes. Historical v1 dirty recovery authorizations are
  preserved but fail closed; exact retries do not renew authority. There is no
  supported in-place reauthorization for such unconsumed historical recovery;
  preserve it for separately approved operator recovery design. No new wire shape
  or Management Protocol revision; immutable Check pins remain unchanged.
- Final required gates passed: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (**561 passed, 0 failed, 1 existing ignored**, default
  parallelism), and `git diff --check`. Focused profile/outcome/glossary, CLI
  documented commands, Stack/continuation execution, and skill parser/portability
  checks passed. Intermediate fixture/lint failures were corrected before the
  final full green run. The report identifies the initial zero-test parser filter
  and its corrected executed filter; zero tests were not counted as evidence.
- Unrun: existing ignored real-Pi runtime test, stochastic provider trials,
  optional release-build Python state-upgrade subprocess rehearsal, cross-platform
  crash/power-loss verification and live operational rehearsals. Local delivery
  is not release readiness. No shipped skills changed; their extra gates passed
  nonetheless.
- Blockers: none for local T07 delivery. Historical-v1 recovery limitations and
  rollout approvals are separate operational blockers, documented in the report.
  No revision installation/activation, live migration, profile source overwrite,
  registration/selection, Check repinning, #37 recovery/dispatch, merge, force-push,
  branch reset or parent-Spec completion occurred.

## Routing hints

Parent Spec test plan, all T01–T06 deliverables/tests, `CODING_STANDARDS.md`, existing full-range feature review precedents, runtime/dispatch/recovery documentation and delivered profile artifact.

## Boundaries

This final Task verifies delivery, not release readiness or authorization for operations. Stop after evidence/report delivery and configured completion gates; no implementation of #37's unrelated Base Branch Policy feature is included.
