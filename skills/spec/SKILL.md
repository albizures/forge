---
name: spec
description: Define, plan, and autonomously deliver a Forge Spec through Rust Forge assignments, task rounds, review Grillings, and a human-merged release PR. Use when the user asks to spec, plan, or deliver a project outcome.
---

# Autonomous Spec workflow

Forge is authoritative. Use its public Rust CLI/API only; never edit its SQLite store or revive the retired `.agents/skills/spec` workflow. A Spec has one Project and a Markdown body (not a title). Read [the Forge Runtime vocabulary](../../crates/CONTEXT.md) before changing this workflow.

## Define

For a request that does not yet name a Spec:

1. Run `forge here`; stop if it cannot identify the registered Project.
2. Synthesize the conversation into `spec-template.md`. Keep definition behavior unchanged: include the outcome, constraints, acceptance checks, and out-of-scope items already known; do not interview merely to invent details.
3. Create exactly one Spec and report its ID:

   ```sh
   forge spec open --project <slug> --body -
   ```

Do not create Tasks in the same turn unless the human explicitly asks to plan too.

## Plan one autonomous round

For an existing Spec, first read it with `forge record show <spec-id> --json` and confirm it is an open, single-Project Spec. Reconcile its current children, dependencies, comments, and updates before planning; do not infer state from a previous wake.

Create a small, ordered round: **N ordinary implementation Tasks**, then a PR-review Grilling, optionally a manual-test Grilling, then a release Task. Each implementation Task is an end-to-end slice and names its acceptance check. Use one dependency chain when work is ordered; independent work may run in parallel only when it has no shared integration risk.

- Create every implementation and release Task beneath the Spec with `forge task under-spec --spec <spec-id> --body -`. Use `--kind research`, `prototype`, or `review` only when that is truly its work.
- Add ordering edges with `forge record relate <dependent-id> --kind depends-on --to <prerequisite-id>` before dispatch. Create prerequisite records before dependents.
- Rust Forge currently opens Grillings only beneath Wayfinders. Create a single-Project review Wayfinder with `forge wayfinder open --project <slug> --body -`, then `forge grilling open --wayfinder <wayfinder-id> --body -` for PR review and, when needed, manual testing. Their bodies must name the Spec, the PR(s), the exact review/test question, and required outcome comment. Do not claim they are Spec children or Forge dependency gates.
- Put the release Task after the implementation chain. Its body requires all review/manual-test outcomes, ordered unification, required verification, final PR creation, and a human merge. The release Task must not be marked done before that merge.

Do not create a later round while an earlier task PR is open, review/manual-test outcomes are unresolved, or the integration tip has not been reconciled. Read all available PR comments and Forge comments first. Create follow-up Tasks only for concrete findings or missing acceptance work; never modify an already-reviewed task branch.

## Dispatch, wake, and reconcile

Forge dispatches eligible open Tasks into durable Assignments and Managed Worktrees. Configure/diagnose it with the public surface only:

```sh
forge agent-dispatch status --project <slug>
forge agent-dispatch reconcile
forge worktree info <task-id>
```

An interactive review Grilling is explicitly dispatched after it is ready:

```sh
forge agent-dispatch grilling-dispatch <grilling-id>
```

A dispatched Spec Assignment may subscribe only to its direct child Tasks; a Task Assignment may subscribe only to direct prerequisites. In an Assignment Capability Delivery session, use `forge_subscribe` for those explicit record IDs, retain its cursor, and use `forge_wait_for_updates` only as a wake. After every update, timeout, connection loss, deletion/expiry, or `resync_required`, reread authoritative Forge records; renew or recreate the subscription as necessary. SSE is at-least-once and never authorizes a lifecycle transition by itself.

For a Dispatch Hold or retained/invalid worktree, stop rather than repairing it by hand. Diagnose with `forge agent-dispatch status`, `forge worktree info`, and `forge worktree doctor`; an operator may then use `forge agent-dispatch resume <task-id>` or `forge agent-dispatch assignment-retire <assignment-id>`. Only an unassigned manual in-progress Task may use `forge agent-dispatch task-reopen <task-id>`.

## Work a Task and unify in order

A Task agent works only in its Forge Managed Worktree and preserves Forge's deterministic `feat/forge-<task-id>` branch. Run focused checks, commit, push, and open one PR. Once opened, the task PR and branch are immutable: review findings become later round Tasks.

A linear dependency is an immutable PR stack: before opening dependent Task B's PR, merge its predecessor Task A's immutable branch into B's still-unreviewed branch and target B's PR at A's branch. This is the only stack-shaping merge; do not amend either branch after its PR opens. For a linear chain, merge accepted task PRs into the round integration branch in dependency order. Before each merge and before a new round, verify locally and with GitHub that the integration branch contains exactly accepted task PR commits plus explicit integration/conflict-resolution commits—no rejected, closed, abandoned, or unexpected commits. Stop and report reconciliation rather than guessing. Do not reset, rebase, delete, recreate, or repair dispatcher-created worktrees or branches.

Record meaningful outcomes with `forge record comment <id> --message <markdown>`, then complete only the delivered Task with `forge record done <id> --resolution <text>`. The parent Spec remains open during all child work.

## Review, release, and completion

Dispatch the PR-review Grilling only after its named PR set is ready. Capture findings as immutable Forge comments and create confirmed follow-up Tasks for required changes. Dispatch optional manual testing only after review acceptance. The release Task verifies the ordered unification branch, all required checks, final PR target, and final PR body, then opens the release PR. A human—not an agent—merges that PR.

After authoritative GitHub and Forge reconciliation confirms the human merge and all required child outcomes, comment the release result and complete the Spec. Never infer merge or completion from provider state, SSE, or a PR's existence.

## Boundaries

- Do not use retired commands such as `forge new`, `forge summary`, `forge tree`, or `forge start`.
- Do not create GitHub issues as Forge records, expose Assignment Capabilities, or edit Forge backing storage.
- Do not dispatch, complete, or subscribe outside the Assignment's capability and direct-record authorization.
- Do not add task PR update cycles, worktree modes, copy manifests, or internal join branches: Rust Forge owns task worktrees and task branches.
