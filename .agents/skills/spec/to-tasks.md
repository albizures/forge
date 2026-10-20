# Plan Tasks

Break an approved plan, Spec, or conversation into Forge **Tasks** under an existing Spec. Use tracer-bullet vertical slices, preserve blockers, and add verification/release work for code changes. Existing non-worktree Task execution is the default; use worktree-backed implementation mode only when the human explicitly opts the confirmed Spec into it during this planning workflow.

## Required parent Spec

Require an existing Forge Spec. If the user did not supply a Spec id/URL and one is not unambiguously discoverable, stop and ask for it. Inspect the candidate with `forge show <record>` and confirm it is a Spec before drafting or publishing child Tasks.

Do not use Task planning to backfill an unconfirmed definition. Spec definition remains separate: defining a Spec creates exactly the Spec unless the human explicitly asked to define and plan in one request.

## Process

### 1. Gather context

Use the existing conversation. For a supplied Spec, fetch it with `forge show` and `forge history` and read the body/history needed. Explore the codebase when necessary; use project glossary vocabulary and respect relevant ADRs.

Look for prefactoring opportunities: make the change easy, then make the easy change.

### 2. Draft vertical slices

Break the work into tracer-bullet Tasks:

- Each slice cuts a narrow but complete path through every relevant layer.
- Each completed slice is demoable or verifiable on its own.
- Each slice fits in a single fresh context window.
- Prefactoring comes first.

Give each Task its blocking edges. Wide refactors are the exception: sequence them as expand–contract, using an integration branch only when batches cannot stay green independently, and make final integration/verification explicit.

### Worktree-backed implementation opt-in

Worktree-backed implementation is an optional planning decision for an existing confirmed Spec, not a replacement for ordinary `/spec` execution.

Use worktree mode only when the human explicitly chooses it for the Spec (for example, asks to plan the Spec in worktrees, use Forge worktrees for implementation Tasks, or keep the main checkout clean with task PRs). If the human does not opt in, preserve the existing non-worktree execution path and do not add worktree-specific gates.

When a Spec is opted into worktree mode:

- Plan first-round ordinary implementation Tasks before any worktree-backed implementation starts.
- Keep the definition/planning/execution phase boundary intact: planning creates or confirms child Tasks; implementation happens later through direct child Task execution.
- Treat worktree mode as execution metadata and routing guidance for ordinary implementation Tasks. Do not imply child Tasks during Spec definition.
- Include explicit verification/release work as needed so the later worktree flow has a known integration and completion path, including final integration PR and completion gates.
- Ensure the final integration/release work verifies that the integration branch contains exactly accepted task PR results plus explicit integration/conflict-resolution commits, excludes rejected or abandoned work, targets the final integration PR at the branch from which the Spec was created, renders the integration PR body from Forge record data, and confirms required verification before completion.
- Plan task branches using `worktree-branch-planning.md`: each round starts from the current integration branch, independent Tasks base/target that integration base, linear dependencies form stacked task PR branches, and diamonds use explicit internal join branches.
- Retain the branch-plan metadata required by `worktree-branch-planning.md` before implementation starts.

### Worktree round and follow-up gates

Use these gates only for a Spec already opted into worktree mode:

- Refuse to plan or start a next round while any previous-round task PR remains open. Report the open PRs and wait for explicit human resolution.
- Before proposing next-round Tasks, read the previous round's PR outcomes and all available review comments.
- Before proposing or creating follow-up Tasks, validate the next-round integration branch with `git`, `gh`, and local branch inspection: it must contain exactly the accepted merged task PR results plus explicit integration/conflict-resolution commits, and it must exclude rejected, closed, or abandoned task PR work.
- If the integration branch has unexpected commits, is missing accepted work, includes rejected/closed/abandoned work, or has an unverified tip, stop before follow-up Task planning and report the reconciliation needed.
- Treat merged PRs as accepted input to integration; treat closed, rejected, or abandoned PRs as explicit human decisions and exclude their work from integration unless the human confirms replacement work.
- Do not create a follow-up Task solely because a PR has no available review comments.
- Propose follow-up Tasks only for meaningful feature gaps, review feedback, replacement work, or integration needs.
- Present next-round follow-up Tasks for human confirmation before creating them in Forge.
- Preserve immutable task PRs after creation: later feature or review changes belong to confirmed follow-up Tasks, not updates to the existing task branch.
- Fail before PR creation if the round dependency graph cannot be represented as an acyclic branch plan with explicit join branches where needed.

### 3. Add verification Tasks

For code work, add ordinary Forge Tasks by default unless the human explicitly opts out:

- **Verification Task**: state the integration/local verification to run.
- **Release Task**: state the release handoff required.

Gate verification after implementation/review and release after verification. If follow-up implementation or review Tasks are added after verification, add another Verification Task before release.

### 4. Quiz the user

Present the proposed breakdown as a numbered list. For every Task show its title, kind, blockers, delivered end-to-end behavior, and routing hint. Ask whether the granularity, edges, kinds, and routing are right. Iterate until approved; do not publish unapproved Tasks.

### 5. Publish

Create blockers before dependents with `forge new task`. Use `--description -` or `--description-file` for long descriptions. Use research, prototype, or review kinds only when approved and semantically right. Preserve every approved dependency with repeatable `--depends-on` or `forge deps add`.

Report every created id. Work the frontier through `forge ready`; do not close the parent Spec.
