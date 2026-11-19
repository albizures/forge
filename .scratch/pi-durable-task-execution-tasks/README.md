# Pi-durable Task Execution — Filesystem Task Plan

Parent Spec: [Headless Task Execution with Pi-durable](../pi-durable-task-execution-spec.md).

## Status and boundaries

This is the complete one-shot filesystem plan for the confirmed Spec, following the `spec/to-tasks.md` guide with publication deliberately replaced by local Markdown artifacts. No Forge Records, dependency relationships, worktrees, profile selections or live Stack have been created. T01–T20 are local planning references, not opaque Forge Record IDs. The parent Spec is unchanged.

Every Task is a direct planned child of the parent Spec. Each Task is sized as a focused fresh-context deliverable, includes its local acceptance checks and evidence, and builds on its immediate predecessor. Read the parent Spec and the Task body before execution; predecessor implementation/research evidence supplies the concrete interfaces chosen within the approved constraints.

All Tasks form **one ordinary linear dependency chain**. There is no parallel frontier:

```text
T01 → T02 → T03 → T04 → T05 → T06 → T07 → T08 → T09 → T10
    → T11 → T12 → T13 → T14 → T15 → T16 → T17 → T18 → T19 → T20
```

Arrows mean execution order: the Task on the right depends on the Task on the left. Explicit per-Task edges appear below and in each Task body; these are filesystem declarations, not claimed Forge relationships.

## Completion checklist

Mark a Task complete only after its acceptance checks and required verification evidence are satisfied. All Tasks are initially unchecked; creating the plan does not complete them.

- [x] [T01 — Validate runtime contracts](01-validate-runtime-contract.md) — [contract and evidence](../../docs/research/pi-durable-t01/README.md): 19 Node probes and 4 Linux host probes pass; the stock read-only opening limitation is resolved by an explicit pinned derived read adapter. Research complete, not production/release readiness.
- [x] [T02 — Separate provider wiring](02-separate-provider-wiring.md) — provider-neutral factory/capability-delivery seam; Herdr guards retained; alternate-provider and fail-closed tests pass. All four repository gates pass (565 tests passed, 1 opt-in probe ignored); durable launch remains disabled.
- [ ] [T03 — Persist runtime admission](03-persist-runtime-admission.md)
- [ ] [T04 — Build the single-owner runner](04-build-owned-runner.md)
- [ ] [T05 — Enforce authorization leases](05-enforce-authorization-leases.md)
- [ ] [T06 — Map profiles and authentication](06-map-profiles-and-authentication.md)
- [ ] [T07 — Deliver Check execution tracer](07-deliver-check-execution-tracer.md)
- [ ] [T08 — Block interrupted effects](08-block-interrupted-effects.md)
- [ ] [T09 — Retire supervised execution](09-retire-supervised-execution.md)
- [ ] [T10 — Resume authorized Assignments](10-resume-authorized-assignments.md)
- [ ] [T11 — Inspect status and transcripts](11-inspect-status-and-transcripts.md)
- [ ] [T12 — Deliver bounded follow-ups](12-deliver-bounded-followups.md)
- [ ] [T13 — Migrate effective Check pins](13-migrate-effective-check-pins.md)
- [ ] [T14 — Manage retained runtime storage](14-manage-retained-runtime-storage.md)
- [ ] [T15 — Package the matched runner](15-package-matched-runner.md)
- [ ] [T16 — Gate mixed-backend rollout](16-gate-mixed-backend-rollout.md)
- [ ] [T17 — Document domain and operations](17-document-domain-and-operations.md)
- [ ] [T18 — Review implementation and standards](18-review-contract-and-standards.md)
- [ ] [T19 — Verify integration and recovery](19-verify-integration-and-recovery.md)
- [ ] [T20 — Deliver release handoff](20-publish-release-handoff.md)

## Ordered Tasks and explicit dependency edges

