# Worktree PR Templates

Use this reference only for Specs that have explicitly opted into worktree-backed implementation mode.

Task PR and final integration PR bodies are rendered from Forge record data plus the retained branch/integration plan. Forge remains authoritative for workflow records; PR creation still uses `git` or `gh` from inside the managed worktree or integration-branch checkout.

## Required data sources

Before opening an immutable task PR, collect the PR body inputs from public Forge and local workflow state:

- parent Spec id and title
- Task id, title, and full body/description
- Task dependencies and blocker state
- retained branch-plan metadata: round number, branch kind, task branch name, base ref, PR target ref, prerequisite branch tips, and immutable-after-open state
- verification performed for this Task, including commands run and outcomes
- relevant Forge comments/outcomes from the parent Spec, the Task, dependencies, and prior review/follow-up records when they affect this PR

Do not invent missing workflow facts. If required Forge record data or branch-plan metadata is unavailable, stop before PR creation and ask for the planning/round gate to provide it.

## Body shape

Render a concise Markdown body with these sections:

1. **Forge context**: parent Spec `#<id> <title>` and Task `#<id> <title>`.
2. **Task scope**: the Task body/description summarized or quoted enough for reviewers to understand the promised deliverable.
3. **Dependencies and branch plan**: dependency ids, round, branch name, base ref, PR target, prerequisite tips, and immutable-after-open note.
4. **Implementation summary**: what changed in this task branch.
5. **Verification**: commands/checks run and their result, or an explicit reason if a check was not applicable.
6. **Forge notes**: relevant comments, review outcomes, or follow-up context that affect review.

## Final integration PR data sources

Before opening or finalizing the final integration PR, collect the body inputs from public Forge, `git`, `gh`, and retained workflow state:

- parent Spec id, title, body, and source/base branch from which the Spec implementation was created
- integration branch name, expected tip, and target branch for the final PR
- accepted task PR ids/branches/outcomes included in the integration branch
- rejected, closed, or abandoned task PR ids/branches/outcomes explicitly excluded from the integration branch
- explicit integration-only or conflict-resolution commits included in the integration branch
- verification required by the Spec and verification actually performed, including commands/checks and outcomes
- relevant Forge comments/outcomes from the Spec, implementation Tasks, review Tasks, verification Tasks, and release/handoff Tasks that affect final review

Do not open or finalize the integration PR if required integration facts are missing, if the target is not the branch from which the Spec was created, or if the integration branch contains work outside accepted task PR results plus explicit integration/conflict-resolution commits.

## Final integration PR body shape

Render a concise Markdown body with these sections:

1. **Forge context**: parent Spec `#<id> <title>` and implementation mode.
2. **Integration scope**: the accepted task PRs included and the rejected/abandoned task PRs excluded.
3. **Branch and target**: integration branch, expected tip, final PR target branch, and confirmation that the target is the Spec source branch.
4. **Integration-only changes**: explicit conflict-resolution or integration commits, or `None`.
5. **Verification**: required checks, commands/checks run, and outcomes.
6. **Forge notes**: relevant review, follow-up, verification, or handoff comments that affect final review.

## Creation boundary

Use the rendered body when opening the PR with `gh pr create --body-file <file>` or an equivalent `gh`/`git`-based flow. Do not add PR rendering, pushing, comment retrieval, integration validation, or branch orchestration responsibilities to Forge worktree commands.
