# To Tasks

Break a published spec into its full Task plan once: tracer-bullet vertical slices ordered as one linear cumulative Builds-On chain. The request authorizes Task creation and validated Spec start; no further approval or release command is needed. Dispatch and implementation are separate workflows.

Examples use illustrative IDs and selectors; substitute inspected values and preserve Workspace selection.

## 1. Require an unplanned Open Spec

Resolve the existing Spec from the request or unambiguous conversation context. Use the Forge skill to select CLI, endpoint and Workspace, then inspect the Spec, Comments, Update Log and existing Tasks:

```sh
forge record show 42 --json
forge record comments 42
forge record updates 42
forge record list --project personal/alpha --type task --json
```

Confirm publication, Open status and the Spec's single Project. Inspect direct child parentage, including Done Tasks. If the target or scope is ambiguous, ask before writing.

**If any child Task exists, refuse creation.** Report existing children. Additions, replanning and partial-plan recovery are outside this one-shot mode. Return material Spec contradictions or missing decisions to grilling rather than redefining the outcome.

## 2. Resolve the Stack destination and preflight

Resolve the GitHub repository, push remote and starting branch from explicit context and repository observations. A feature or release branch is valid; the default branch is not an implicit choice. Resolve ambiguity with the human before creating Tasks.

Run the read-only Check Template preflight for the Spec's Project:

```sh
forge project check-template preflight personal/alpha --json
```

It exits 1 when `ok` is false or the server is too old to report blockers. Then report the `blockers` and stop: the operator must fix the template or profiles. This mode never edits templates, registers or selects profiles, or repins Checks. On success, record `version`; every Task creation in step 4 passes it so each Task opens with exactly the Checks you checked. Preflight covers configuration only, not credentials, hosting or provider readiness.

## 3. Explore and draft the plan

Use the Spec and conversation as authority. If you have not already, explore the codebase to understand its current state; use the domain glossary vocabulary and respect ADRs in the area you're touching. Look for prefactoring that makes the work easier: "Make the change easy, then make the easy change."

Break the work into **tracer-bullet** Tasks:

<vertical-slice-rules>

- Each slice cuts a narrow but COMPLETE path through every layer (schema, API, UI, tests): vertical, NOT a horizontal slice of one layer
- A completed slice is demoable or verifiable on its predecessor's result
- Each slice is sized to fit in a single fresh context window
- Any prefactoring comes first

</vertical-slice-rules>

**Wide refactors are the exception to vertical slicing.** A **wide refactor** is one mechanical change (rename a column, retype a shared symbol) whose **blast radius** fans across the whole codebase, so no vertical slice can land green. Sequence it as **expand–contract** within the chain: expand (add the new form beside the old so nothing breaks), then one Task per migration batch sized by blast radius (per package, per directory), then contract (delete the old form once no caller remains). Each Task inherits the previous one's commits, so the cumulative Stack is the integration branch; if a batch cannot stay green alone, say so in its body and promise green only at a final integrate-and-verify Task.

Plan shape:

- Order **all** Tasks into one Builds-On chain, including research, review, verification and other Tasks that consume repository code without editing it. Unchanged output still runs every required Check, including Publication.
- Describe deliverables in ordinary Task bodies; there are no Task-kind flags.
- Assign proposed glossary/ADR changes to their owning slices.
- For code changes, include an integration/local Verification Task after implementation and review unless the user explicitly opts out. Add required release/handoff work after verification.

Each body provides a descriptive outcome, context and settled decisions, scope boundaries, routing hints, numbered Spec criteria, observable acceptance checks, verification/evidence and its immediate predecessor. Require the predecessor's exact verified final commit as input, not the then-current Project Root. Repository inheritance transfers commits only: external files, untracked artifacts, services, credentials and other state need explicit handoff contracts. Avoid file paths and code snippets that go stale, unless a prototype snippet encodes a decision more precisely than prose (state machine, schema, type shape).

`builds-on` declares immediate code inheritance and predecessor completion. Use `waits-for` only for a completion-only prerequisite whose repository result the dependent does not consume, and document why in the body:

```sh
forge record relate 57 --kind waits-for --to 49
```

Prose or missing Stack configuration is not a reason for `waits-for`. Forks or multiple code prerequisites need an explicit integration decision before writing, not a guessed predecessor. Finish drafting only when every Spec criterion and domain change is covered and the plan has one root and one immediate predecessor per successor.

## 4. Publish in order

Immediately before each write, recheck the Spec's child membership; refuse if another planner has created children. Create each Task under the Spec with its Markdown body on stdin and the preflight version:

```sh
forge task under-spec --spec 42 --expected-template-version 5 --body - --json
```

The server rejects the Task, creating nothing, with `stale-version` if the template was replaced since preflight or `check-template-blocked` if it is no longer Stack-ready. Treat either as a configuration change: stop and report.

Use actual returned IDs, even when non-consecutive; never assume `N-1`. Configure the first Task as Stack root and declare every later Task as building on the immediately preceding one, including unchanged code consumers. Create and configure each Task before creating the next. For example, if the root returned 44 and the next Task returned 49:

```sh
forge task stack root 44 --repository acme/widgets --remote origin --base feature/payments
forge record relate 49 --kind builds-on --to 44
```

Creation, relationships and root configuration are separate operations. On any failed write, unexpected child, validation error or concurrency conflict, stop. Report exact created IDs, Checks, relationships, Stack state and incomplete operations. Leave the incomplete plan Open; do not dispatch, blindly retry creation, delete children or repair through this mode.

## 5. Read back, start and report

Read every Task, its Checks and relationships. Verify exact direct child membership, criterion coverage and the immediate Builds-On chain without forks. Inspect every Stack member:

```sh
forge task stack show 44 --json
forge task stack show 49 --json
```

Require the resolved root destination and immediate Stack Predecessor for every successor, including unchanged Tasks; verify Waits-For prerequisites separately. Once the complete plan matches, invoke validated start:

```sh
forge record start 42
forge record show 42 --json
```

Start transactionally validates the persisted plan, including every Task's Checks, before Open → In Progress. On failure, report the diagnostic and read back the actual state; validation failure leaves the Spec Open. Do not describe a concurrent actor's changes as your success. On successful start, report ordered IDs and outcomes, prerequisites, the derived Stack, the template version used and the Spec status. Stop: the planner does not dispatch agents, prepare worktrees, publish PRs or manage production rollout.
