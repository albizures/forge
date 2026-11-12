# To Tasks

Create a published Spec's full Task plan once, as one linear dependency chain. The request authorizes publication; there is no additional breakdown-approval checkpoint.

## 1. Require an unplanned Spec

Resolve the existing Spec from the request or unambiguous conversation context. If the target is unclear, ask for it. Use the Forge skill to select the correct CLI, endpoint, and Workspace, then inspect the Spec, its Comments, Update Log, and relationships.

Command examples use illustrative IDs and Project references; substitute inspected values and preserve Workspace selection:

```sh
forge record show 42 --json
forge record comments 42
forge record updates 42
forge record list --project personal/alpha --type task --json
```

Confirm that the parent is a published, non-Done Spec in the intended single Project. Publication is approval; no extra confirmation is needed. Inspect Task parentage, including Done Tasks, to establish whether the Spec has any direct child Tasks. Use Record details where the list does not expose the relationship.

**If any child Task exists, refuse task creation.** A second planning request, additions, replanning, and recovery of a partially published plan are outside this mode. Report the existing Tasks and explain the boundary; do not append, duplicate, or rewrite them.

If the Spec contains a material gap or contradiction that prevents planning, report it and return the unresolved decisions to grilling rather than silently redefining the Spec.

## 2. Draft the full linear plan

Use the Spec and conversation as authority. Explore the codebase as needed; use the project's domain glossary and relevant ADRs. Look for prefactoring opportunities: make the change easy, then make the easy change.

Adapt the tracer-bullet process:

- Cover the entire defined outcome with narrow, end-to-end vertical slices across the relevant layers.
- Each Task must fit a single fresh context window and deliver an independently demoable or verifiable result, building on its predecessors.
- Put necessary prefactoring first. For wide refactors, sequence expand–contract steps so intermediate results remain green.
- Order **all** Tasks into one chain: T2 depends on T1, T3 on T2, and so on, even when some work could otherwise be independent. Record explicit Forge dependency edges, not merely numbered titles or prose blockers.
- Describe research, prototype, or review deliverables in ordinary Task bodies when needed; this Forge does not use Task-kind flags.
- Assign the Spec's proposed glossary/ADR changes to the relevant slices so applying them is part of delivery.
- For code changes, include an explicit integration/local Verification Task after implementation and any planned review, unless the user explicitly opts out. State the checks and acceptance criteria it verifies. Include necessary release or handoff work after verification when the Spec requires it.

Each Task body must carry the information a fresh agent needs:

- A descriptive heading and its delivered end-to-end outcome.
- Relevant context, settled decisions, scope boundaries, and useful routing hints.
- References to the Spec's numbered acceptance criteria and the Task's observable acceptance checks.
- Verification steps and the expected evidence or deliverable.
- Its immediate predecessor, if any, and the expectation that it builds on completed predecessor results.

Keep supporting detail local to the Task rather than copying the whole Spec. Before publication, verify that every Spec acceptance criterion and required domain change is covered, every Task is verifiable, and the order is one linear plan. A dependency chain serializes workflow readiness; it does not itself integrate Git changes or guarantee conflict-free merges. Execution policy belongs outside this mode.

## 3. Publish without another approval round

Recheck authoritative child membership immediately before the first write; refuse if another planner has already created Tasks. Create Tasks in chain order under the Spec, supplying each Markdown body on stdin:

```sh
forge task under-spec --spec 42 --body -
```

After creating each Task after the first, immediately add its dependency on the actual ID returned for its predecessor before creating the next Task. For example, if the predecessor is 44 and the new Task is 45:

```sh
forge record relate 45 --kind depends-on --to 44
```

Task creation and dependency creation are separate operations. If a write fails, an unexpected child appears, or concurrent execution prevents adding an edge, stop and report the exact created IDs, relationships, and incomplete plan. Do not dispatch work or attempt recovery through this mode; any subsequent repair is outside its one-shot boundary.

## 4. Verify and report

Read back the created Tasks and relationships. Confirm that all planned Tasks are direct children of the Spec and every Task after the first depends on its immediate predecessor, with no branching.

Report the ordered Task IDs, concise outcomes, and dependency chain. Leave the Spec's lifecycle unchanged and stop; implementation is a separate request and workflow.
