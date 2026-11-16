# To Tasks

Create a published Open Spec's full Task plan once, as one linear cumulative chain. The request authorizes Task creation and validated Spec start; no further approval or release command is needed. Dispatch and implementation are separate workflows.

## 1. Require an unplanned Open Spec

Resolve the existing Spec from the request or unambiguous conversation context. Use the Forge skill to select CLI, endpoint and Workspace, then inspect the Spec, Comments, Update Log and relationships:

```sh
forge record show 42 --json
forge record comments 42
forge record updates 42
forge record list --project personal/alpha --type task --json
```

Substitute inspected values in every example and preserve Workspace selection. Confirm publication, Open status and the intended single Project. Inspect direct child parentage, including Done Tasks. If the target or scope is ambiguous, ask before writing.

**If any child Task exists, refuse creation.** Report existing children. Additions, replanning and partial-plan recovery are outside this one-shot mode. Return material Spec contradictions or missing decisions to grilling rather than redefining the outcome.

## 2. Resolve and preflight before the first Task write

Resolve the GitHub repository, push remote and starting branch from explicit context and repository observations. A feature or release branch is valid; the default branch is not an implicit choice. Resolve ambiguity with the human before creating Tasks.

Inspect the Project Check Template and run the read-only preflight:

```sh
forge project check-template show personal/alpha --json
forge project check-template preflight personal/alpha --json
```

Require a successful preflight from a supporting server. It checks Implementation first, Publication last and registered Task profile launch policies for every Check of future default Dispatched Tasks. Record the observed template version. This is configuration compatibility, not proof of credentials, hosting availability, provider readiness or a reservation against concurrent template replacement. Missing/invalid configuration stops before Task creation and requests operator action. This planning mode never rewrites templates, registers/selects profiles or repins existing Checks. Recheck the version before writing; inspect each created Task's actual immutable Checks and stop on drift.

## 3. Draft the full cumulative plan

Use the Spec and conversation as authority; inspect the codebase, domain glossary and relevant ADRs as needed.

- Cover the entire outcome with narrow end-to-end vertical slices. Each Task fits one fresh context window and is independently verifiable on its predecessor's result.
- Put necessary prefactoring first; sequence expand–contract work so intermediate results stay green.
- Order **all** Tasks into one dependency chain and one cumulative Stack by default, including research, review, verification and other Tasks consuming repository code without editing it. Unchanged output still runs every required Check, including Publication.
- Describe deliverables in ordinary Task bodies; there are no Task-kind flags.
- Assign proposed glossary/ADR changes to their owning slices.
- For code changes, include an integration/local Verification Task after implementation and review unless the user explicitly opts out. Add required release/handoff work after verification.

Each body provides a descriptive outcome, context and settled decisions, scope boundaries, routing hints, numbered Spec criteria, observable acceptance checks, verification/evidence and its immediate predecessor. Require the predecessor's exact verified final commit as input, not the then-current Project Root. Repository inheritance transfers commits only: external files, untracked artifacts, services, credentials and other state need explicit handoff contracts.

Default same-Spec code inheritance is configured with Stack links, not inferred from every dependency. Intentional wait-only exceptions use persisted dependency-level intent:

```sh
forge record relate 57 --kind ordering-only-depends-on --to 49
```

Use that only when the dependent does not consume the prerequisite's repository result; document why. Prose or missing Stack configuration is not an exception. Forks or multiple code prerequisites need an explicit integration decision before writing, not a guessed predecessor. Finish drafting only when every Spec criterion and domain change is covered and the default plan has one root and one immediate predecessor per successor.

## 4. Publish in order

Immediately before writing, recheck authoritative child membership and the template version. Refuse if another planner has created children or configuration changed. Create each Task under the Open Spec with its Markdown body on stdin:

```sh
forge task under-spec --spec 42 --body -
```

Use actual returned IDs, even when non-consecutive. Configure the first Task as root and every later Task after the immediately preceding Task, including unchanged code consumers. For example, if the root returned 44 and the next Task returned 49:

```sh
forge task stack root 44 --repository acme/widgets --remote origin --base feature/payments
forge record relate 49 --kind depends-on --to 44
forge task stack after 49 --predecessor 44
```

Stack configuration also creates the dependency atomically; explicit dependency readback must still match the complete plan. Record IDs are opaque, never `N-1`. Create and configure each Task before creating the next.

Task creation, dependencies and Stack configuration are separate operations. On any failed write, unexpected child, incompatible immutable Check, validation error or concurrency conflict, stop. Report exact created IDs, Checks, relationships, Stack state and incomplete operations. Leave the incomplete plan Open; do not dispatch, blindly retry creation, delete children or repair through this mode.

## 5. Read back, start and report

Read every Task, its Checks and relationships. Verify exact direct child membership, criterion coverage and the immediate dependency chain without forks. Inspect every Stack member:

```sh
forge task stack show 44 --json
forge task stack show 49 --json
```

Require the resolved root destination and immediate Stack Predecessor for every successor, including unchanged Tasks; verify persisted ordering-only exceptions separately. Once the complete plan matches, invoke validated start:

```sh
forge record start 42
forge record show 42 --json
```

Start transactionally validates the persisted plan before Open → In Progress. On failure, report the diagnostic and read back the actual state; validation failure leaves Open. Do not describe a concurrent actor's changes as your success. On successful start, report ordered IDs/outcomes, dependency chain, cumulative Stack, template observation and parent status. Stop: the planner does not dispatch agents, prepare worktrees, publish PRs or manage production rollout.
