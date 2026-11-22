# Checks, Assessments and Findings

How a Task's Checks, Assessments and Findings record its progress, and how Project Check
Templates set the Checks for new Tasks. Commands select a Workspace and Project the same way as
other [Record commands](records.md#select-a-workspace); add `--json` for structured output.
Record lifecycle, relationships and Execution Mode are in [Records](records.md).

## Record Checks and Assessments (Tasks only)

Every new Task receives an immutable, required **Implementation** Check in the
same transaction as opening the Task. Its criterion is “The work and deliverables
described by this Task are complete.” This applies to all deliverables, parentage
paths and Projectless Tasks; research, review, and prototype work need not produce code.
Check IDs, names, descriptions and ordering cannot be edited or removed.

```sh
forge record check list 44 --workspace personal --json
forge record check show 44 1 --limit 50 --workspace personal --json
forge record check work 44 --workspace personal --json
forge record check begin 44 --input-file attempt.json --workspace personal --json
forge record check outcome 44 --input-file outcome.json --workspace personal --json
forge record check invalidate 44 1 --assessment 7 --expected-version 3 \
  --actor reviewer --message 'Evidence withdrawn' --workspace personal --json
forge record check show 44 1 --compare-revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  --workspace personal --json
```

A human explicitly completes Manual work with `record done`. All immutable Checks
must have a current, evidence-bound `pass` Assessment in their fixed order.
Pending and failed criteria block completion;
`checks-not-passed` reports every blocked Check ID together with unresolved blocking
Finding IDs. Finding-only failures use `completion-blocked`; the durable Dispatch Hold
is retained, never implicitly cleared. Completion rechecks that direct prerequisites
and children are Done, requires In Progress status and Manual mode, and rejects active or
uncertain Assignment ownership, unreconciled retained resources and open attempts.
Idle or absent Assignments are not proof of safe retirement. Dispatched Tasks reject
completion with `dispatched-completion-forbidden`; historical Assignment credentials
cannot bypass these gates or authorize autonomous completion. Completion never
fabricates an Assessment. Assessments and invalidations are frozen once the Task is Done (`record-done`). Other Record
Types are unchanged.

Each immutable Assessment retains its numeric ID, verdict, actor, explanation,
timestamp, attempt ID, generation and immutable evidence. Only its current,
not-invalidated verdict counts. Each accepted outcome atomically adds its Assessment,
activity, Check version, output evidence and `check-assessed` Record Update, and
closes the attempt with a durable replay result. Unsupported unbound `check assess`
and Assignment `check assess` operations return `attempt-required`; they cannot
manufacture passing evidence.

`invalidate` requires the observed Check version and `--assessment` ID. Only the
latest, not-yet-invalidated Assessment may be invalidated (`invalid-transition`
otherwise). It appends attributed invalidation metadata and activity, preserving
the original Assessment payload. Invalidation conservatively invalidates **all**
current Checks, clears output evidence, advances the Task's freshness generation,
and closes outstanding attempts, even at an unchanged commit. An older pass never
revives. These changes, activities and `check-invalidated` updates commit together.
Invalidation accepts attribution, explanation and optional revision/dirty annotations.
Repeating it at the old version is stale; at the current version it is an invalid
transition. `invalidate-work` below also works when there is no current Assessment.

Operators must supply a nonblank `--actor` and exactly one of `--message` or
`--message-file`. Explanations must be nonblank; `--message-file -` reads stdin.
For Check invalidation annotations, optional `--revision` accepts a full 40-hex
commit SHA and `--dirty true|false` distinguishes known from unknown state. Outcomes
instead require the immutable evidence object below. Evidence is caller-attested;
Forge does not fetch artifacts, inspect Git or independently certify their contents.
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

Typed operator RPC additions are `record_recordCheckList` (`record`, optional `compare_revision`),
`record_recordCheckShow` (`record`, `check`, `after`, `limit`, optional `compare_revision`),
`record_recordCheckInvalidate` (`record`, `check`, `invalidation`). Each uses the flattened
Workspace/cwd selection envelope. Responses are a Check array, a bounded Check page,
and an updated Check. Invalidation input contains `expected_version`, `assessment_id`,
`actor`, `message`, optional `revision` and `dirty`. The former unbound
`record_recordCheckAssess` method remains a rejecting diagnostic boundary, not a
supported way to create Assessments.

### Manual attempts and immutable evidence

Manual Execution Mode is always an explicit choice: normal Check-driven dispatch never falls
back to Manual. Evidence is caller-attested, not fetched or independently certified by Forge.
For an end-to-end example, see the
[two-review handback walkthrough](#walkthrough-two-review-handback) below.

Start the Task explicitly, then inspect `record check work <record>`. Freshness starts
at generation 1. No Managed Worktree, Agent Profile or provider launch is needed.
Explicit Manual Execution Mode and confirmed safe ownership/resource state are required
for Manual attempts and completion. These commands do not retire ownership, reconcile
resources or clear holds.
Dispatched Tasks instead use [Check-bound Assignment outcomes](dispatch.md#check-attempts-and-controlled-acceptance), not Manual attempts.

`attempt.json` (use the actual Check ID and observed generation):

```json
{"id":"report-implementation-1","check_id":1,"generation":1,"actor":"operator",
 "evidence":{"kind":"artifact","uri":"urn:report:input","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}
```

An attempt permanently binds its ID, Task, Check, generation, actor, input evidence
and Task body. Only one attempt per Task may be open. An Implementation attempt binds
the current input; reviews bind the passing Implementation's output and may proceed
only after every earlier Check passes. Reusing an ID with different bindings returns
`attempt-conflict`. A failed Implementation retains its explanation/history and cannot
advance reviews; a new attempt can retry it. A passing Check requires invalidation
before reassessment.

`outcome.json`:

```json
{"attempt_id":"report-implementation-1","generation":1,"verdict":"pass",
 "actor":"operator","message":"Verified the completed report",
 "evidence":{"kind":"artifact","uri":"urn:report:final","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}
```

Implementation may produce different output evidence. Reviews must repeat the output
evidence they bound at attempt start. Repository evidence is also supported:
`{"kind":"repository","repository":"personal/project","commit":"<full 40-hex SHA>"}`.
An artifact requires a nonblank URI plus full 64-hex SHA-256; mutable URLs or paths alone
are insufficient. `--input-file -` reads strict JSON from stdin. Unknown fields are
rejected, including unknown fields inside Finding and verification entries.

Outcomes may additionally contain `findings` (`severity`, optional `blocking`, `message`),
`verifications` (`finding_id`, `expected_version`, `message`) and `disposition`
(`complete` by default, `handback`, or `interrupted`). New Findings inherit the attempt's
Check, new Assessment, actor and evidence; callers cannot substitute origin IDs.
A failed review must explicitly choose `handback` or `interrupted`:

- `handback` atomically retains the failed Assessment and Findings, advances freshness,
  invalidates **every** current Assessment and closes outstanding attempts. Remediation
  starts from the reviewed deliverable. Implementation and all reviews rerun in their
  original order with unchanged Check identities and pinned profiles.
- `interrupted` closes the attempt without an Assessment, Findings, verification or
  invalidation. It requires a failed review verdict and empty Finding/verification lists;
  a new attempt can retry that review without rerunning prior passing Checks.
- Failed Implementation retains its Assessment and explanation, never permits review
  advancement and cannot request handback/interruption.

Example review-specific fields (alongside the bound outcome fields above):

```json
{"verdict":"fail","disposition":"handback",
 "findings":[{"severity":"major","blocking":true,"message":"Correct validation"}]}
```

On rerun, the owning Check supplies, for example,
`"verifications":[{"finding_id":2,"expected_version":2,"message":"Fix verified"}]`.
Implementation may `respond --response fixed` to propose a fix but cannot verify a
reviewer's Finding. A passing review with its own unresolved blocking Finding rejects
atomically with `unresolved-check-findings`; no Assessment or other submitted effect
is accepted. Verification ownership is Check identity, not a claimed actor name.

`record check outcome` returns `{attempt, evidence, check, findings, work, blocking_findings}`.
`findings` contains changed Finding snapshots; `work` is freshness after acceptance.
Residual blockers are reported in `blocking_findings` and retain a durable Dispatch
Hold with a diagnostic. Existing safety holds are neither overwritten nor cleared;
verification also does not clear the operator-owned hold.
Exact replay returns the
**original** result even after closure, invalidation, Done and restart. Conflicting
replay returns `outcome-conflict`. Replay never restores historical freshness.
New stale/unauthorized outcomes cause no Assessment, activity, output or acceptance
writes. Actor is operator attribution, not a replacement for Workspace authorization;
Assignment capabilities cannot submit these operator-owned Manual attempts.

To change inputs/deliverables or explicitly invalidate at the same commit:

```sh
forge record check invalidate-work 44 --input-file invalidation.json --workspace personal --json
```

The JSON fields are `generation`, `evidence` (new immutable input), `actor`, `message`.
Editing the Task body also advances freshness and invalidates all current Assessments.
`work` exposes input/output evidence, generation and the latest invalidation's attribution.
Accepted outcomes remain history; old-generation unaccepted submissions return
`stale-generation`. Rerun Implementation and reviews in fixed order. Passing the final
Check does **not** complete the Task: a human must still request `record done`.

The operator RPC methods are `record_recordWorkState` (`record`),
`record_recordAttemptBegin` (`record`, `attempt`), `record_recordOutcomeSubmit`
(`record`, `outcome`), and `record_recordWorkInvalidate` (`record`, `invalidation`),
using the same strict flattened selection envelope and Task-only ownership rules.

#### Walkthrough: two-review handback

**Prepare.** Select the Workspace and register a Project ([Records](records.md),
[Projects](projects.md)); Projectless Tasks already default to Manual and receive
Implementation alone. Before opening a Project Task, replace its whole
[Check Template](#project-check-templates) using the observed template version, for
example:

```json
[
  {"name":"Implementation","description":"Deliver the described work"},
  {"name":"Review 1","description":"Review correctness"},
  {"name":"Review 2","description":"Review acceptance and verify findings"}
]
```

Profiles may be omitted for Manual work. Then open the Task, select Manual if it defaults to
Dispatched, start it and inspect its Check IDs (replace the example IDs with returned
values):

```sh
forge task open --body 'Deliver the report' --project personal/api --json
forge record execution-mode 44 manual --expected dispatched \
  --actor human --message 'Implement and verify manually' --workspace personal
forge record start 44 --workspace personal
forge record check list 44 --workspace personal --json
forge record check work 44 --workspace personal --json
```

Mode selection cannot override active or uncertain ownership or retained resources: confirm
retirement and reconciliation first. Silence and idle state are not retirement.

**Review cycle.** Submit each step with `record check begin` and then `record check outcome`,
using the `attempt.json` and `outcome.json` shapes above with
`--input-file <file> --workspace personal --json`. Let A be the initial evidence and B the
corrected deliverable:

| Step | Bound input / outcome | Result |
| --- | --- | --- |
| Implementation | A / pass with A | Review 1 becomes eligible |
| Review 1 | A / pass with A | Review 2 becomes eligible |
| Review 2 | A / fail with A, `disposition: "handback"`, blocking Finding | Atomic failed Assessment + Finding; generation advances; every Check becomes pending |
| Implementation rerun | A / pass with B | New output; old review passes cannot count |
| Review 1 rerun | B / pass with B | Required even if its earlier review passed |
| Review 2 rerun | B / pass with B and Finding verification | Owning Check verifies the correction |
| Human | Explicit `record done` | Completes only if every gate passes |

For an unlinked blocker, explicitly assign a review Check with
`record finding assign-verification`; assigning ownership alone does not resolve it.

**Complete.** Passing the last Check leaves the Task In Progress. Inspect Findings, then
request completion:

```sh
forge record finding list 44 --workspace personal --json
forge record done 44 --resolution 'Implemented and verified' --workspace personal
```

Completion rechecks Manual mode, all current ordered Checks, unresolved blockers,
dependencies, children, open attempts and conservative ownership/resource safety. It neither
fabricates Assessments nor clears retained safety state. Manual commands never upgrade a
store implicitly ([Revision State upgrades](state-upgrades.md)). The deterministic scenario
for this workflow, with repository and artifact evidence, same-evidence invalidation, restart
and explicit Done, is `crates/store/tests/manual_release.rs`.

## Record Findings (Task review cycles)

Findings retain review issues independently of Check verdicts. Passing a Check
alone never closes a Finding; an outcome must explicitly include verification. Minor Findings default nonblocking; major and critical
Findings default blocking. An explicit `--blocking true|false` overrides that
creation default. Findings have no separate title.

```sh
forge record finding raise 44 --severity major \
  --actor reviewer --message 'Validation missing' --dirty false --workspace personal --json
forge record finding list 44 --limit 50 --workspace personal --json
forge record finding show 44 2 --after 0 --limit 50 --workspace personal --json
forge record finding respond 44 2 --response fixed --expected-version 1 \
  --actor implementer --message 'Corrected validation' --revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
forge record finding resolve 44 2 --expected-version 2 --actor reviewer \
  --message-file verification.md --workspace personal --json
forge record finding reopen 44 2 --expected-version 3 --actor reviewer --message 'Still reproducible'
forge record finding dismiss 44 2 --expected-version 4 --actor reviewer --message 'Not applicable'
forge record finding amend 44 2 --severity minor --blocking false --expected-version 5 \
  --actor reviewer --message 'Impact clarified'
forge record finding note 44 2 --expected-version 6 --actor reviewer --message-file correction.md
```

All mutations require nonblank `--actor` and exactly one nonblank explanation
via `--message` or `--message-file` (`-` reads stdin). Optional `--revision` is a
full 40-hex SHA; `--dirty true|false` is caller-reported, with omission unknown.
List/show accept `--compare-revision` with the same read-only Project-context
comparison rules as Check evidence. No Git operation is performed.

Optional `--check` and `--assessment` origins must belong to the same Task.
An Assessment implies its Check; a conflicting explicit Check fails with
`invalid-finding-origin`. Invalidated Assessments remain valid historical origins.
Raised text, evidence and origins are immutable. Linked Findings have
`verification_check_id` fixed to their originating Check. They reject standalone
`resolve`, `dismiss` and amendments that remove blocking with `verification-required`;
only their Check's evidence-bound outcome may verify them. Unlinked Findings initially
have no verification owner. Assign a review Check explicitly (once only):

```sh
forge record finding assign-verification 44 2 --check 3 --expected-version 1 \
  --actor operator --message 'Review 2 owns verification' --workspace personal --json
```

This is an operator-only mutation. It does not rewrite origins or disposition and
rejects Implementation, foreign Checks and reassignment. Unresolved unlinked blockers
cannot be silently skipped; their hold diagnostic calls for an explicit owner and
verification. For unlinked Findings without an owner, `resolve` records a verification
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

Explicit Manual Task completion transactionally enforces all Check
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
- `record_recordFindingList`: `record`, `after`, `limit`, `compare_revision` → Finding list page.
- `record_recordFindingShow`: `record`, `finding`, `after`, `limit`, `compare_revision` → Finding/activity page.
- `record_recordFindingRaise`: `record`, `finding` containing `severity`, optional `blocking`,
  `check_id`, `assessment_id`, plus `actor`, `message`, `revision`, `dirty` → Finding.
- `record_recordFindingResolve`: `record`, `finding` ID, `resolution` containing mandatory
  `expected_version`, `actor`, `message`, optional `revision`, `dirty` → Finding.

- `record_recordFindingMutate`: `record`, `finding` ID, `mutation` containing mandatory
  `expected_version`, `actor`, `message`, optional `revision`, `dirty`, plus a typed
  `change` object → Finding. `change` is one of:
  - `{ "action": "respond", "response": "fixed" }` (or `rejected`)
  - `{ "action": "resolve" }`, `{ "action": "dismiss" }`, `{ "action": "reopen" }`, `{ "action": "note" }`
  - `{ "action": "amend", "severity": "minor", "blocking": false }` (at least one field)
  - `{ "action": "assign-verification", "check_id": 3 }` (operator-only, unlinked Finding)

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
Task completion requires retirement/reconciliation first, so historical Assignment
credentials do not permit post-Done review. Operator review never revives capabilities
or reverses Task completion. There is no template mutation,
identity registration, or execution orchestration through these commands.

The same explanation, evidence, observed-version, lifecycle, pagination and atomic
Record Update rules apply as for operators. Mutations require exactly one of
`--message` and `--message-file` (`-` reads stdin). Text and JSON expose current
state and bounded history. Each activity retains authenticated Assignment ID as
`actor` and a separate `assignment_id` linking immutable profile/execution
provenance. Operator activities keep `assignment_id: null`, even if actor text
matches an Assignment ID. Neither route requires distinct reviewer identities.
Assignment completion cannot bypass Manual mode, Check/Finding or ownership gates;
a human completes the Task after confirmed retirement and resource reconciliation.

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
forge assignment check invalidate 3 --assessment 4 --expected-version 2 --message-file - --json
```

These use only protected Assignment delivery (`FORGE_ASSIGNMENT_ID`,
`FORGE_ASSIGNMENT_DAEMON_URL`, `FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE`), not
operator Workspace selection or endpoint overrides. No Task selector or `--actor`
is accepted: these operations implicitly target the authenticated Assignment's
Task. They grant no template mutation or arbitrary cross-Task access, even to
Checks of Records readable through the existing one-hop context API. Grilling
Assignments cannot operate on Task Checks. Every operation revalidates delivery
and durable Assignment validity; revoked secret files and closed Assignments fail
with the existing redacted authorization failure. Done Tasks freeze mutations
without extending or reviving capability lifetime.

The same input, evidence, pagination, version, transaction and completion rules
apply as for operators. Mutations require exactly one nonblank `--message` or
`--message-file` (including stdin via `-`). Invalidations retain `actor` derived
from authenticated Assignment identity and a separate
`assignment_id` provenance field; operator mutations have `assignment_id: null`,
even if the supplied actor text happens to equal an Assignment ID. The immutable
Assignment retains its profile/execution provenance. Attribution is not an
independent reviewer identity registry. Text and JSON expose current state and
bounded history using the same views as operator Check commands.

Typed RPCs are `assignment_checkList`, `assignment_checkShow`,
and `assignment_checkInvalidate`. Unbound `assignment_checkAssess` rejects with
`attempt-required`; Check-bound live Assignment outcomes are not enabled in this release. All requests contain
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

A Project's operator-managed Check Template contains mandatory Implementation first,
followed by ordered, required reviews. The whole template is copied when a Task opens. Standalone Tasks use their selected Project;
Spec children use the inherited Project; Wayfinder children use their selected
Project, not every Project in the Wayfinder scope. Projectless Tasks and Projects
without a configured template still receive Implementation alone. All Task deliverables
use the same rules. Snapshot creation and Task opening commit atomically; concurrent
replacement/opening observes a whole template, never mixed criteria. Existing Task
Check IDs, criteria, positions and optional pinned Task Agent Profile revisions never
change when a template or Workspace profile selection changes. Checks cannot be
individually added, edited, retired or rebound; Assessment version changes do not
change their configuration.

```sh
forge project check-template show personal/api --limit 50 --json
forge project check-template preflight personal/api --json
forge project check-template set personal/api --file criteria.json \
  --actor operator --message 'Require review and tests' --expected-version 0 --json
forge project check-template show personal/api --after 1 --limit 50 --json
```

The file is an ordered JSON array of objects, for example:

```json
[
  {"name":"Implementation","description":"Deliver the described work","profile_revision":2},
  {"name":"Review","description":"Deliverables have been reviewed","profile_revision":3}
]
```

Names and descriptions must be nonblank and are trimmed before storage. Names must be unique ignoring case and surrounding
whitespace. The first entry must be named `Implementation`; empty templates,
missing/reordered baselines, duplicate names and revision zero are rejected.
To remove reviews, submit Implementation alone. Unknown object fields and malformed
JSON are rejected. `profile_revision` is optional (or null) for Manual work and pins
a positive immutable Task Agent Profile revision, not a live Workspace selection.
Projectless Tasks receive a profile-less Implementation baseline.

The application exposes `validate_record_check_profiles` for later Dispatched
eligibility gates: every Check must have a pinned registered revision whose
prerequisites pass profile preflight. Missing/unusable revisions yield Check-specific
`invalid-check-profile` diagnostics; validation never substitutes a revision, skips
a Check, rewrites a binding or launches an agent. Restore prerequisites or use
Manual work; an existing Task cannot repair a binding by rebinding. Template
snapshots may retain unavailable positive revisions, making such configuration
inspectable without preventing profile-less Manual work. Normal Task dispatch
requires usable pins and all [dispatch safety gates](dispatch.md); Workspace
selection is never a replacement for a missing Check pin.

For sequential cumulative planning, run read-only `check-template preflight` before
opening any Task. It requires Implementation first, Publication last, and a pinned
registered Task profile with a launch policy for every future Dispatched Check.
`show` adds optional `stack_preflight.blockers`; an empty list means compatible.
Older servers omit the field; the preflight command fails closed on absence.
This additive response leaves old responses decodable; strict clients that reject
unknown fields need a matching revision. No schema or Management Protocol change
is involved. The observation shares the template read transaction, does not mutate
configuration or create Tasks, and does not certify hosting/provider readiness.
Concurrent replacement can still change future pins: record/recheck the version
and inspect actual created Checks before completing a plan.

Read the observed `version` before replacement. An unconfigured template reads as
version **0**, the profile-less Implementation baseline and empty history; its first replacement requires
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
Assignment capabilities have no template mutation operation. Stores predating the
schema-7 contracts require fresh development stores; no legacy Task backfill is
provided. The separate [Revision State upgrade](state-upgrades.md) workflow can
preserve schema-7 records when upgrading to schema 8.
