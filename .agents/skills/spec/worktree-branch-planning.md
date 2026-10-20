# Worktree Branch Planning

Use this reference only for Specs that have explicitly opted into worktree-backed implementation mode during Task planning.

## Round base

Each implementation round starts from the current Spec integration branch. The integration branch is the round integration base: it contains the accepted result from earlier rounds plus explicit integration/conflict-resolution commits.

Before next-round follow-up Task planning, validate the integration branch with `git`, `gh`, and local branch inspection. It must contain exactly the accepted merged task PR results plus explicit integration/conflict-resolution commits, exclude rejected/closed/abandoned task PR work, and have a known expected tip. If unexpected commits, missing accepted work, rejected work, or an unverified tip are present, stop before follow-up Task planning and report the reconciliation needed.

Do not plan task branches from the user's main checkout or from an unverified ad-hoc branch. If the integration branch or its expected tip is unknown, stop before PR creation and ask for the round gate to establish it.

## Branch shapes

Represent every ready ordinary implementation Task with one concrete branch base and one concrete PR target.

- **Independent Tasks**: base on the round integration base and target the round integration base.
- **Linear dependencies**: if Task B depends on Task A, base B on A's task branch and target B's PR at A's task branch. This forms a stacked task PR relationship.
- **Diamond dependencies**: if a downstream Task depends on multiple prerequisite task branches, create an explicit internal join/intermediate branch that merges the prerequisite tips. Base the downstream Task on that join branch and target its PR at that join branch.

Internal join branches are workflow metadata. They are not human-reviewed task PRs and must not hide meaningful product, conflict-resolution, or integration-only changes. If a join requires meaningful changes, represent those changes on the integration branch or as a named follow-up/integration Task.

## Required branch-plan metadata

Record each planned branch before implementation starts:

- branch kind: task, internal join, or integration
- owning Task id, when the branch is a task branch
- branch name
- base ref
- PR target ref, for task branches
- merged prerequisite branch tips, for internal joins or branches that incorporate prerequisites
- round number
- immutable-after-open state for task branches

Task branches become immutable after their PR is opened. Do not amend, force-push, or update them for feature or review changes; use later confirmed follow-up Tasks.

## Failure gates

Fail before task PR creation when the dependency graph cannot be represented as an acyclic branch plan. Report the cycle, missing prerequisite, ambiguous base, or unrepresentable diamond and wait for the plan to be corrected.

Fail before file changes when an executing Task lacks its branch name, base ref, PR target, round, or required copy manifest.
