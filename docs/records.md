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
forge task check invalidate 44 1 --assessment 7 --expected-version 3 \
  --actor reviewer --message 'Evidence withdrawn' --workspace personal --json
forge task check show 44 1 --compare-revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  --workspace personal --json
```

All Checks must have a latest `pass` Assessment before either `record done` or
`assignment done` may complete a Task. Pending and failed criteria block completion;
`checks-not-passed` reports every blocked Check ID together with unresolved blocking
Finding IDs. Finding-only failures use `completion-blocked`. Existing lifecycle and child
constraints still apply. Completion never fabricates an Assessment. Assessments
and invalidations are frozen once the Task is Done (`record-done`). Other Record
Types are unchanged.

Each immutable Assessment retains its numeric ID, verdict (`pass` or `fail`),
actor, explanation and timestamp. The latest Assessment alone is authoritative:
a new fail supersedes an older pass. List/show and successful assessment return
the observed Check `version`; assessment requires `--expected-version`. A stale
write returns `stale-version` without changing verdict, version, history or Update
Log. Each successful assessment atomically adds a minimal `check-assessed` Record
Update referring to its Assessment ID, not a copy of the explanation.

`invalidate` requires the observed Check version and `--assessment` ID. Only the
latest, not-yet-invalidated Assessment may be invalidated (`invalid-transition`
otherwise). It appends attributed invalidation metadata and activity, preserving
the original Assessment payload. The Check becomes pending: an older pass never
revives. A new Assessment establishes a new verdict. Invalidation, version increment,
activity and minimal `check-invalidated` Record Update (referencing the Assessment)
commit together. Failed or stale attempts leave no partial history or Update Log.
Invalidation accepts the same explanation, actor and optional evidence inputs as
assessment. Repeating it at the old version is stale; at the current version it is
an invalid transition.

Operators must supply a nonblank `--actor` and exactly one of `--message` or
`--message-file`. Explanations must be nonblank; `--message-file -` reads stdin.
Optional caller-reported `--revision` accepts a full 40-hex commit SHA and `--dirty
true|false` distinguishes a known state from omitted/unknown state. Evidence is
retained without fetching Git data, running commands or changing verdict authority.
List/show accept explicit `--compare-revision` (a full SHA). Assessment and invalidation
`comparison` is `matching`, `mismatching` or `unknown`, separate from `dirty`.
Matching is case-insensitive SHA equality within the Task's Project context;
missing evidence, omitted comparison or Projectless context yields unknown.
There is no `--repository` option and no automatic evidence collection or verification.
Comparisons are read-only and never invalidate Assessments or alter completion gates.

List ordering follows immutable Check position, then ID. Show returns current
Check state and an ascending activity page (`--limit` 1–100, default 50).
`activities` contains stable numeric activity IDs and Assessment references, with
invalidation metadata on invalidation activities; `assessments` contains the
Assessment payloads for assessment activities in that page. Retained Assessment
payloads also expose any appended `invalidation` metadata. Use its `next_cursor`
with `--after` for the next page; cursors are activity IDs, not Assessment IDs.
A null cursor ends history. Empty pages beyond the last ID have a null cursor.
Current state may be newer than the history page. Histories never expand to an
unbounded dump. All commands support text and `--json` output and the same Workspace
selection/confinement as Record commands. Check JSON includes `current_verdict`
(`pass`/`fail`, or null for pending), `version`, `project` and the retained
`latest_assessment` (which may be invalidated). `record show` for Tasks embeds
current `checks` summaries, never history; its text output labels pending Checks.

Typed operator RPC additions are `record_taskCheckList` (`task`, optional `compare_revision`),
`record_taskCheckShow` (`task`, `check`, `after`, `limit`, optional `compare_revision`),
`record_taskCheckAssess` (`task`, `check`, `assessment`) and
`record_taskCheckInvalidate` (`task`, `check`, `invalidation`). Each uses the flattened
Workspace/cwd selection envelope. Assessment input contains `expected_version`,
`verdict`, `actor`, `message`, optional `revision` and optional `dirty`. Responses
are respectively a Check array, `{check, assessments, activities, next_cursor}`, and
an updated Check for either mutation (including its latest Assessment). Invalidation
input replaces `verdict` with `assessment_id`, retaining the other Assessment input
fields. Invalid attribution/evidence, wrong Record
type or Check ownership, stale versions, frozen Tasks and completion blockers have
distinct error categories. Assignment completion shares these gates.

## Task Findings (review cycles)

Findings retain review issues independently of Check verdicts. Passing a Check
never closes a Finding. Minor Findings default nonblocking; major and critical
Findings default blocking. An explicit `--blocking true|false` overrides that
creation default. Findings have no separate title.

```sh
forge task finding raise 44 --severity major --check 1 --assessment 7 \
  --actor reviewer --message 'Validation missing' --dirty false --workspace personal --json
