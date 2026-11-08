# Command reference

Read [SKILL.md](SKILL.md) first for CLI selection, scope, and role boundaries. IDs, Project Slugs, paths, branches, and assignment identifiers below are illustrative: substitute inspected values. Commands are independent examples, not a batch workflow. Add `--json` where supported for structured output; consult source-built subcommand help for exact flags.

## Connection and Project registration

```sh
forge status --server-url http://127.0.0.1:45004/rpc --json
forge workspace add personal --root /repos
forge add project /repos/alpha alpha --workspace personal
forge here
forge here --workspace personal --project alpha --json
forge here --project personal/alpha --json
```

Registration accepts a Git main checkout as its canonical Project root, within a selected Forge Workspace. The Repository needs an `origin` remote; independent clones are distinct Repositories. Project roots may equal their Workspace root or nest inside other Project roots; duplicate canonical roots, linked Worktrees and the reserved Workspace-relative `.forge/worktrees` subtree are rejected. Project slugs are immutable and unique within their Workspace.

Project selectors use `--workspace personal --project alpha` or the qualified `--project personal/alpha`. Explicit selection wins over cwd; contradictory selectors fail. Otherwise `here` selects the deepest containing Project root. Outside registered Workspaces, supply a Workspace or qualified Project selector; there is no saved default. Examples using short Project slugs below assume cwd is within the intended Workspace (or add `--workspace`). Project output exposes `reference`, `workspace_id`, local `slug`, and `project_root`; persist the returned `reference` in scope/configuration requests. See [Project registration and selection](../../docs/projects.md).

## Forge Workspace registrations

```sh
forge workspace add personal --root /repos --name 'Personal projects'
forge workspace list --json
forge workspace show --workspace personal --json
forge workspace here
forge workspace rename personal 'My projects'
forge workspace remove personal
```

A Forge Workspace owns a fixed canonical filesystem root; it is distinct from a Herdr terminal container. IDs and roots are immutable, display names are editable, and equal or overlapping roots (including symlink aliases) are rejected. `show` and `here` prefer `--workspace`, otherwise infer from canonical cwd; outside all registered roots, supply an explicit selector. There is no saved default. Listing and registration work without selection. Removal requires an empty registration and leaves files and provider containers untouched.

Projects and every Record belong to one Forge Workspace. Record commands select it by `--workspace`, a qualified Project selector, or canonical client cwd. Bare Record IDs do not select a Workspace; outside registered roots, pass `--workspace`. Mixed-Workspace Project scopes, parentage, Initiative membership, dependencies and provenance are rejected atomically. Lists without a Project filter include only the selected Workspace. The fixed Workspace-rooted Managed Worktree layout and Workspace-owned dispatch configuration remain follow-on slices.

## Open work

```sh
forge record open --body 'Work outside a Project' --no-projects
forge record open --body 'Cross-project work' --project alpha --project beta
forge spec open --body 'Deliver the capability' --project alpha
forge initiative open --body 'Coordinate delivery' --workspace personal --project alpha --project beta
forge wayfinder open --body 'Explore the route' --project alpha --project beta
forge wayfinder open --body 'Workspace-wide discovery' --workspace personal
forge task open --body 'Deliberately scope-free investigation' --kind research
forge task open --body 'Standalone implementation' --project alpha
forge task under-spec --body - --spec 42
forge task under-wayfinder --body 'Compare alternatives' --wayfinder 43 --project alpha --kind research --generated-by 41
forge grilling open --body 'Resolve the scope decision' --wayfinder 43 --project alpha --project beta
```

Use `--body -` to read creation content from stdin. Supply stdin yourself; running that example alone waits for input. Repeat `--generated-by` for multiple generators. A multi-Project Wayfinder Task needs an explicit selected Project; a single-Project Wayfinder can supply it. A Grilling can omit Project flags to inherit its Wayfinder's entire scope. Initiatives and Wayfinders may have empty Project scope within a selected Workspace; children of a Projectless Wayfinder also have empty scope. Empty Project scope never means daemon-wide ownership.

