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

## Inspect Task activity

`forge record show ID` and `forge record list` display Task lifecycle separately
from persisted Assignment activity, Dispatch Holds and current Checks. A closed
Assignment can retain historical `working` state: it is not an active worker.
An unacknowledged launch or retained resource is not proof of agent absence.
Human-input holds require investigation and explicit intervention, not automatic
resume. The output links to Assignment inspection for deeper recovery evidence.

Structured Record responses add an optional `activity` object on Task show/list:
`state`, `assignment_id`, `historical_assignment_state`, `closed_at_millis`,
`acknowledged_at_millis`, `retained_resources`, `dispatch_hold`, and `stack`.
States are `absent`, `closed`, `provisioning`, `working`, `idle`, `blocked`,
`unknown`, or `retained-resources`. Closure overrides historical state; resources
retained by any attempt prevent claiming safe absence. `checks` contains current
Check summaries on both Task show and list; null verdict means pending. Remaining
Checks are those without a current Pass. Other Record/mutation responses may omit
`activity`; missing activity means not inspected, not no assignment.

`stack`, when present, reports predecessor ID, expected prepared input commit,
verified predecessor commit and any evidence problem. These are persisted facts,
not a new Git/hosting verification. Inspection is read-only: no provider calls,
reconciliation, launches, hold clearing or resource retirement. Activity, hold,
resource count and lineage share a storage read snapshot; lifecycle and Checks
are separate reads, so the complete response is not an atomic execution snapshot.
Profiles, capabilities and provider resource identities are not embedded.

This is an additive wire change. Clients reading new responses must tolerate the
new member; older responses without it still decode. Old clients that reject
unknown fields need a matching CLI revision.

## Open work

A Record has a nonblank Markdown body, an opaque numeric ID, immutable Workspace,
Record Type and Project Scope. Creation accepts `--body -` for stdin.

```sh
forge record open --body 'Cross-project investigation' --project api --project web
forge record open --body 'Workspace research' --no-projects --workspace personal
forge spec open --body 'Deliver the capability' --project api
forge task open --body 'Projectless research' --workspace personal
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
- Describe research, prototype, and review deliverables in the Task body; they are not separate execution categories.

## Explicit Execution Mode

Every Record stores `execution_mode` independently of its type, lifecycle and
profiles. Project-scoped Tasks default to `dispatched`; Projectless Tasks and
all other types default to `manual`. Missing Assignments never imply Manual.
Other types' metadata does not authorize unsupported execution; Grilling's
existing explicit Assignment workflow remains separate from Task mode gates.

Ordinary operator start of a Dispatched Task returns `dispatched-start-forbidden`.
Select Manual explicitly before starting Project work without agents:

```sh
forge record execution-mode 44 manual --expected dispatched \
  --actor human --message 'Deliver and verify manually' --workspace personal
forge record start 44 --workspace personal
```

`record_executionModeSet` exposes the same selected-scope RPC contract with
`id`, `expected`, `mode`, `actor` and `message`. Only `manual` and `dispatched`
are accepted. Inspection includes the mode; Update Logs retain both modes,
attribution and explanation. A stale expected mode rejects without writes.
Starting never changes mode, and Done Records cannot change it.

Changes require a between-attempt boundary: close or explicitly invalidate any
open Manual attempt, confirm Assignment retirement, and reconcile retained or
uncertain resources. Active, idle, blocked and uncertain ownership all reject
with `execution-mode-unsafe-ownership`; silence is not retirement. Switching to
Dispatched validates **every immutable pinned Check profile**, including external
prerequisites, without selecting an adapter or substituting profiles. Missing or
unusable bindings return `invalid-check-profile` identifying the Check. Restore
prerequisites or remain Manual; profile-less Manual Checks need no launch setup.

Successful changes invalidate current Assessments and advance existing work
freshness without deleting history, Findings, holds, Assignment resources or
retry counters. A pristine Task has no work to invalidate. No-op selections do
not rewrite history. Neither mode selection nor a passing outcome clears a hold.

Manual Tasks reject explicit dispatch with `manual-dispatch-forbidden` and are
excluded from automatic scheduling. A Dispatched Projectless Task is ineligible
with `dispatch-requires-project`; Dispatched metadata cannot supply a Project.
Eligible Task dispatch now advances immutable ordered Checks with their pinned
profiles, after the verified rollout gates. See [dispatch operations](dispatch.md)
for prerequisites, bounded continuation and the separate final completion gate.
It never silently falls back to Manual.

## Relationships and lifecycle

```sh
forge record relate 44 --kind waits-for --to 45 --workspace personal
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

