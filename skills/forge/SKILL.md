---
name: forge
description: "Operate Forge records, project scope, relationships, lifecycle, managed worktrees, agent dispatch, and assignment recovery through the Forge CLI."
---

# Forge workflow

## Select the CLI and endpoint

Use the installed `forge`. When the task identifies an operation, inspect that subcommand's `--help` directly. For discovery within a known area, use recursive subtree help; use full recursive help only for broad discovery:

```sh
forge record list --help
forge help workspace --all
forge help --all
```

Recursive help accepts nested command paths. If an older revision lacks recursive help, fall back to ordinary subcommand `--help`. If the CLI or a required operation is missing, stop and request installation or an appropriate update. Switching to a source-built CLI is a separate, explicit Forge-development action.

Ordinary commands use Forgeup's Active Forge Revision. Use `--server-url` or `FORGE_SERVER_URL` only when an explicit endpoint override is intended. Forgeup is a separate program, not a Forge subcommand.

## Establish scope before writing

```sh
forge here
```

Select the Forge Workspace explicitly with `--workspace` or infer it from canonical client cwd. Outside registered roots, use an explicit Workspace or qualified Project selector; bare Record IDs do not select a Workspace. Confirm the intended Project before creating project-scoped work. If it cannot be determined, ask the human to select or register it. Creation commands require explicit Project or parent arguments where applicable; `here` does not supply implicit creation scope. Use a scope-free generic Record or standalone Task only when work is deliberately outside a Project.

- Spec: exactly one Project; may parent Tasks.
- Initiative: Workspace-owned coordination Record with zero or more Projects; Specs and Wayfinders may attach to it without becoming children.
- Wayfinder: zero or more Projects in one Workspace; may parent Tasks and Grillings.
- Task: leaf work, standalone or under a Spec/Wayfinder. A Project-scoped Wayfinder Task selects exactly one of its Projects; a Projectless Wayfinder Task remains Projectless; a Spec Task inherits its Project.
- Grilling: human-in-the-loop leaf child of a Wayfinder; inherits its Project Scope unless a non-empty subset is selected. A Projectless parent supplies empty scope.
- Generic Record: explicit Project Scope, including deliberately empty scope.

Use the Record Body for Markdown work content, including research, prototype, and review deliverables. Creation supports `--body -` for stdin; titles are not separate flags. Scope and typed parent membership are fixed when opened.

## Operate records

1. Inspect the Record, its Comments, and Update Log before changing it.
2. Preserve dependencies with `record relate --kind depends-on`; `--to` names the prerequisite. `generated-by` records provenance, not a dependency or parent.
3. For manual work, advance Open → In Progress with `record start`. An assigned Task or Grilling instead starts through its assignment bootstrap below.
4. Record substantive outcomes with `record comment --message`. Comments are immutable; revise a non-Done Record's canonical content with `record edit --body`.
5. Complete In Progress work with `record done`. Completion Resolution is optional free text, not a required slug. Complete a parent only after its children are Done.

`record list --status open` is a Workspace-local inventory, not a readiness guarantee. Use `ready` for executable workflow state, adding `--planning`, `--include-hitl`, or `--blocked` as needed. Workflow readiness is separate from provider dispatchability. Cross-Workspace scope, membership, dependency and provenance changes are rejected. This CLI does not expose `next` or store-management commands. If a required capability is absent, report that gap rather than inventing a command or editing persistence directly.

## Assigned agents

When launched with Forge Assignment Capability Delivery, bootstrap before beginning work:

```sh
forge assignment bootstrap
```

Bootstrap reads `FORGE_ASSIGNMENT_ID`, `FORGE_ASSIGNMENT_DAEMON_URL`, and the owner-only file named by `FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE`. It authenticates, acknowledges, starts the bound Task or Grilling and fetches context before returning separately labelled behavior. If any step fails or is uncertain, stop and report the diagnostic. Keep the capability secret out of prompts, comments, shell arguments, and logs.

Work in the bound execution directory and follow the returned trusted constraints and behavior. Use `forge assignment --help` for bounded reads and own-Record actions. Task workers submit the bound Check outcome with `assignment outcome`; Grilling completion requires explicit human confirmation and the agreed outcome via `assignment done`. Bootstrap's assignment endpoint is separate from ordinary CLI endpoint selection. Preserve assignment ownership; report bootstrap failures to the operator rather than starting manually or replacing an unacknowledged agent.

## Operators

Dispatch administration, recovery assertions, setup hooks, and destructive worktree actions are operator responsibilities. Inspect command help and persisted evidence before acting. Reconciliation can launch agents; it is not a read-only check. Verify that old agents cannot act and retained resources are safe before making recovery assertions. Clearing a Dispatch Hold alone does not establish safe replacement.

### Agent Profiles

Agent Profiles configure launch policy and role-specific behavior; Skills provide reusable workflows. Profiles select zero or more Skills explicitly, never through filesystem adjacency. Forge retains execution authority and lifecycle rules; do not create wrapper Skills merely to house profiles.

Author profiles as `.md` files beneath the selected Forge Workspace's `.agents/profiles/` directory; nested folders are allowed. Preview and registration enforce canonical containment and reject symlink escapes and legacy skill-directory sources. The profiles directory and its `.agents` parent must not redirect elsewhere. Document arguments resolve from the Workspace root, while resource references resolve from the document directory and retain catalog membership rules.

Inspect `forge help agent-profile --all` before authoring or registration. Preview first, then register with explicit `--workspace` and `--record-type task|grilling`. Registration creates an immutable revision without selecting it; explicitly select the returned revision with `dispatcher config set task-profile|grilling-profile`. Inspect selections with `dispatcher profile --kind task|grilling` and provider readiness with `dispatcher status`; registration and selection do not certify dispatchability. Task execution uses Check-pinned revisions: configure the Project Check Template before opening Tasks, rather than assuming the Workspace Task selection routes existing Checks.

Existing registered revisions and Task Check pins remain usable when source files move or disappear. Changed profiles require new registration and explicit selection; Check Template changes affect future Tasks only. Do not rewrite existing pins or migrate live legacy sources until a Forge Revision supporting standalone profiles is installed and active.

Forge owns managed local worktrees, dispatch state, and explicit Task stacks.
Use `forge task stack --help` for stack configuration and `forge assignment stack`
for own-Task destination reads and publication verification. Stack branches use
`feat/forge-<record-id>`; links identify predecessors explicitly, never by ID
arithmetic. A final `Publication` Check verifies an open non-draft PR before Done;
merge is not required. Agents use `git`/`gh` to push and create/reuse PRs, while
Forge independently validates their stack evidence. Non-stacked Tasks retain
existing behavior. Integration and recovery remain operator responsibilities;
never edit Forge persistence by hand.
