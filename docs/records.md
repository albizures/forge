# Projects and Records

Commands below assume a selected Forgeup Revision. For a manually started daemon,
use an explicit [endpoint override](development.md#run-a-manual-daemon). Record
commands support `--json`; use `forge <command> --help` for the full CLI surface.

## Register a Project

A Project is a local Git repository's main worktree, identified by an immutable
slug. Registration requires an `origin` remote:

```sh
forge add project /repos/alpha alpha
cd /repos/alpha/src/module
forge here
```

`forge here` resolves the Project containing the current directory, including
subfolders. It exits nonzero outside registered Projects. Registration rejects
non-Git folders, linked worktrees, missing `origin`, and conflicting slugs or
Registered Folders. Registered Folders cannot overlap. Registrations persist in
the daemon's SQLite store.

## Open Records with explicit scope

A Record has a required, non-blank Markdown body and an opaque numeric ID.
Record Type and Project Scope are fixed when it opens.

```sh
forge record open --body 'Investigate the migration' --project alpha
forge record open --body 'General research' --no-projects
forge record show <record-id>
forge record edit <record-id> --body 'Revised Markdown body'
```

Generic Records accept repeatable `--project` flags or explicit `--no-projects`.
A Spec requires exactly one Project. A standalone Task may have zero or one;
a Task under a Spec atomically inherits its parent's Project and relationship:

```sh
forge spec open --body 'Deliver the migration' --project alpha
forge task open --body 'Investigate options'
forge task open --body 'Project-specific handoff' --project alpha
forge task under-spec --body 'Implement the migration' --spec <spec-id>
```

The CLI also provides Wayfinder and Grilling workflows, Tasks under Wayfinders,
and optional Task Kinds (`research`, `prototype`, `review`). See the
[agent workflow reference](../skills/forge/SKILL.md) and command help for these
planning surfaces.

## Lifecycle and completion

Records normally advance only through `open` → `in-progress` → `done`:

```sh
forge record start <record-id>
forge record done <record-id> --resolution 'Completed and verified'
```

A Done Record's body and optional completion resolution cannot change. Records
are retained rather than deleted; complete mistaken Records with an explanation.
A parent cannot complete while any child is not done. Starting a Record also
requires all direct prerequisites to be Done.

Dispatch has an explicit recovery-only action for unassigned manual In Progress
Tasks; it is not a general lifecycle reversal. See [assignment recovery](assignment-recovery.md).

## Hierarchy, dependencies, and provenance

A Spec may parent Tasks. A Task cannot parent another Record. Typed hierarchy is
chosen at opening and cannot be changed using `record relate` or `record unrelate`.
Use dedicated creation commands instead.

Generic Records can form nested `child-of` hierarchies. Each child has at most
one parent; both endpoints must be Generic and have identical Project Scopes.
Cycles are forbidden. Generic relationships may be corrected after completion:

```sh
forge record relate <child-id> --kind child-of --to <parent-id>
forge record unrelate <child-id> --kind child-of --to <parent-id>
```

`depends-on` points from a blocked Record to its prerequisite. An agent-owned
In Progress Task cannot gain a new dependency.

```sh
forge record relate <blocked-id> --kind depends-on --to <prerequisite-id>
```

`generated-by` points from a generated Record to the Record whose work or output
caused it. It is provenance, not hierarchy or dependency: it does not change
lifecycle, readiness, or dispatch. It may cross Project Scopes. Opening commands
accept repeatable `--generated-by <id>` flags; corrections are allowed only while
the generated Record is Open:

```sh
forge record relate <generated-id> --kind generated-by --to <generator-id>
forge record unrelate <generated-id> --kind generated-by --to <generator-id>
```

Assignment Context includes directly incident provenance Records, not transitive
traversal. Context visibility does not grant unrestricted update subscriptions;
see [subscription authorization](record-update-subscriptions.md).

## Discovery and updates

```sh
forge record list --project alpha --status open --type spec
forge record list --type task --kind research
forge record comments <record-id>
forge record updates <record-id>
```

Filters combine with AND; results are newest first. There is no full-text search,
Record deletion, or scope editing. Human and JSON output identify Record Type.

Existing databases migrate automatically. Records predating typed creation
remain `generic` with their existing IDs and data. Relationship updates and
subscription payloads include relationship kind for new events; consumers must
tolerate its absence on migrated legacy events rather than inventing history.
Back up databases before upgrades.
