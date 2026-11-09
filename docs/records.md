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