## Inspect, edit, relate, complete

```sh
forge record show 42 --json
forge record list --project alpha --status open --type task --kind research --json
forge record comments 42
forge record updates 42
forge record edit 42 --body 'Revised canonical body'
forge record relate 44 --kind depends-on --to 45
forge record unrelate 44 --kind depends-on --to 45
forge record relate 44 --kind generated-by --to 41
forge record relate 46 --kind child-of --to 42
forge record relate 42 --kind initiative --to 40
forge record unrelate 42 --kind initiative --to 40
forge record start 44
forge record comment 44 --message 'Verified the outcome and recorded evidence.' --author operator
forge record done 44 --resolution 'Implemented and verified'
```

For `child-of`, the first ID is the child; use typed creation commands for typed parent membership. Relationship handlers enforce Record Type constraints; generic relationship syntax is not a way to reparent typed work. Dependencies must be satisfied before starting; an agent-owned In Progress Task cannot gain a new dependency. Generated-By Relationships may cross Project Scopes within one Workspace and imply neither ordering nor lifecycle control. A Spec or Wayfinder may attach to one Initiative in the same Workspace; membership is independent of parentage and can be detached explicitly.

List status values are `open`, `in-progress`, and `done`; Record Types are `generic`, `spec`, `task`, `wayfinder`, `grilling`, and `initiative`. A list without a Project filter remains confined to the selected Workspace, not the containing Project. `record updates` is the Forge-authored Update Log, not editable history. Body edits and comment messages are inline strings, unlike creation's stdin support; do not assume `--body -` works for edit or that file flags exist.

## Workflow readiness and planning

```sh
forge ready --workspace personal --json
forge ready --workspace personal --planning --include-hitl --blocked --json
```

`ready` lists Open Tasks whose direct prerequisites are Done. `--planning` also includes Specs, Wayfinders and Initiatives; `--include-hitl` includes Grillings. `--blocked` retains candidates with unfinished prerequisites in `blocked_by`. Results remain Workspace-local. This is workflow readiness, independent of provider availability or dispatch setup; it does not promise that an Agent can be provisioned.

## Managed Worktrees and hooks

```sh
forge project setup-hook set --project alpha 'cargo fetch'
forge project setup-hook clear --project alpha
forge worktree create 44 --branch feat/forge-44 --base main --json
forge worktree info 44 --json
forge worktree list --all --json
forge worktree doctor --json
forge worktree doctor --repair --json
forge worktree recover 44 --confirm-no-active-agent --json
forge worktree remove 44 --json
```

Select a Forge Workspace with `--workspace` or canonical cwd; worktree IDs, lists and doctor operations stay within it. Placement is fixed at `<workspace-root>/.forge/worktrees/<project-slug>/<record-id>`, including nested Projects. Restore redirected/symlinked placement before retrying; force does not bypass containment or open-Assignment/resource guards. Inspect the Task's Base Commit and ownership before choosing branch/base; automatic dispatch uses the captured Base Commit and deterministic Task Branch. An example `main` is not authorization to change the dispatch base or bypass an existing Worktree Claim.

Hooks execute shell commands in newly created Managed Worktrees. A failed hook retains evidence and blocks automatic retry. `doctor` diagnoses; `--repair` mutates. `recover --confirm-no-active-agent` is an operator assertion requiring verification that no agent is active. Removal is safety-checked; inspect dirty, missing, invalid, or agent-owned state rather than forcing cleanup. Forge does not automatically remove a Managed Worktree when its Task completes.

## Dispatch administration