`waits-for` points from blocked Record to prerequisite. Same-type Tasks or Specs
may wait for each other, including cross-Project and Projectless cases; a Task may
also wait for a Grilling containing its Project (or both may be Projectless).
Both canonical prerequisite kinds require completion and participate in one cycle
check. Adding either kind requires the dependent Task to be Open.
`generated-by` is provenance, independent of lifecycle ordering; it may cross
Project scopes within one Workspace. Opening commands accept
repeatable `--generated-by`; corrections require the generated Record to be Open.

To require completion without inheriting a prerequisite's repository result,
create a Waits-For Relationship:

```sh
forge record relate 44 --kind waits-for --to 45 --workspace personal
forge record show 44 --workspace personal --json
forge record unrelate 44 --kind waits-for --to 45 --workspace personal
```

Use `builds-on` when a Task also inherits its prerequisite's verified repository
result. Both endpoints must be Tasks in one Project; the relationship alone selects
the immediate Stack Predecessor. Each Task has at most one predecessor and one
successor. Configure repository, remote and starting branch on the root only; see
[Task stacks](stacks.md). Review-only Tasks may inherit and verify unchanged output.

Inspection reports `waits-for` or `builds-on` in `kind`, in human and JSON output,
including Assignment-visible Record reads. The old `ordering_only` wire field is
removed. Removal must name the actual kind. A pair cannot carry both kinds;
replace intent explicitly by removing and recreating the relationship while
lifecycle restrictions permit. Shared parentage, Task prose and file edits never
infer intent. Neither declaration nor prerequisite completion authorizes dispatch.

Legacy `depends-on`, `ordering-only-depends-on` and `task stack after` declarations
are rejected, not aliases. Choose completion-only `waits-for` or code-inheriting
`builds-on`. Historical audit entries retain their original terminology. Existing
state requires the explicit validated [upgrade workflow](state-upgrades.md), not
reissuing declarations against live old state. This is an intentional CLI/RPC
compatibility break: update clients to declare and inspect canonical `kind`, rather
than relying on legacy aliases or the removed flag. It does not change daemon
Management Protocol 1 or offline upgrade interface 2.

Starting a Spec validates its persisted initial Task plan in the same transaction
as its Open → In Progress transition. Builds-On chains require complete root
configuration and valid inheritance; same-Spec Waits-For Relationships require no
Stack ancestry or publication configuration. Independent Tasks, independent derived
Stacks with Waits-For coordination, and direct waits on transitive ancestors remain
supported; Forge does not merge multiple code branches. Configured members must retain compatible Publication
Checks and dispatched profiles. Invalid plans remain Open without transition history.

Open Specs permit incomplete intermediate plans. Once a Spec starts, ordinary
child Task creation, prerequisite addition/removal, and root reconfiguration are
rejected transactionally. Comments and otherwise permitted Body edits remain available. Preparation and existing explicit
ownership/evidence recovery operations are not structural replanning; they do not
grant permission to rewrite a started plan. This initial product limitation does
not make Spec plans inherently immutable. See
[the Spec-start decision](../crates/docs/adr/0009-spec-start-gates-initial-plan-execution.md).