| ID | Task body / delivered outcome | Depends on | Planned code Stack predecessor |
| --- | --- | --- | --- |
| T01 | [Validate exact upstream/runtime contracts and probes](01-validate-runtime-contract.md) | None | Not a Stack member |
| T02 | [Separate Herdr-specific provider wiring without behavior regression](02-separate-provider-wiring.md) | T01 | Stack root |
| T03 | [Persist runtime identity, intent and idempotent admission](03-persist-runtime-admission.md) | T02 | T02 |
| T04 | [Run one persistent conversation under exclusive process ownership](04-build-owned-runner.md) | T03 | T03 |
| T05 | [Gate all execution with Forge authorization and expiring leases](05-enforce-authorization-leases.md) | T04 | T04 |
| T06 | [Validate explicit profiles, approved resources and authentication](06-map-profiles-and-authentication.md) | T05 | T05 |
| T07 | [Deliver a Check-bound dispatch/bootstrap/Outcome tracer](07-deliver-check-execution-tracer.md) | T06 | T06 |
| T08 | [Block unsafe interrupted effects and blind replay](08-block-interrupted-effects.md) | T07 | T07 |
| T09 | [Revoke, stop and reconcile durable execution conservatively](09-retire-supervised-execution.md) | T08 | T08 |
| T10 | [Recover the same authorized attempt and distinguish Continuations](10-resume-authorized-assignments.md) | T09 | T09 |
| T11 | [Expose read-only status and paginated committed transcripts](11-inspect-status-and-transcripts.md) | T10 | T10 |
| T12 | [Deliver recorded follow-ups without rebinding work](12-deliver-bounded-followups.md) | T11 | T11 |
| T13 | [Migrate effective Check pins with history and freshness invalidation](13-migrate-effective-check-pins.md) | T12 | T12 |
| T14 | [Retain protected history and safely delete eligible runtime storage](14-manage-retained-runtime-storage.md) | T13 | T13 |
| T15 | [Package and attest a matched runner with Forge Revisions](15-package-matched-runner.md) | T14 | T14 |
| T16 | [Gate complete scheduling, mixed backends and rollback](16-gate-mixed-backend-rollout.md) | T15 | T15 |
| T17 | [Complete domain records and verifiable operator documentation](17-document-domain-and-operations.md) | T16 | T16 |
| T18 | [Review implementation and coding standards independently](18-review-contract-and-standards.md) | T17 | Not a Stack member |
| T19 | [Verify packaged integration, fault recovery and real-provider handoff](19-verify-integration-and-recovery.md) | T18 | Not a Stack member |
| T20 | [Deliver revision-specific readiness and rollout/rollback handoff](20-publish-release-handoff.md) | T19 | Not a Stack member |

## Expand–contract and verification order

- T01 resolves API/host mechanism facts without changing settled policy. A material incompatibility blocks successors and returns for decision.
- T02 expands the existing adapter seam while Herdr behavior remains green.
- T03–T06 build identity, ownership, authorization and explicit configuration before real dispatch integration.
- T07 provides the narrow end-to-end tracer; default dispatch and automatic recovery remain disabled at that stage.
- T08–T10 establish interrupted-effect, retirement and recovery safety before automatic recovery becomes usable.
- T11–T15 deliver required operator controls, explicit migration and matched packaging.
- T16 integrates complete sequencing and a release gate, not premature default rollout.
- T17 completes domain and operational contracts before independent review.
- T18 reviews the completed snapshot; T19 is the explicit post-implementation integration/local Verification Task, including real-provider release evidence.
- T20 is the handoff only. It neither enables live dispatch nor treats failed/unrun gates as readiness.

Review or verification findings block advancement. Corrections must meet the responsible implementation deliverable, be reviewed on the corrected snapshot and renew affected verification; no policy weakening, extra plan branches or silent additions are authorized.

## Planned code Stack and later publication prerequisites

Code/documentation Stack declaration: **T02 → T03 → … → T17**. Each member Builds-On the immediately preceding code member. T02 is the proposed root. Ordinary predecessor dependencies still apply to every Task, including T01 and T18–T20.

Read-only repository inspection found Git remote `origin` at `git@github.com:albizures/forge` and the checkout on `main`. These are observations, **not an operator-selected publication destination or starting branch**. No Stack root destination has been invented.

Before any future Forge publication:

1. Resolve/confirm the owning Workspace and single Project through the installed public CLI and inspect current Spec/child membership. Existing children make the guide's one-shot publication mode inapplicable.
2. Confirm the GitHub repository, push remote and Stack starting branch explicitly. Do not assume the observed `main` is the intended base.
3. Inspect the Project Check Template. Its final Check must be `Publication`, after Implementation/reviews, with valid pins for Dispatched execution. The planning workflow must not rewrite that template or existing pins.
4. Create real direct-child Tasks and immediate-predecessor dependency edges using returned Record IDs, never arithmetic on IDs. Configure/read back the actual root destination and code Stack links separately.
5. Preserve the Spec lifecycle. This local plan does not claim existing published parentage or authorize execution, dispatch, PR creation, merge or rollout.

