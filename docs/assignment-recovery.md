# Assignment recovery

Use this ordered runbook to investigate uncertain launches, revoke Assignments,
verify provider cleanup, and safely restore dispatch eligibility. For initial
configuration and launch behavior, see [dispatch operations](dispatch.md). Durable
(pi-durable) Assignments follow the same investigation, retirement, salvage and hold
steps; their backend-specific operations are in
[Durable Task execution](durable-execution.md#operate-durable-assignments).

## Trust and upgrade boundary

Back up the Forge database before opening it with new binaries. Owned development
schemas can evolve, but pre-Workspace unowned data requires a new store; legacy
import or implicit adoption is unsupported. Preserve Assignment, resource, audit
and worktree evidence. Do not edit SQLite, adopt
lookalike provider resources, delete worktree claims or launch a replacement to
bypass a recovery blocker.

The operator HTTP/RPC surface is **trusted and unauthenticated**. Restrict daemon
access to trusted operators. Rejecting agent credential-shaped requests is input
validation, not authentication. An Assignment Capability grants only the agent's
scoped operations; it is not operator authority. Diagnostics exclude Task/profile
prompts, capability values and secret-file paths; treat provider identities and
operational evidence as sensitive nonetheless.

## Investigate before changing state

Select the owning Forge Workspace with `--workspace <id>` or canonical client cwd;
outside registered roots, supply an explicit selector. Bare Record/Assignment IDs
do not select a Workspace. Use qualified Project references (`personal/api`) or a
local slug within the selected owner. Selection is routing, not authentication.
Existing attempts retain their original Workspace, profile revision and provider
resource identities across restart; changing current selections never redirects
cleanup. Binding changes are guarded by the owning Workspace's live Assignments
and retained resources; socket changes apply that guard daemon-wide. After a safe
socket change, every Workspace needs fresh binding verification before dispatch.
See [Workspace ownership](workspaces.md). Missing profile sources or direct
resource files do not prevent authenticated operations, reconciliation or
retirement of an existing Assignment. Repairing them may unblock a new launch,
but does not clear holds or resolve uncertain resources; see
[profile preflight and drift](dispatch.md#profile-preflight-and-drift).

```sh
forge dispatcher assignment-info <assignment-id>
forge dispatcher assignment-info <assignment-id> --json
forge dispatcher status --project <slug>
forge worktree info <task-id>
forge worktree doctor
```

Inspection shows persisted Task/Assignment state, launch and acknowledgement
facts/deadline, Dispatch Hold/reason, resource identities/retirement, recovery
blockers, capacity reservation and any unknown-resolution audit. It works for
closed Assignments. It does not contact Herdr: a missing timestamp or retained
identifier is not proof of absence or current ownership. Investigate the actual
provider resources and external processes separately; retain evidence without
copying capabilities or prompt contents into logs or tickets.

A missed acknowledgement deadline retains the Assignment and capacity. Clearing
the hold cannot replay a launch or prompt; reconciliation may reapply the timeout
hold. Late acknowledgement can still make the existing Assignment Working and
allow its Task to start without clearing that hold. Do not retire an otherwise
healthy agent just because its acknowledgement was late.

## Durable Assignments

For a durable Assignment, also read its
[status and transcript](durable-execution.md#status-and-transcripts-read-only), and handle
its backend-specific state in Durable Task execution before the steps below:

- `interrupted-effect` blockers: [interrupted effects](durable-execution.md#interrupted-effects).
  Nothing below waives them.
- `durable-runtime-*` codes and fence proof:
  [leases and outages](durable-execution.md#authorization-leases-and-outages). Recovery
  after a proven crash or fence is automatic
  ([same-attempt recovery](durable-execution.md#same-attempt-recovery)).
- `durable-launch-*` codes: [failed launches](durable-execution.md#failed-launches).
- Missing Outcome: [follow-ups](durable-execution.md#follow-ups) or retirement.
- After retirement: [runtime history deletion](durable-execution.md#runtime-history-retention-and-deletion).

## Revoke and clean up known resources

```sh
forge dispatcher assignment-retire <assignment-id> --json
forge dispatcher assignment-info <assignment-id> --json
```

Retirement installs a Dispatch Hold and durable cleanup evidence, closes/revokes
the Assignment **before** provider cleanup. It
checks retained ownership identities before destructive requests and verifies
agent/tab/pane absence afterward. It retains Git/worktree evidence and does not
delete the operator-selected Herdr Workspace or the Forge Workspace root. Revocation cannot undo external
actions or prove that an external child process is dead.

If cleanup fails, identities mismatch, resources survive, or the provider is
unavailable, stop and investigate. Closed does not mean capacity-free: unresolved
known resources, retirement-pending evidence and provisioning uncertainty survive
restart and reserve capacity. Correct the provider/endpoint problem and retry the
same `assignment-retire`, including for an already closed Assignment. Do not
bypass ownership checks or treat a successful mutation alone as verified absence.

Provider success alone also cannot finalize retirement. Forge atomically requires
revoked authority and every other retained resource to be reconciled before
retiring its `retirement-pending/operator` marker. Otherwise `assignment-retire`
returns `assignment-retirement-unreconciled`, with no success JSON, even if known
provider cleanup succeeded. The marker, capacity reservation and Dispatch Hold
remain. Generic provider resource cleanup cannot retire this marker. Exact
finalization retries preserve its original retirement timestamp.

For a durable Assignment, the same command reconciles the original host's receipt instead
of Herdr resources; see [durable retirement](durable-execution.md#retirement).

## Resolve unknown resources only after external safety checks

A crash may leave a provisioning-intent marker without the resource identity
Forge would need to clean it up. Known-resource retirement alone does not resolve
this uncertainty. First revoke the Assignment, verify known-resource cleanup,
investigate unknown resources and make the old agent and any external children
unable to act. Remove unknown resources or otherwise make them safe. Only when
both statements are actually true:

```sh
forge dispatcher assignment-resolve-unknown <assignment-id> \
  --confirm-old-agent-cannot-act --confirm-unknown-resources-safe --json
forge dispatcher assignment-info <assignment-id> --json
```

Both independent confirmations are mandatory; an open/unrevoked Assignment is
rejected. An incorrect assertion risks duplicate agents and concurrent external
work. Confirmation is a deliberate operator safety override, **not proof of
process death or an authenticated human identity**. The immutable audit retains
the timestamp, authority boundary and confirmation text across restart; repeated
calls preserve the original audit.

Only provisioning-intent uncertainty is resolved. Known resources,
retirement-pending and other blockers remain untouched and reserve capacity until
separately resolved. The Dispatch Hold remains, too. Inspect again rather than
assuming the command's success means recovery is complete. After resolving
unknown provisioning resources, retry the same `assignment-retire` to reconcile
and finalize its retained retirement marker. This does not clear the hold or
waive any other blocker. The generic finalization gate itself supplies no
process-death proof; durable cleanup additionally requires the exact protected
supervisor receipt ([durable retirement](durable-execution.md#retirement)).

## Authorize retained Implementation work for salvage

Unreported changed HEAD is **not** an accepted Continuation. Ordinary dispatch or
clearing a drift hold never adopts that commit. After retirement and complete
known/unknown resource reconciliation above, inspect the actual checkout and any
external children. Only when the old agent cannot act and all resources are safe:

```sh
forge record check work <task-id> --workspace <id> --json
forge dispatcher recover-retained-work <task-id> --workspace <id> \
  --source-assignment <assignment-id> --generation <current-generation> \
  --repository <registered-project-root> --commit <full-HEAD> \
  --actor <operator> --message <salvage-explanation> \
  --confirm-old-agent-cannot-act --confirm-resources-safe --json
```

This is a trusted operator assertion, not authenticated human identity or automatic
proof of process death. Both confirmations and nonblank attribution are mandatory.
The Task must be Dispatched, In Progress and single-Project, with its latest
Check-bound Assignment explicitly retired and resources fully reconciled. Its
unpassed Check must be Implementation, with no accepted outcome other than an
interruption (no outcome is also permitted). Recovery rejects Open/unclaimed Tasks,
review attempts, passing Implementation, stale generations, mismatched scope,
Repository, branch, placement, Base Commit ancestry or changed observed HEAD.
Git observations run outside the writer transaction; Forge ownership, claim,
source attempt and freshness are revalidated atomically. Operators must exclude
external Git writers during inspection/authorization; Forge cannot lock out
arbitrary same-user filesystem mutation.

Clean commits and dirty tracked, staged and untracked work are supported. The
command selects HEAD and independently observes retained state twice. For dirty
input, `recoveries[].retained_state.sha256` binds HEAD, index entries/stages/flags,
extended index flags (including intent-to-add), file names, file modes and working
contents (including untracked/ignored files),
without committing, stashing, copying or discarding anything. The digest is salvage
identity, **not clean checkpoint evidence**. Inspection failures remain unknown:
links in dirty input, submodules, unreadable files and unsupported path encodings
fail closed with `recovery-state-unknown`; preserve them for explicit investigation.
Exclude external writers throughout authorization and dispatch.

Clean and dirty recovery both revalidate canonical Repository/branch/base/claim
before the first successor claim, including after restart. Dirty recovery also
revalidates the exact retained fingerprint. Repository configuration cannot hide
untracked files from cleanliness checks; assume-unchanged/skip-worktree index flags
conservatively require dirty salvage rather than waiving tracked-byte validation.
Schema 19 uses v2 fingerprints; historical v1 dirty authorizations are retained but
fail closed, with no automatic in-place reauthorization (see [state upgrades](state-upgrades.md)). Changed bytes
or staged state retain an actionable drift hold and launch nothing. Exact request
replay returns the original audit, never new authority for changed bytes; conflicts
cannot overwrite it. The exception is consumed by reserving the first successor,
not by bootstrap, preventing crash/restart from duplicating an actor. A failed or
uncertain launch still requires normal retirement/resource reconciliation; it does
not renew salvage authority. Do not clean or reset work to satisfy dispatch.
The replacement receives the original checkout and explicit recovery context.
Its Continuation/success must report actual clean committed output: dirty recovery
input never waives output, review or publication validation.

Authorization atomically records selected evidence, source Assignment, actor,
explanation, confirmations, canonical path and timestamp. It advances freshness,
invalidates current passes/attempts while retaining all historical Assessments,
outcomes and Findings, and selects retained HEAD plus the separate dirty-state identity as Implementation input.
It does not backfill a Continuation, pass a Check, certify correctness, change the
immutable Stack Base Commit, reset any budget, clear existing holds, or launch an
agent. A hold is retained (existing safety reasons win). Exact retries for the same
source Assignment return the original authorization, including after restart or
replacement; conflicting retries have no effects. Inspect `record check work`:
`recoveries` contains the durable audit. The next eligible same-Check attempt
captures it in `bound.checkAttempt.recovery` for inspection and salvage; normal
Checks, Findings, review/publication and dispatch gates still apply.

Inspect again, then clear the hold separately below if justified. Authorization
is distinct from `worktree recover`: that older command adopts an **unclaimed Open
Task checkout**, not interrupted In Progress work or unexplained commits.

## Clear the hold separately; eligibility is not a launch guarantee

After the previous Assignment is closed and all known and uncertain resources
are safely resolved, review Task state, dependency readiness, Project capacity,
Workspace and worktree health. Forge-managed In Progress Tasks with durable
Check-bound Assignment history continue without reopening: accepted interruption
or safely reconciled explicit retirement retries the same Check, within its
persisted budget, after the hold is explicitly cleared. Manual/unmanaged In
Progress status or missing ownership alone never authorizes continuation.
Retirement does not complete or automatically reopen a Task. Use explicit safe
mode handoff for Manual work; do not invent Check history to resume dispatch.

```sh
forge dispatcher resume <task-id>
forge dispatcher status --project <slug>
forge dispatcher reconcile
```

`resume` atomically clears the operator-controlled hold and resets the Task's
interrupted-Check retry counts. It leaves remediation budget unchanged and does not
resolve blockers, prove agent absence, redeliver a prompt or itself authorize
replacement. Its JSON
`resolves_recovery_blockers: false` and `authorizes_replacement: false` describe
operation semantics, not a live eligibility verdict. Replacement still requires
normal dispatch checks, including an eligible Open or Forge-managed In Progress
Task and no unresolved old Assignment/resource evidence. `forge dispatcher dispatch <record-id>` applies
the same safety checks: it returns an existing open Assignment rather than
launching a duplicate and rejects replacement while old resource blockers remain.
It does not bypass a Dispatch Hold. Grillings require explicit dispatch; they are
never selected by automatic reconciliation. Inspect the resulting Assignment;
never create a manual duplicate to work around withheld capacity.

## Check evidence, bounded progress and completion

Inspect `forge record check work <task-id> --json`, current Checks/Assessments and
Findings alongside Assignment/resource evidence. Dirty/invalid checkouts,
unexpected commits, stale freshness, unlinked blockers and human-input outcomes
can retain diagnostic holds. Forge never commits or discards changes to recover.
Description edits and substantive handback invalidate current passes without
removing Findings/history; both original reviews rerun after Implementation.

Two automatic retries per interrupted Check and three remediation cycles per Task
run bound progress across restart, redispatch and mode changes. No progress stops
earlier. `resume` renews interrupted retries, but cannot bypass remediation
exhaustion or no-progress state. Only a recorded operator
`dispatcher reset-remediation <task> --actor <operator> --message <explanation>`
after safe ownership/resource reconciliation resets remediation budget; it does
not reset interruption counts or clear holds, Findings or other safety state.
See [bounded progress](dispatch.md#bounded-progress-and-operator-intervention)
for interrupted-Check recovery through explicit resume; Manual handoff is not required.

A final pass is not Done. Reconciliation separately requires confirmed retirement,
all resources reconciled, clean current evidence/freshness, all current passes,
no blockers/holds and satisfied dependencies. Managed Worktrees/branches remain
retained after completion; no automatic Git integration or cleanup occurs.

Continuation accounting is independent: 20 total or two consecutive unchanged
checkpoints exhaust a Check. `record check work <task> --workspace <id>` (or `--json`)
shows counters, retained handoff and reset audit. Only
`dispatcher reset-continuations <task> --check <check> --workspace <id> --actor <operator> --message <explanation>`
renews that Check's total and consecutive-unchanged allowance. It retains holds,
ownership/resources, evidence and all other budgets/history, and launches nothing.
After investigating the limit and recording a justified reset, complete the separate
retirement/resource checks above before explicit resume. Neither resume nor remediation
reset bypasses exhausted continuation accounting. See
[continuation operations](dispatch.md#deliberate-implementation-continuations).

## Spec parent and inheritance rejections

An Open Spec is planning-only: its child Tasks cannot start, claim an attempt or
launch. Finish the plan and explicitly start the Spec after validation. An In
Progress upgraded Spec is revalidated too; lifecycle status alone does not establish
valid Builds-On inheritance or exact predecessor evidence. `invalid-spec-plan` identifies
missing inheritance; `stack-blocked` identifies unprepared, mismatched or stale
Base Commit/predecessor evidence. Neither diagnostic authorizes a checkout reset,
merge, rebase, Check repin or deletion of retained evidence.

Inspect the Record relationships, `forge task stack show <task-id>`, Checks/work state,
Assignment info and worktree info first. Retire/reconcile old ownership as above;
unknown resources, retained resources, capacity, holds and freshness still gate
replacement. Ordinary root/prerequisite edits remain forbidden after Spec start.
There is **no supported in-place conversion** of a started legacy plan missing
Stack configuration, nor a supported repin of a wrong prepared Base Commit.
Stop and retain that evidence for an explicitly designed operator recovery; do
not edit SQLite or reopen the Spec to bypass structural protection.

If configuration and immutable pins are already correct, a safe checkout/evidence
correction need not replan the Spec. After confirming retirement and resources,
inspect and correct deliverables deliberately outside automatic dispatch, then
use the existing explicit work invalidation operation with current generation,
verified Repository evidence and attribution. This advances freshness, invalidates
current passes and closes stale attempts while retaining historical outcomes,
Checks, Findings and pins. It does not clear a hold or retry budget. Clear the hold
separately only after investigation; the next claim rechecks all execution gates.
An initially wrong checkout is rejected without recording a new attempt. Never
claim a correction succeeded merely because a hold was cleared.

## Verification and deployment limits

This runbook is not proof of a published release, verified installation, successful
rollback rehearsal, or production migration/recovery. Provider cleanup cannot
establish universal process death, and recovery checks do not guarantee the
absence of every SQLite failure. Do not distribute capability files with
investigation evidence.

See [development verification](development.md#verification) for quality gates and
known test/lint limitations. A deployment owner must decide whether current
verification satisfies release policy; these instructions do not waive it.