```sh
forge dispatcher config get
forge dispatcher config set herdr-socket /absolute/path/to/herdr.sock
forge dispatcher config set task-profile 1
forge dispatcher config set grilling-profile 1
forge dispatcher config set workspace existing-herdr-workspace-id
forge dispatcher config get workspace
forge dispatcher config unset workspace
forge dispatcher config unset herdr-socket
forge dispatcher profile
forge dispatcher profile --kind grilling
forge dispatcher config set acknowledgement-timeout-ms 300000
forge dispatcher config unset acknowledgement-timeout-ms
forge dispatcher config set project-limit 2 --project alpha
forge dispatcher config get project-limit --project alpha
forge dispatcher config unset project-limit --project alpha
forge dispatcher status --project alpha --json
forge dispatcher reconcile --json
forge dispatcher dispatch 47 --json
```

Select the Forge Workspace by `--workspace <id>`, qualified Project reference or canonical cwd, with no saved default. `task-profile` and `grilling-profile` independently select existing supported revision numbers; both are required for dispatch. `config set workspace` verifies the exact existing Herdr identity and rejects a binding already owned by another Forge Workspace. Forge never creates, adopts by label, replaces or deletes the Herdr container. `herdr-socket` is the daemon-wide absolute Unix socket, not the Forge RPC endpoint. Reads show effective values, ownership and default/override provenance without contacting Herdr. `profile --kind task|grilling` inspects the Workspace-selected revision.

`config unset` explicitly removes a selection/override. Binding changes are guarded by that Workspace's live Assignments and retained resources, including closed uncertain attempts; socket changes apply that guard daemon-wide and invalidate every Workspace's provider verification. Repeat `config set workspace <bound-id>` for fresh verification at the new endpoint. `status` distinguishes configured completeness from current dispatchability and reports profile, binding, availability and identity blockers. Registration, planning and local inspection remain usable while dispatch is blocked. Investigate with `assignment-info` before safe recovery.

`project-limit` requires explicit `--project` scope for reads, sets and unsets; unset restores capacity 1. Aggregate configuration includes scoped capacity alongside Workspace selections and daemon socket. Timeout set/unset clones the selected Task profile into a new immutable revision selected only by this Workspace; existing Assignments retain their original revision and provider identity. Unset restores 300000 ms.

Reconciliation can provision agents and worktrees; it is not a read-only health check. Automatic dispatch selects oldest-ready eligible Open Tasks within Project capacity, never Grillings. `dispatcher dispatch <record-id>` attempts the requested Task with its Managed Worktree, or an eligible single-Project Grilling interactively without one; other Record Types are unsupported. Readiness, capacity, Dispatch Holds, and recovery blockers still apply. Repeated/concurrent dispatch returns the existing open Assignment (`already_assigned: true`) without another launch or reservation. A closed Assignment with unresolved resources blocks replacement; follow the recovery runbook rather than dispatching around a hold.

## Assignment bootstrap and recovery

```sh
forge assignment bootstrap
forge dispatcher assignment-info assignment-example --json
forge dispatcher assignment-retire assignment-example --json
forge dispatcher assignment-resolve-unknown assignment-example --confirm-old-agent-cannot-act --confirm-unknown-resources-safe --json
forge dispatcher resume 44 --json
forge dispatcher task-reopen 44 --json
```

Agents use `bootstrap` with their delivered environment, not operator recovery commands. The lower-level `assignment task-start` requires assignment/capability/correlation arguments; use bootstrap rather than manually copying credentials to command arguments.

Operators: inspect `assignment-info` before recovery; it reads persisted evidence without contacting the provider. Retirement revokes Assignment access and attempts verified provider cleanup. Unresolved resources can continue reserving capacity after retirement, and the Dispatch Hold remains separate. Investigate before retrying or replacing an agent.

Only use `assignment-resolve-unknown` after verifying both assertions, including external agent activity and unknown resource safety. It is not automatic cleanup. `resume` clears a Dispatch Hold; it neither proves the old agent is gone nor resolves recovery blockers or authorizes replacement. `task-reopen` is for an unassigned manual In Progress Task, not a workaround for an active Assignment.