Spec child Tasks execute only while their parent is In Progress. Manual start,
Manual attempt begin, Assignment-bound start, automatic/explicit claims and
Check-bound dispatch revalidate the persisted plan at authorization, including
upgraded In Progress plans. An Open parent reports `spec-execution-blocked`; missing
required inheritance reports Task/prerequisite identities with `invalid-spec-plan`.
Configured Stack execution also rechecks the pinned Base Commit and verified
predecessor lineage; an initial attempt must bind the exact Stack input commit.
Standalone Tasks and Wayfinder children have no Spec-parent gate. Planning and
read-only operations remain available. See [safe recovery](assignment-recovery.md#spec-parent-and-inheritance-rejections)
for retained legacy checkouts; changing lifecycle alone does not repair a plan.

A Spec or Wayfinder may belong to one Initiative. Membership is non-hierarchical,
independent of lifecycle, and explicitly detachable; it neither reparents Records
nor creates dependencies.

Normal lifecycle remains `open` → `in-progress` → `done`. Direct prerequisites
must be Done before starting, and children must be Done before completing their
parent. Done bodies and completion resolutions are immutable. Records remain
retained rather than deleted. Comments are immutable; `record updates` is the
Forge-authored Update Log. Assignment recovery retains its separate, conservative
[reopen and retirement rules](assignment-recovery.md).

### Explicit operator-forced completion

Establish safety first: inspect Assignment/recovery evidence and close or invalidate
open Manual attempts through existing supported operations. Resolve uncertain
ownership and unsafe retained resources using [Assignment recovery](assignment-recovery.md)
and [Managed Worktrees](worktrees.md), not force. Missing heartbeats are not proof
of retirement; a clean retained historical checkout is not a cleanup target merely
because its Record is obsolete.

If newly eligible work must not launch, use the existing
[dispatch controls](dispatch.md) **before** closure. Completion neither pauses
dispatch nor launches work; ordinary reconciliation remains responsible.

```sh
forge record done 44 --force --resolution 'Superseded by replacement work' --workspace personal --json
```

Force closes only the selected safe Open or In Progress Record, of any type,
without starting it. It requires a nonblank Completion Resolution and bypasses
workflow/completion gates, not active Assignments, open Manual attempts,
unresolved ownership, or unsafe retained resource state. It performs no cleanup,
mode change, hold clearing, assessment rewrite, or child closure. Unmet prerequisites,
unfinished children, pending/failed Checks, blocking Findings, invalid legacy plans,
parent execution gates and missing publication can be bypassed. Their evidence,
dependency links, Stack configuration, profile revisions and Check pins are unchanged.
This is not a new status or replacement relationship.

The operator `record_done` RPC accepts the additive `force` boolean (default
`false`) alongside `id` and `completion_resolution` in its existing selected
Workspace envelope. `forced_completion` in completion and show/list responses
contains the linked Update ID, operator authority attribution, supplied resolution,
and transaction-time `{code, message}` bypassed gates. Readable inspection reports
the same override. Already-Done force replies set `already_done: true` and cannot
rewrite original metadata, label normal completion forced, or append an Update.
Assignment Capability completion and outcome payloads reject override fields:
workers cannot waive their own requirements. `--force` and the required existing
`--resolution` are sufficient explicit authorization; execution is noninteractive,
with no preview, confirmation or second reason flag. The existing operator API's
`operator` attribution is an authority label, not newly authenticated personal identity.

A new forced transition returns `dependency_consequences` with
`newly_eligible_record_ids` and a `warning`; readable output reports the same IDs
(or explicitly none) and warning. These are transaction-time workflow consequences,
not a later readiness inventory. Only Open direct dependents with all prerequisites
Done and valid persisted parent/plan/Stack authorization are reported. Other
prerequisites or an independently blocked plan/Stack can keep a dependent ineligible.
Already-Done replies and read-only inspection omit this operation-only result;
retries do not imply new eligibility changes. This additive response member defaults
to absent when decoding older responses; clients rejecting unknown fields need a
matching revision. Non-forced completion responses omit it and retain normal semantics.

Forced Done satisfies ordinary dependency gates exactly like normal Done. Workflow
eligibility is **not dispatchability**, a launch promise, passing Checks or verified
publication. Provider configuration, Execution Mode, holds and execution safety
remain independent dispatch gates. Stack inheritance still requires its own verified
repository result: forcing a predecessor Done supplies no missing commit or
publication evidence. Normal reconciliation may dispatch newly eligible work without
further confirmation. Closing a parent leaves every child unchanged (and a Spec's
children still require an In Progress parent for execution).

Done is closure, not assessment certification. Completion without force retains all
normal requirements. Already-Done requests preserve the original resolution,
forced/normal distinction and immutable Update Log even if a different explanation
is supplied. This feature does not authorize live Record cleanup, legacy Stack
migration, installation, release activation or rollout.

## Checks, Assessments and Findings

Every Task has ordered Checks, starting with a required Implementation Check. Their
Assessments, Findings, Manual attempts and Project Check Templates are described in
[Checks, Assessments and Findings](checks.md).

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

The current `ready` command is a dependency-only frontier, not a complete
execution preflight: it can include a Task under an Open Spec or one with invalid
Stack inheritance. Inspect the parent and Stack evidence separately; authoritative
start/attempt/dispatch operations enforce those gates. It also does not evaluate
Dispatch Holds, Assignment ownership or Check progress; normal Task show/list
exposes those facts separately.

Workflow eligibility is distinct from provider dispatchability. Registration, records,
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

Only fresh stores and restart of the current marked schema revision are supported.
Unversioned or incompatible stores fail with `fresh-store-required` before schema
initialization, without conversion, deletion, or reset. Ordinary restart preserves
ownership, scopes, logs, Checks, Assessments, and Findings. A reset is a separate
operator action requiring safe ownership retirement and resource reconciliation.

The supported contract uses `RecordCheck`, `record_checks` and `record_findings`,
with `record_id` ownership. Operator methods and requests use Record-prefixed names
and a `record` selector; old method names and owner selectors have no aliases.
Record creation and discovery reject obsolete classification inputs rather than
ignoring them. Generic naming does not expand ownership beyond Tasks or relax
same-owner linkage and selected-Workspace authorization.
