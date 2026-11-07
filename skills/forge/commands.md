# Command reference

Read [SKILL.md](SKILL.md) first for CLI selection, scope, and role boundaries. IDs, Project Slugs, paths, branches, and assignment identifiers below are illustrative: substitute inspected values. Commands are independent examples, not a batch workflow. Add `--json` where supported for structured output; consult source-built subcommand help for exact flags.

## Connection and Project registration

```sh
forge status --server-url http://127.0.0.1:45004/rpc --json
forge add project /repos/alpha alpha
forge here
```

Registration accepts a Git main Worktree and immutable Slug. The Repository needs an `origin` remote; independent clones are distinct Repositories. Select the correct daemon before registering. `here` recognizes the Project containing the current folder, including its other Worktrees.

## Open work

```sh
forge record open --body 'Work outside a Project' --no-projects
forge record open --body 'Cross-project work' --project alpha --project beta
forge spec open --body 'Deliver the capability' --project alpha
forge wayfinder open --body 'Explore the route' --project alpha --project beta
forge task open --body 'Deliberately scope-free investigation' --kind research
forge task open --body 'Standalone implementation' --project alpha
forge task under-spec --body - --spec 42
forge task under-wayfinder --body 'Compare alternatives' --wayfinder 43 --project alpha --kind research --generated-by 41
forge grilling open --body 'Resolve the scope decision' --wayfinder 43 --project alpha --project beta
```

Use `--body -` to read creation content from stdin. Supply stdin yourself; running that example alone waits for input. Repeat `--generated-by` for multiple generators. A multi-Project Wayfinder Task needs an explicit selected Project; a single-Project Wayfinder can supply it. A Grilling can omit Project flags to inherit its Wayfinder's entire scope.

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
forge record start 44
forge record comment 44 --message 'Verified the outcome and recorded evidence.' --author operator
forge record done 44 --resolution 'Implemented and verified'
```

For `child-of`, the first ID is the child; use typed creation commands for typed parent membership. Relationship handlers enforce Record Type constraints; generic relationship syntax is not a way to reparent typed work. Dependencies must be satisfied before starting; an agent-owned In Progress Task cannot gain a new dependency. Generated-By Relationships may cross Project Scopes and imply neither ordering nor lifecycle control.

List status values are `open`, `in-progress`, and `done`; Record Types are `generic`, `spec`, `task`, `wayfinder`, and `grilling`. A list without a Project filter is not automatically restricted by the current folder. `record updates` is the Forge-authored Update Log, not editable history. Body edits and comment messages are inline strings, unlike creation's stdin support; do not assume `--body -` works for edit or that file flags exist.

## Managed Worktrees and hooks

```sh
forge config get worktreeRoot
forge config set worktreeRoot /absolute/path/to/worktrees
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

Configure an absolute Worktree Root before creation. Inspect the Task's Base Commit and ownership before choosing branch/base; automatic dispatch uses the captured Base Commit and deterministic Task Branch. An example `main` is not authorization to change the dispatch base or bypass an existing Worktree Claim.

Hooks execute shell commands in newly created Managed Worktrees. A failed hook retains evidence and blocks automatic retry. `doctor` diagnoses; `--repair` mutates. `recover --confirm-no-active-agent` is an operator assertion requiring verification that no agent is active. Removal is safety-checked; inspect dirty, missing, invalid, or agent-owned state rather than forcing cleanup. Forge does not automatically remove a Managed Worktree when its Task completes.

## Dispatch administration

```sh
forge dispatcher config get
forge dispatcher config set herdr-socket /absolute/path/to/herdr.sock
forge dispatcher config set workspace existing-workspace-id
forge dispatcher config get workspace
forge dispatcher config unset workspace
forge dispatcher config unset herdr-socket
forge dispatcher profile
forge dispatcher config set acknowledgement-timeout-ms 300000
forge dispatcher config unset acknowledgement-timeout-ms
forge dispatcher config set project-limit 2 --project alpha
forge dispatcher config get project-limit --project alpha
forge dispatcher config unset project-limit --project alpha
forge dispatcher status --project alpha --json
forge dispatcher reconcile --json
forge dispatcher dispatch 47 --json
```

`config set workspace` verifies the explicitly named existing Herdr Workspace. `herdr-socket` is Herdr's absolute Unix socket path, not the Forge RPC endpoint. Reads show effective values and default/override provenance; `config unset` explicitly removes an override. Socket/Workspace changes are rejected while unresolved Assignments or retained resources depend on them; investigate with `status` and `assignment-info` before safe recovery. `project-limit` requires explicit `--project` scope for reads, sets, and unsets; unset restores capacity 1. Aggregate `config get --project <slug>` includes that Project's capacity alongside global values. Timeout set/unset creates an immutable Task Agent Profile revision for future Assignments only; existing Assignments retain their bound profiles. Unset restores 300000 ms.

Reconciliation can provision agents and worktrees; it is not a read-only health check. Automatic dispatch selects eligible dependency-ready Open Tasks within Project capacity. Grilling dispatch explicitly launches interactive human-in-the-loop work without a Managed Worktree.

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
