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

## Routing hints

Parent Spec test plan, all T01–T06 deliverables/tests, `CODING_STANDARDS.md`, existing full-range feature review precedents, runtime/dispatch/recovery documentation and delivered profile artifact.

## Boundaries

This final Task verifies delivery, not release readiness or authorization for operations. Stop after evidence/report delivery and configured completion gates; no implementation of #37's unrelated Base Branch Policy feature is included.