forge task finding list 44 --limit 50 --workspace personal --json
forge task finding show 44 2 --after 0 --limit 50 --workspace personal --json
forge task finding respond 44 2 --response fixed --expected-version 1 \
  --actor implementer --message 'Corrected validation' --revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
forge task finding resolve 44 2 --expected-version 2 --actor reviewer \
  --message-file verification.md --workspace personal --json
forge task finding reopen 44 2 --expected-version 3 --actor reviewer --message 'Still reproducible'
forge task finding dismiss 44 2 --expected-version 4 --actor reviewer --message 'Not applicable'
forge task finding amend 44 2 --severity minor --blocking false --expected-version 5 \
  --actor reviewer --message 'Impact clarified'
forge task finding note 44 2 --expected-version 6 --actor reviewer --message-file correction.md
```

All mutations require nonblank `--actor` and exactly one nonblank explanation
via `--message` or `--message-file` (`-` reads stdin). Optional `--revision` is a
full 40-hex SHA; `--dirty true|false` is caller-reported, with omission unknown.
List/show accept `--compare-revision` with the same read-only Project-context
comparison rules as Check evidence. No Git operation is performed.

Optional `--check` and `--assessment` origins must belong to the same Task.
An Assessment implies its Check; a conflicting explicit Check fails with
`invalid-finding-origin`. Invalidated Assessments remain valid historical origins.
Raised text, evidence and origins are immutable. `resolve` records a verification
explanation and evidence, moving an open or awaiting-review Finding to resolved
without changing severity or blocking. `dismiss` similarly closes an unresolved
Finding with a reason. `respond --response fixed|rejected` enters awaiting-review
from open or awaiting-review, retaining every successive response. A response
never closes a Finding or changes blocking. Closed Findings must `reopen` with a
reason before responding; reopening returns to open. `note` appends a correction
or evidence without changing disposition, including on closed Findings.

`amend` requires severity and/or blocking explicitly, retaining old/new severity
and blocking plus its reason. Severity-only amendments never reapply creation
defaults. Amendments and notes leave disposition unchanged. Activity exposes
`response` (fixed/rejected or null) and `amendment` (old/new severity and blocking,
or null). Raised text and origins remain unchanged throughout review cycles.
Every existing-Finding mutation requires `--expected-version`. Invalid transitions
fail `invalid-transition`; stale versions fail `stale-version`, with no activity
or log changes. Empty amendments fail `invalid-finding`. Each successful mutation returns
the observed `version` and atomically appends an attributed activity plus a minimal
`finding-activity` Record Update (`finding_activity_id`).

List returns `{findings, next_cursor}` in ascending Finding ID order. Show returns
`{finding, activities, next_cursor}` with ascending activity IDs. Both support
`--limit` 1–100 (default 50) and `--after`; pass `next_cursor` to continue, null
means the end. Current summaries include the immutable `raised` activity and
`latest_activity`, so a bounded older history page does not hide current state.
All commands support text/JSON and existing Workspace confinement. Wrong Task
ownership is `finding-not-found`; unavailable Workspace records retain existing
non-disclosure behavior. Blank attribution/explanation is `invalid-finding`,
malformed evidence `invalid-revision`, and invalid pagination `invalid-page`.

Both operator and Assignment Task completion transactionally enforce all Check
and unresolved blocking Finding gates, reporting all blocker IDs together.
Awaiting-review remains unresolved, including rejected responses. Nonblocking
Findings do not block Done. Completion adds no Finding/Assessment activity.
On Done, raising, severity/blocking amendments and blocking-Finding reopening
fail `record-done`. Valid responses, notes, resolve/dismiss and reopening of
existing nonblocking Findings remain permitted. Notes and valid reviewer
dispositions on existing blocking Findings remain permitted, but closed Findings
still require reopening before responses, and blocking reopening is frozen.
No operation reverses completion or revives Assignment capabilities. Completion
and blocker mutations serialize transactionally; failed operations leave no
partial activities, amendments or Record Updates.

Typed operator RPCs (flattened Workspace/cwd envelope):
- `record_taskFindingList`: `task`, `after`, `limit`, `compare_revision` → Finding list page.
- `record_taskFindingShow`: `task`, `finding`, `after`, `limit`, `compare_revision` → Finding/activity page.
- `record_taskFindingRaise`: `task`, `finding` containing `severity`, optional `blocking`,
  `check_id`, `assessment_id`, plus `actor`, `message`, `revision`, `dirty` → Finding.
- `record_taskFindingResolve`: `task`, `finding` ID, `resolution` containing mandatory
  `expected_version`, `actor`, `message`, optional `revision`, `dirty` → Finding.

- `record_taskFindingMutate`: `task`, `finding` ID, `mutation` containing mandatory
  `expected_version`, `actor`, `message`, optional `revision`, `dirty`, plus a typed
  `change` object → Finding. `change` is one of:
  - `{ "action": "respond", "response": "fixed" }` (or `rejected`)
  - `{ "action": "resolve" }`, `{ "action": "dismiss" }`, `{ "action": "reopen" }`, `{ "action": "note" }`
  - `{ "action": "amend", "severity": "minor", "blocking": false }` (at least one field)

### Capability-bound Assignment Findings

```sh
forge assignment finding list --after 0 --limit 50 --json
forge assignment finding show 2 --limit 50 --compare-revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa --json
forge assignment finding raise --severity major --check 1 --assessment 7 --message 'Validation missing' --dirty false --json
forge assignment finding respond 2 --response fixed --expected-version 1 --message-file - --json
forge assignment finding resolve 2 --expected-version 2 --message-file verification.md --json
forge assignment finding reopen 2 --expected-version 3 --message 'Still reproducible'
forge assignment finding dismiss 2 --expected-version 4 --message 'Not applicable'
forge assignment finding amend 2 --severity minor --blocking false --expected-version 5 --message 'Impact clarified'
forge assignment finding note 2 --expected-version 6 --message-file correction.md
```

All nine commands implicitly target only the authenticated Assignment's Task.
They use protected Assignment delivery (`FORGE_ASSIGNMENT_ID`,
`FORGE_ASSIGNMENT_DAEMON_URL`, `FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE`), never
operator Workspace selection or endpoint overrides. No Task selector or `--actor`
is accepted. Findings and origin Check/Assessment IDs from other Tasks or
Workspaces are unavailable, even when a related Record is readable through
Assignment context. Unknown and foreign IDs have the same non-disclosing errors.
Grilling Assignments cannot operate on Task Findings.

Every operation revalidates delivery and durable Assignment validity. Closed
Assignments and revoked/replaced secret files fail `assignment-capability-denied`.
Permitted post-Done review still requires valid existing authorization; it never
revives capabilities or reverses Task completion. There is no template mutation,
identity registration, or execution orchestration through these commands.

The same explanation, evidence, observed-version, lifecycle, pagination and atomic
Record Update rules apply as for operators. Mutations require exactly one of
`--message` and `--message-file` (`-` reads stdin). Text and JSON expose current
state and bounded history. Each activity retains authenticated Assignment ID as
`actor` and a separate `assignment_id` linking immutable profile/execution
provenance. Operator activities keep `assignment_id: null`, even if actor text
matches an Assignment ID. Neither route requires distinct reviewer identities.
Both completion routes use the same transactional Check/Finding gates.

Typed RPC requests contain `assignmentId` and `secret` (no Debug implementation),
with no Workspace/cwd envelope or caller Task/actor override:
- `assignment_findingList`: optional `after`, required `limit`, optional
  `compareRevision` → Finding list page.
- `assignment_findingShow`: `finding`, optional `after`, required `limit`, optional
  `compareRevision` → Finding/activity page.
- `assignment_findingRaise`: `severity`, optional `blocking`, `checkId`,
  `assessmentId`, required `message`, optional `revision`, `dirty` → Finding.
- `assignment_findingMutate`: `finding`, mandatory `expectedVersion`, `message`,
  optional `revision`, `dirty`, and `change` → Finding. `change` uses the same
  typed action objects listed above; all six existing-Finding CLI actions use
  this method (including resolve).

Unknown request fields, including Task/actor overrides inside `change`, are
rejected as invalid RPC parameters. Valid requests retain stable domain errors
listed above; the CLI reports allowlisted categories without echoing server
messages, secrets, or delivery endpoints.

## Capability-bound Assignment Checks

```sh
forge assignment check list --compare-revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa --json
forge assignment check show 3 --after 9 --limit 50 --json
forge assignment check assess 3 --verdict pass --expected-version 1 --message 'Verified deliverables' --dirty false --json
forge assignment check invalidate 3 --assessment 4 --expected-version 2 --message-file - --json
```

These use only protected Assignment delivery (`FORGE_ASSIGNMENT_ID`,
`FORGE_ASSIGNMENT_DAEMON_URL`, `FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE`), not
operator Workspace selection or endpoint overrides. No Task selector or `--actor`
is accepted: all four operations implicitly target the authenticated Assignment's
Task. They grant no template mutation or arbitrary cross-Task access, even to
Checks of Records readable through the existing one-hop context API. Grilling
Assignments cannot operate on Task Checks. Every operation revalidates delivery
and durable Assignment validity; revoked secret files and closed Assignments fail
with the existing redacted authorization failure. Done Tasks freeze mutations
without extending or reviving capability lifetime.

The same input, evidence, pagination, version, transaction and completion rules
apply as for operators. Mutations require exactly one nonblank `--message` or
`--message-file` (including stdin via `-`). Both Assessments and invalidations
retain `actor` derived from authenticated Assignment identity and a separate
`assignment_id` provenance field; operator mutations have `assignment_id: null`,
even if the supplied actor text happens to equal an Assignment ID. The immutable
Assignment retains its profile/execution provenance. Attribution is not an
independent reviewer identity registry. Text and JSON expose current state and
bounded history using the same views as operator Check commands.

Typed RPCs are `assignment_checkList`, `assignment_checkShow`,
`assignment_checkAssess`, and `assignment_checkInvalidate`. All requests contain
`assignmentId` and `secret` (never Debug-logged). List adds optional
`compareRevision`; show adds `check`, `after`, `limit` and optional
`compareRevision`. Assessment adds `check`, `expectedVersion`, `verdict`, `message`,
optional `revision` and optional `dirty`; invalidation replaces `verdict` with
`assessmentId`. Unknown fields, including caller `actor` or Task selectors, are
rejected. Responses are the same Check array, Check page and updated Check shapes
as the operator APIs. Stable server domain errors are retained; the CLI reports
allowlisted error categories without echoing arbitrary credential-bearing server
messages. No Git commands or automatic evidence collection are performed.

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

The file is an ordered JSON array of objects, for example:

```json
[{"name":"Review","description":"Deliverables have been reviewed"}]
```

Names and descriptions must be nonblank and are trimmed before storage. Names must be unique ignoring case and surrounding
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