The filesystem request does not require a live Project lookup or mutate live template/configuration state.

## Acceptance-criteria coverage

AC numbers refer to the parent Spec. T18 reviews every criterion and T19 verifies every criterion; owners below identify implementation delivery, not a claim that tests already passed.

| Spec AC | Required outcome | Primary delivery Tasks |
| --- | --- | --- |
| AC1 | Headless dispatch, correct cwd, no Herdr prerequisite | T02, T07, T16 |
| AC2 | Forge lifecycle/Check/evidence authority | T02, T07, T10, T13, T16 |
| AC3 | Protected bootstrap and authenticated Outcomes | T06, T07 |
| AC4 | Per-Assignment process/conversation/store and fresh successors | T03, T04, T07, T10, T16 |
| AC5 | Versioned idempotent protocol and explicit unknown outcomes | T01, T03, T04, T08, T12, T15 |
| AC6 | Exclusive host ownership before writable open | T01, T03, T04, T14 |
| AC7 | Safe authorized same-attempt recovery | T05, T08, T10 |
| AC8 | Bounded authorization leases and expiry cancellation | T01, T05, T10 |
| AC9 | No arbitrary command replay; uncertain-effect inspection | T01, T08, T10 |
| AC10 | Preserve worktree bytes and retained-work rules | T08, T09, T10, T14 |
| AC11 | Revoke before cleanup; verify ownership and absence | T09, T16 |
| AC12 | Persist blockers/capacity for closed uncertain work | T03, T05, T08, T09, T10, T11, T16 |
| AC13 | Missing Outcome stays unresolved | T07, T10, T11, T12, T16 |
| AC14 | CLI/RPC read-only status and committed pagination | T04, T11, T14 |
| AC15 | Recorded, idempotent, scope-preserving follow-ups | T12 |
| AC16 | Protected retention and explicit safe deletion | T14 |
| AC17 | Explicit compatible profiles; historical cleanup independent | T01, T02, T06, T07, T13, T15, T17 |
| AC18 | Deliberate provider authentication separate from capabilities | T01, T06, T15, T17 |
| AC19 | Matched installed components and compatibility preflight | T01, T15, T17, T20 |
| AC20 | Explicit effective-pin migration with immutable history | T13, T16, T17, T20 |
| AC21 | Original-backend cleanup, staged rollout and rollback | T02, T09, T13, T15, T16, T17, T20 |
| AC22 | Retained deterministic and real-provider verification gate | T16, T18, T19, T20 |
| AC23 | Truthful operational/security documentation | T11, T17, T19, T20 |

## Domain-change ownership

Targets are the actual context/ADR documents named in the Spec; no glossary/ADR is applied by creating this plan.

| Proposed change | Delivery owner |
| --- | --- |
| Add Assignment Authorization Lease glossary definition | T05 |
| Add Assignment Execution Recovery glossary definition | T10 |
| Change Check and Agent Profile glossary definitions | T13 |
| Change Forge Workspace and Forge Dispatch Workspace definitions | T17 |
| ADR-0012: headless durable execution boundary, isolation and separate persistence | T17 |
| ADR-0013: authorized recovery and conservative retirement | T05 begins; T09/T10 complete; T17 reconciles |
| ADR-0014: explicit backend/effective-profile migration | T13; T17 reconciles |
| Preserve Task, Continuation and Assignment Session meanings; qualify execution tasks | T10, T17 |

ADR numbers must be rechecked against the repository when applied. All glossary changes belong in the current runtime context, not a competing new glossary.

## Shared execution expectations

- Keep changes capability-focused and obey `CODING_STANDARDS.md`, especially policy ownership, atomic history/replay, external-call boundaries, retained uncertainty, secret handling and compatibility rules.
- Each implementation Task adds focused behavioral/failure tests and records actual results. Do not defer every safety assertion to T19.
- Use isolated databases, listeners, environment and credentials, fake clocks and explicit synchronization. Distinguish protocol fakes from real-process and real-provider evidence.
- T19 runs the required repository gates, established runner tests/build and packaged integration. Any unrun/failed release gate is a blocker, not assumed coverage.
- A future execution process may use Forge's own required Checks and publication workflow; local T references and this proposed Stack do not bypass them.
