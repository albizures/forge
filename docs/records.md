# Workspace-owned Records

Record commands use the selected Forgeup Revision, or an explicit
[endpoint override](development.md#run-a-manual-daemon). Add `--json` for structured
output. See [Project registration and selection](projects.md) for canonical roots,
nested Projects, Workspace-local slugs and qualified Project references.

## Select a Workspace

Every Record belongs to exactly one Forge Workspace. Select it with `--workspace`,
a qualified Project selector, or canonical client cwd containment, in that order.
Explicit selectors must agree; an explicit selector overrides cwd. There is no
saved default. Bare Record IDs do not select a Workspace.

```sh
forge record list --workspace personal --json
forge spec open --body 'Deliver the capability' --project personal/api
forge record show 42 --workspace personal --json
```

Outside all registered roots, supply a Workspace or qualified Project selector.
A list without a Project filter includes only the selected Workspace, not just
the containing Project. Empty Project scope means work within the Workspace but
outside any particular Project, never daemon-wide work. Identical Project slugs
in different Workspaces do not mix records or relationships.

Workspace selection is trusted-operator routing, not authentication or process
isolation. Assignment capabilities retain their separate authorization rules.

## Open work

A Record has a nonblank Markdown body, an opaque numeric ID, immutable Workspace,
Record Type and Project Scope. Creation accepts `--body -` for stdin.

```sh
forge record open --body 'Cross-project investigation' --project api --project web
forge record open --body 'Workspace research' --no-projects --workspace personal
forge spec open --body 'Deliver the capability' --project api
forge task open --body 'Projectless research' --kind research --workspace personal
forge task under-spec --body 'Implement the capability' --spec 42 --workspace personal
forge initiative open --body 'Coordinate delivery' --project api --project web
forge wayfinder open --body 'Discover the route' --workspace personal
forge task under-wayfinder --body 'Explore options' --wayfinder 43 --workspace personal
forge grilling open --body 'Choose the route' --wayfinder 43 --workspace personal
```

- Generic Records accept repeatable Projects or explicit `--no-projects`.
- Specs require exactly one Project; Spec Tasks inherit its scope and parentage.
- Standalone Tasks may have zero or one Project.
- Wayfinders and Initiatives may have zero or more Projects in one Workspace.
- A Task under a Project-scoped Wayfinder selects one of its Projects (inferred
  when there is only one). A Projectless Wayfinder's Task remains Projectless.
- Grillings inherit the Wayfinder's entire scope or select a nonempty subset;
  Projectless Wayfinder children inherit empty scope.
- Task Kind is optional `research`, `prototype`, or `review`.

## Relationships and lifecycle

```sh
forge record relate 44 --kind depends-on --to 45 --workspace personal
forge record relate 44 --kind generated-by --to 41 --workspace personal
forge record relate 42 --kind initiative --to 40 --workspace personal
forge record unrelate 42 --kind initiative --to 40 --workspace personal
forge record start 44 --workspace personal
forge record done 44 --resolution 'Implemented and verified' --workspace personal
```

Relationships cannot cross Workspaces: this includes parentage, dependencies,
provenance and Initiative membership. Mixed-Workspace Project scopes and failed
creation-time provenance leave no partial Records, relationships or update logs.

Typed parentage is chosen at creation and cannot be changed with generic
relationship commands. Generic Records may form nested `child-of` hierarchies;
a Generic child may attach to a Generic or Task parent with identical scope.
A child has at most one parent and cycles are forbidden.

`depends-on` points from blocked Record to prerequisite. Same-type Tasks or Specs
may depend on each other; a Task may also depend on a Grilling containing its
Project (or both may be Projectless). An agent-owned In Progress Task cannot gain
a dependency. `generated-by` is provenance, independent of lifecycle ordering;
it may cross Project scopes within one Workspace. Opening commands accept
repeatable `--generated-by`; corrections require the generated Record to be Open.

A Spec or Wayfinder may belong to one Initiative. Membership is non-hierarchical,
independent of lifecycle, and explicitly detachable; it neither reparents Records
nor creates dependencies.

Normal lifecycle remains `open` → `in-progress` → `done`. Direct prerequisites
must be Done before starting, and children must be Done before completing their
parent. Done bodies and completion resolutions are immutable. Records remain
retained rather than deleted. Comments are immutable; `record updates` is the
Forge-authored Update Log. Assignment recovery retains its separate, conservative
[reopen and retirement rules](assignment-recovery.md).

## Task Checks and Assessments

Every new Task receives an immutable, required **Implementation** Check in the
same transaction as opening the Task. Its criterion is “The work and deliverables
described by this Task are complete.” This applies to all Task Kinds, parentage
paths and Projectless Tasks; Research/Review/Prototype Tasks need not produce code.
Check IDs, names, descriptions and ordering cannot be edited or removed.

```sh
forge task check list 44 --workspace personal --json
forge task check show 44 1 --limit 50 --workspace personal --json
forge task check assess 44 1 --verdict pass --expected-version 1 \
  --actor reviewer --message 'Deliverables verified' --workspace personal --json
forge task check assess 44 1 --verdict fail --expected-version 2 \
  --actor reviewer --message-file explanation.md --workspace personal
```

All Checks must have a latest `pass` Assessment before either `record done` or
`assignment done` may complete a Task. Pending and failed criteria block completion;
`checks-not-passed` reports every blocked Check ID. Existing lifecycle and child
constraints still apply. Completion never fabricates an Assessment. Assessments
are frozen once the Task is Done (`record-done`). Other Record Types are unchanged.

Each immutable Assessment retains its numeric ID, verdict (`pass` or `fail`),
actor, explanation and timestamp. The latest Assessment alone is authoritative:
a new fail supersedes an older pass. List/show and successful assessment return
the observed Check `version`; assessment requires `--expected-version`. A stale
write returns `stale-version` without changing verdict, version, history or Update
Log. Each successful assessment atomically adds a minimal `check-assessed` Record
Update referring to its Assessment ID, not a copy of the explanation.

Operators must supply a nonblank `--actor` and exactly one of `--message` or
`--message-file`. Explanations must be nonblank; `--message-file -` reads stdin.
Optional caller-reported `--revision` accepts a full 40-hex commit SHA and `--dirty
true|false` distinguishes a known state from omitted/unknown state. Evidence is
retained without fetching Git data, running commands or changing verdict authority.

List ordering follows immutable Check position, then ID. Show returns current
Check state and an ascending Assessment history page (`--limit` 1–100, default 50).
Use its `next_cursor` with `--after` for the next page; a null cursor ends history.
Current state may be newer than the history page. Histories never expand to an
unbounded dump. All commands support text and `--json` output and the same Workspace
selection/confinement as Record commands.

Typed operator RPC additions are `record_taskCheckList` (`id`),
`record_taskCheckShow` (`task`, `check`, `after`, `limit`) and
`record_taskCheckAssess` (`task`, `check`, `assessment`). Each uses the flattened
Workspace/cwd selection envelope. Assessment input contains `expected_version`,
`verdict`, `actor`, `message`, optional `revision` and optional `dirty`. Responses
are respectively a Check array, `{check, assessments, next_cursor}`, and the updated
Check (including its latest Assessment). Invalid attribution/evidence, wrong Record
type or Check ownership, stale versions, frozen Tasks and completion blockers have
distinct error categories. Assignment completion already shares these gates;
capability-bound Check operations are a separate delivery.

This Check schema requires a fresh development store: no pre-feature Task backfill,
legacy database upgrade or no-Check completion fallback is provided.

## Project Check Templates

A Project's operator-managed Check Template adds ordered, required criteria after
Implementation when a Task opens. Standalone Tasks use their selected Project;
Spec children use the inherited Project; Wayfinder children use their selected
Project, not every Project in the Wayfinder scope. Projectless Tasks and Projects
without a configured template still receive Implementation alone. Every Task Kind
uses the same rules. Snapshot creation and Task opening commit atomically; concurrent
replacement/opening observes a whole template, never mixed criteria. Existing Task
Check IDs, criteria and positions never change when a template is replaced.

```sh
forge project check-template show personal/api --limit 50 --json
forge project check-template set personal/api --file criteria.json \
  --actor operator --message 'Require review and tests' --expected-version 0 --json
forge project check-template show personal/api --after 1 --limit 50 --json
```

The file is an ordered JSON array of `{\"name\":\"Review\",\"description\":\"Deliverables
have been reviewed\"}` objects. Names and descriptions must be nonblank and are
trimmed before storage. Names must be unique ignoring case and surrounding
whitespace; `Implementation` is reserved in any case. `[]` clears only the extras,
never the baseline. Unknown object fields and malformed JSON are rejected.

Read the observed `version` before replacement. An unconfigured template reads as
version **0**, empty criteria and empty history; its first replacement requires
`--expected-version 0`. Every successful whole-set replacement (including clearing)
increments the version. Stale writes return `stale-version`; invalid criteria or
blank attribution/explanations return `invalid-check-template`. Failed replacements
leave the current template and history unchanged. Supply nonblank `--actor` and
exactly one nonblank `--message` or `--message-file`; `--message-file -` reads stdin.

Show returns current `version`, ordered `criteria`, a bounded ascending `history`
page and `next_cursor`. Each immutable history entry has an opaque numeric `id`,
`version`, the replacement `criteria`, `actor`, `message` and `created_at_millis`.
`--limit` accepts 1–100 (default 50); pass `next_cursor` to `--after` to continue.
Current criteria/version may be newer than the history page. Template configuration
has its own attributed history and never invents a Task Record Update.

Both commands support text and `--json`, bare or qualified Project selectors and
normal Workspace selection. Typed operator RPC methods are
`record_projectCheckTemplateShow` (`project`, `after`, `limit`) and
`record_projectCheckTemplateSet` (`project`, `replacement`), using the flattened
Workspace/cwd envelope. Replacement contains `expected_version`, ordered `criteria`,
`actor` and `message`. Show returns `{version, criteria, history, next_cursor}`;
set returns `{version, criteria}`. Project ownership is checked in the repository
and selectors are qualified within the selected Workspace before application work.
Assignment capabilities have no template mutation operation. These additions
require fresh development stores; no migration or Task backfill is provided.

## Workflow readiness

```sh
forge ready --workspace personal --json
forge ready --workspace personal --planning --include-hitl --blocked --json
```

By default, readiness lists Open Tasks with all direct prerequisites Done.
`--planning` also includes Specs, Wayfinders and Initiatives; `--include-hitl`
includes Grillings. `--blocked` includes blocked candidates and their direct
unfinished prerequisite IDs in `blocked_by`. Ordering is newest first, like
Record listing. Every result remains in the selected Workspace.

This is workflow readiness, not provider dispatchability. Registration, records,
planning and inspection work without dispatch profiles or a provider binding.
[Dispatch readiness](dispatch.md#configure-dispatch) additionally requires both
profiles, a verified provider binding and present availability; workflow readiness
does not promise an Agent can be provisioned. See [Workspaces](workspaces.md) for
the ownership contract and [Managed Worktrees](worktrees.md) for checkout placement.

## RPC and persistence

All Record RPC methods use the same flattened selection envelope. For example,
the `record_show` request payload is `{"workspace":"personal","id":42}`;
alternatively send an absolute client `cwd`. Creation can select through qualified
Project references. Relative cwd, missing selection and contradictory selectors
are rejected before mutation; the daemon never substitutes its own cwd. Record
responses expose immutable `workspace_id` and qualified Project references.

The application/store contracts enforce ownership and relationship confinement,
not just the CLI. Scoped application instances and repository handles are
request-local; they never persist a default. Internal unselected repositories
remain available for daemon-wide scheduling/diagnostic scans, not operator Record
request routing. SQLite constraints protect immutable ownership and prevent
cross-Workspace scope/relationship insertion. Retained Records, including
Projectless Records and Initiatives, prevent Workspace removal.

Owned development schemas can evolve while preserving IDs, ownership, Task Kind,
scopes and logs. Pre-Workspace development stores with unowned Records are rejected
with an actionable new-store diagnostic; legacy-data import is unsupported.
