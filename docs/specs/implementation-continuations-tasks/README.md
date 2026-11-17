# Safe Implementation Continuations — Local Task Plan

Parent Spec: [Safe Implementation Continuations and Interrupted-Work Recovery](../implementation-continuations.md).

Project: `factory/forge`. Status: complete local planning artifact; no Forge Records, dependencies, Check pins, Assignments, or Stack preparations have been created. Local IDs T01–T07 are document identifiers, not Forge Record IDs. This is the first local child-task plan for this Spec.

## Progress checklist

Check off each Task only after its deliverables and required verification are complete. Record the output commit, verification evidence, and any blockers in the Task file. Planning alone does not mark implementation complete.

- [x] **T01** — [Reject changed-commit interruption outcomes safely](01-reject-changed-interruptions.md)
- [x] **T02** — [Accept bounded Implementation Continuations end to end](02-accept-bounded-continuations.md)
- [x] **T03** — [Inspect and explicitly reset continuation budgets](03-inspect-and-reset-continuations.md)
- [x] **T04** — [Authorize salvage of clean retained commits](04-recover-clean-retained-work.md)
- [x] **T05** — [Salvage dirty and untracked interrupted work safely](05-recover-dirty-retained-work.md)
- [x] **T06** — [Deliver stopping guidance, glossary, and operator documentation](06-profiles-and-documentation.md)
- [ ] **T07** — [Verify the full feature and correct remaining gaps](07-verify-full-feature.md)

Live rollout and #37 recovery are separately approved operations, not completion items in this implementation checklist.

## Ordered plan

| ID | Task | Depends on | Planned Stack Predecessor |
| --- | --- | --- | --- |
| T01 | [Reject changed-commit interruption outcomes safely](01-reject-changed-interruptions.md) | None | Root |
| T02 | [Accept bounded Implementation Continuations end to end](02-accept-bounded-continuations.md) | T01 | T01 |
| T03 | [Inspect and explicitly reset continuation budgets](03-inspect-and-reset-continuations.md) | T02 | T02 |
| T04 | [Authorize salvage of clean retained commits](04-recover-clean-retained-work.md) | T03 | T03 |
| T05 | [Salvage dirty and untracked interrupted work safely](05-recover-dirty-retained-work.md) | T04 | T04 |
| T06 | [Deliver stopping guidance, glossary, and operator documentation](06-profiles-and-documentation.md) | T05 | T05 |
| T07 | [Verify the full feature and correct remaining gaps](07-verify-full-feature.md) | T06 | T06 |

Single chain: **T01 → T02 → T03 → T04 → T05 → T06 → T07**. All Tasks are direct local children of the same Spec. The chain is linear even where work could otherwise be independent.

## Dependency and Stack declarations

```yaml
project: factory/forge
spec: ../implementation-continuations.md
root:
  task: T01
  repository: albizures/forge
  remote: origin
  starting_branch: main
  preparation: deferred
  base_commit: not-prepared
edges:
  - {task: T02, depends_on: T01, builds_on: T01}
  - {task: T03, depends_on: T02, builds_on: T02}
  - {task: T04, depends_on: T03, builds_on: T03}
  - {task: T05, depends_on: T04, builds_on: T04}
  - {task: T06, depends_on: T05, builds_on: T05}
  - {task: T07, depends_on: T06, builds_on: T06}
```

The destination follows this Repository's observed `origin` and current `main` planning context; it is not a prepared root or remote-readiness assertion. Before managed execution, confirm the destination through supported Forge interfaces and prepare a fresh root input for this plan. Do not reuse #37's historical Base Commit, partial implementation commits, worktree, or Assignment credentials. For future Forge publication, map local IDs to actual returned Record IDs, create explicit immediate-predecessor dependencies and Builds-On links, verify all relationships, and stop on partial publication. This file alone creates no executable Forge edges.

All repository changes inherit through the planned Stack chain. Review/verification work may have unchanged output; it still must pass its configured completion Checks and publication rules. No Task bypasses Implementation, Review Implementation, Review Coding Standards, or Publication if those are configured for its execution. Actual Check pins must be inspected when Tasks are eventually opened; none are invented here.

## Shared execution contract

Each Task must read the parent Spec, this file, its own body, the actual predecessor evidence, applicable repository instructions, `CODING_STANDARDS.md`, `GLOSSARY-MAP.md`, and relevant runtime contracts before working. Spec acceptance criteria (AC) retain their numbering from the parent document.

- Work only in the authorized execution directory under valid ownership. This local plan is not dispatch or recovery authority.
- Implement each slice through its owning layers with boundary tests. Keep intermediate results green. Necessary capability-focused extraction may accompany a slice; no separate speculative refactor is required by this plan.
- Preserve immutable Stack Base Commit, accepted history, Findings, authorization, freshness, resource blockers, and existing review/publication authority.
- External Git/provider observations remain outside database writer transactions; revalidate mutable facts on commit. Exact retries are replay-safe, conflicting retries atomic failures, and secrets never enter ordinary output.
- Changes to persistent contracts require explicit compatibility assessment, schema revision where breaking, storage documentation, and supported explicit isolated upgrades. No implicit live conversion, historical event invention, or incidental counter reset.
- Commit deliverables and report actual verification and unrun gates. Run required root gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `git diff --check`. Add documented-command and portability gates when modifying shipped skills. A local pass is not release readiness.
- Do not install/activate revisions, migrate live state, register/select profiles, repin existing Checks, recover/dispatch #37, force-push, merge, reset branches, or complete the parent Spec. Isolated fixtures are permitted.
- Material contradictions return to the human rather than silently changing the Spec. Source/profile artifacts outside Git require explicit handoff; a successor does not inherit untracked or external files merely because a prior Task touched them.

## Acceptance coverage

| Spec AC | Primary delivery | Full-feature verification |
| --- | --- | --- |
| 1–8 | T02 | T07 |
| 9–11 | T02 | T03, T07 |
| 12–13 | T03 | T07 |
| 14 | T01, T02 | T07 |
| 15 | T01 | T07 |
| 16–17 | T04, T05 | T07 |
| 18 | T02, T04, T05 | T07 |
| 19 | T06 | T07 |
| 20 | Each affected slice; T06 documentation | T07 |
| 21 | Regression tests in every implementation Task | T07 |
| 22 | Every Task's execution boundary | T07 |

## Scope and review range

T07 reviews **the complete feature range from T01's prepared input to T06's verified output**, plus any T07 corrections. Its Task-local diff is not the feature review range. Capture actual full commit IDs during execution; do not substitute the moving main checkout HEAD or assume local Task numbers imply commit identity.

The local Spec and these planning documents must be explicitly included in execution input or handed off as authoritative context. Their existence here does not imply they are already committed or published.
