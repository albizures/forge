# Dispatcher operations

Forge's daemon owns dispatch state, provider resources, and reconciliation.
The CLI neither opens dispatch storage nor contacts Herdr directly. Herdr is a
provider adapter: provider state is never proof of Forge Task completion.

## Verified Check-driven Task dispatch

Normal explicit and automatic Task dispatch is enabled following Spec #644's
retained deterministic verification (#660, renewed after provider fixes in #661)
and six-attempt real-provider handback/remediation gate (#661 comment 2).
Before those gates, normal dispatch rejected with
`task-dispatch-unsupported-contract` and automatic scheduling made no Task claims.
The acceptance command did not enable dispatch or change Execution Mode. This is
a source-release gate, not a per-store flag or a claim that an installed daemon
has been upgraded. Use matched CLI/server builds and verify the deployed Revision.
Any subsequent implementation/review changes require renewed verification.

Explicit application/RPC/CLI requests still reject Manual Tasks with
`manual-dispatch-forbidden` and Projectless Dispatched Tasks with
`dispatch-requires-project`. Unsupported Record Types cannot execute through mode
metadata. See [explicit Execution Mode selection](records.md#explicit-execution-mode);
ordinary operator start never changes a Task's mode. Missing/unusable Check pins,
provider prerequisites, dependencies, holds, capacity and ownership/resources block
execution, with no Manual fallback. `task-dispatch-unsupported-contract` remains
a fail-closed diagnostic for unsupported persistence or unbound provisioning
Assignments; legacy ownership is not converted into Check-bound execution.

### Check attempts and controlled acceptance

For explicit acceptance testing only, `forge dispatcher task-check-acceptance
<task-id> --workspace <id> --json` executes **one** Check-bound attempt. It requires
a clean, active, already prepared Managed Worktree, Dispatched mode, a single
Project, satisfied dependencies, available capacity, no hold and reconciled prior
ownership/resources. In Progress continuation additionally requires durable
Check-bound Assignment history; lifecycle status or missing ownership alone is
not sufficient. Normal `dispatcher dispatch <task-id>` uses the same guarded
Check execution path, preparing a Managed Worktree first if needed for an eligible
Open Task. Automatic scheduling starts eligible Open Tasks; after a current
passing outcome or substantive review handback, daemon reconciliation retires the
old agent, reconciles its retained resources, then launches the next required
Check with its pinned profile. The explicit command does not create a worktree or
complete the Task; daemon reconciliation can complete enrolled work only through
the separate final gate below.
Repeating dispatch or acceptance returns the existing Assignment without relaunching it while
no passing outcome or freshness invalidation is available; both use the same
retirement gate.

Forge selects the first Check without a current pass and atomically reserves its
Assignment and attempt. The Check's immutable pinned profile is authoritative,
not the current Workspace Task selection or template. Missing, policy-less or
unusable pins fail without fallback. Workspace provider configuration and its
verified exact binding remain required.

The worker uses `forge assignment bootstrap --json`. Its trusted context includes
`bound.checkAttempt` (attempt id, Check id, starting generation, input evidence and
Task body), alongside the pinned profile and execution directory. Submit an
`OutcomeInput` JSON file using `forge assignment outcome --file outcome.json
--json`; credentials come only from protected Assignment delivery and `actor` is
replaced with the authenticated Assignment identity. Example file shape (use the
actual context identity/generation and a full commit):

```json
{
  "attempt_id": "<bound.checkAttempt.id>",
  "generation": 1,
  "evidence": {"kind": "repository", "repository": "<bound repository>", "commit": "<40 hexadecimal digits>"},
  "verdict": "pass",
  "actor": "",
  "message": "Deliverables verified",
  "disposition": "complete",
  "findings": [],
  "verifications": []
}
```

Implementation may report a new output commit in the same Repository. Reviews
must assess their bound output evidence and must not modify deliverables. Forge
observes the actual clean checkout at outcome acceptance and again after retirement
before selecting another Check. Implementation input A and authorized output B
remain distinct; every review binds B. A clean checkout may be ahead of its
upstream: removal safety is not the same as evidence freshness.

Dirty/missing/invalid checkouts or unexpected review/handoff commits invalidate
current Assessments and outstanding attempts, advance freshness, and retain a
diagnostic Dispatch Hold. No changes are committed, discarded, reset or cleaned.
Investigate the retained checkout and explicitly resume dispatch after repair.
Description edits also advance freshness, even at the same commit; replacement
returns to Implementation only after old ownership/resources are safely retired.
Historical Assessments and Findings remain, but invalidated passes never revive.
A false output commit claim is rejected without publishing an Assessment.

A failed substantive review submits `"disposition": "handback"` with its Findings.
Acceptance atomically records the failure and Findings, invalidates Implementation
and every review, and advances freshness even if the commit is unchanged. After
safe retirement, remediation starts at the reviewed commit and reruns all original
Checks in order, with their unchanged pinned profiles. Implementation may propose
fixes but cannot verify reviewer Findings; each originating Check verifies its own.
A passing review that leaves its own blocking Findings unresolved is rejected
without partial effects.

Reviewer-owned blockers allow remediation and review continuation without creating
a new hold. Unlinked blocking Findings stop continuation: explicitly assign a
same-Task review verification owner and clear the diagnostic hold. Merely clearing
the hold without assigning an owner cannot authorize another attempt. If every
Check passes but blockers remain, acceptance retains a diagnostic hold, never Done.
Existing holds are never automatically cleared, including after verified resolution.
Manual outcome execution retains its explicit operator-clearance policy.

Failed Implementation remains Implementation. Acceptance records evidence and outcomes,
not provider retirement or permission for another live attempt. Handoff atomically
revokes the old capability and retains a `check-handoff` resource blocker before
provider cleanup. Failed or uncertain cleanup retains capacity across restart;
an adapter's successful response cannot clear other retained resources. Successful
Herdr provisioning reconciles its `provision-intent` uncertainty marker only after
allocation and launch responses succeed and all resource identities are retained.
The one-shot marker row remains, preventing another allocation. Lost responses or
persistence failures retain uncertainty for explicit recovery; a restart does not
infer that an old unresolved marker represented successful provisioning. Known
provider resources still reserve capacity until verified retirement. Holds,
dependencies, mode, capacity, worktree safety and provider prerequisites are
rechecked before the next claim. An authenticated interrupted review retries the
same Check at the same freshness and evidence only after confirmed retirement and
complete resource reconciliation. It publishes no Assessment or Finding and does not
invalidate earlier passes. An attempt retired explicitly before submitting a result
can likewise retry only after its durable operator-retirement marker and all other
resources are reconciled, and the operator clears the retirement hold. Closure,
missing acknowledgement, absent provider resources or a lost launch response alone
never authorize replacement. Failed Implementation retries only Implementation,
under the same bounded remediation/no-progress policy as review handback.
The final passing outcome leaves the Task In Progress. Reconciliation first confirms
agent retirement and all retained resources, then observes the clean active Managed
Worktree again. A separate writer transaction rechecks Dispatched eligibility,
current evidence/freshness, every current Check's matching Assignment-attributed
pass and confirmed retirement, no open attempt, no blocking Findings or Dispatch
Hold (including bounded-progress holds), and satisfied dependencies before recording
Done. Dependency changes after the final review still block completion. Concurrent
scans record Done only once. Failed gates preserve safety state and history; they
never force-clear a hold, commit/discard changes, or remove the Managed Worktree.

Explicit operator completion of a Dispatched Task remains forbidden. Safely changing
Execution Mode requires between-attempt confirmed retirement/resource reconciliation,
nonblank attribution and recorded history; it invalidates current Assessments but
retains Findings, historical outcomes, holds and progress counters. Manual Tasks
still support explicit human completion under their Check/Finding, dependency and
ownership gates, without requiring a Managed Worktree. Exact outcome replay
returns its original result (including after closure/restart when the original
credential remains available); this replay-only path does not restore closed
Assignment reads or mutation authority. Conflicting, stale and cross-owner submissions have
no partial effects. Closed capabilities remain revoked.

## Bounded progress and operator intervention

Check-driven dispatch permits **two automatic retries per interrupted Check**
(three interrupted attempts total) and **three remediation cycles per Task run**.
A Check's successful outcome alone resets its interruption count; success of a
different Check, restarting, changing modes, redispatching or resetting remediation
does not. An explicitly retired, current attempt with no outcome consumes the same
interruption allowance once retirement and every retained resource are reconciled.
Substantive Implementation failure consumes remediation budget, not review retries.

Each handback/failed Implementation records a cycle baseline. At the next failed
Implementation, review handback or final passing review, unchanged deliverable
evidence with no newly verified Finding resolution is **no progress**. Verified
resolution of an overlooked fix counts even at the same commit. Earlier review
passes and new Findings alone do not establish progress. Exhaustion/no progress
retains an operator-cleared Dispatch Hold, never Done. Ordinary `resume` cannot
bypass an exhausted budget: the next acceptance reestablishes its diagnostic hold.
Existing safety holds are not overwritten.

Agents may submit `disposition: "human-input"` with `verdict: "fail"` and an
explanation, without Findings or verifications. This creates a human-input hold
and no substantive Assessment or handback; retirement/resource gates still apply.
The operator must intervene and explicitly resume. `interrupted` likewise creates
no Assessment, Finding or substantive handback.

Inspect counters, the pending baseline, verified-resolution count, stop reason and
latest 20 recorded resets using `forge record check work <task> --json` (the
`progress` object in WorkState). All reset audit rows remain durably stored.
After investigation and safe retirement/resource reconciliation, authorize a new
remediation budget explicitly:

```sh
forge dispatcher reset-remediation 42 --actor operator --message 'Investigated; authorize further remediation' --json
forge dispatcher resume 42
```

The trusted operator RPC is `agentDispatch.resetRemediation` (`id`, `actor`,
`message`); it is not an Assignment capability operation. Nonblank attribution is
required and retained atomically with the reset. Reset clears only remediation
cycles, pending baseline and no-progress/exhaustion state. It never clears a
Dispatch Hold, interrupted retry count, Finding, Assessment, evidence, freshness,
ownership or resource blocker. To recover an exhausted interrupted Check, safely
hand off to Manual, successfully assess that Check, then explicitly clear the hold
and safely return to Dispatched. No resume, redispatch or mode transition silently
resets the safety budget.

Storage revision 7 requires a fresh store; older attempts cannot safely be given
zero retry/remediation counts. No conversion, inferred backfill or automatic reset
is provided. Safely retire ownership and reconcile resources before any separate
operator-directed store replacement.

## Configure dispatch

The adapter requires Herdr Socket API protocol **22**, schema version **1**, and
an operator-selected existing Herdr Workspace. Each **Forge Workspace** independently
selects a Task profile revision, a Grilling profile revision, and an exclusive
Herdr container identity. Select the Forge Workspace with `--workspace <id>` or
canonical cwd containment; there is no saved default. Qualified `--project
<workspace>/<slug>` also routes configuration/status; conflicting selectors fail.
Outside a registered root, supply an explicit selector. Configure an absolute socket path
then select the profiles and provider identity. The setup below assumes a Forge
Workspace named `personal` is already registered (see [local planning setup](workspaces.md#start-with-local-planning)).
Replace the socket path and Herdr ID with inspected existing values. Managed Worktrees use the fixed
`<forge-workspace-root>/.forge/worktrees/<project-slug>/<record-id>` layout:

```sh
forge dispatcher config set herdr-socket /absolute/path/to/herdr.sock --workspace personal
forge dispatcher config set task-profile 1 --workspace personal
forge dispatcher config set grilling-profile 1 --workspace personal
forge dispatcher config set workspace your-herdr-workspace-id --workspace personal
forge dispatcher config get --workspace personal
forge dispatcher status --workspace personal --json
forge dispatcher reconcile --workspace personal
```

`herdr-socket` is the **daemon-to-Herdr Unix socket**, not the CLI-to-Forge
RPC URL. CLI endpoint resolution remains unchanged: `--server-url` overrides
`FORGE_SERVER_URL`; without either override, Forgeup resolves the Active Forge
Revision and its Managed Daemon. The CLI never connects directly to Herdr.

Relative socket paths are rejected. Herdr identity is an explicit argument;
`HERDR_WORKSPACE_ID` is not used. The socket is **daemon-owned**; profile selections
and Herdr binding are **Workspace-owned**; capacity and setup hooks remain
**Project-owned**. `config get [key]` reports effective values, ownership and
default/override provenance without contacting Herdr. The effective acknowledgement
timeout belongs to the Assignment's immutable profile (the Task Check pin), not an
independent Workspace setting. Workspace selection/timeout changes never alter
existing Check pins or Assignments. `profile [--kind task|grilling]` inspects that Workspace's selected
revision. Profile selections and binding start unset; both selections are required,
even when dispatching only one Record Type.

Use `config unset <key>` explicitly; a setter missing its value fails rather than
clearing it. Binding changes/removal are rejected while that Workspace has live
Assignments or retained resources, including closed uncertain attempts. Socket
changes apply the same guard **daemon-wide**. A safe socket change invalidates every
Workspace's provider verification: repeat `config set workspace <bound-id>` in
each Workspace to verify it against the new endpoint. These operations never
retire Agents or delete a provider container; inspect `assignment-info` and follow
safe recovery before retrying.

`dispatcher status` separates **configured** (both supported profile selections
and a binding) from **dispatchable now**. It reports actionable missing/invalid
profile, missing binding, unreachable provider, missing provider identity and
unverifiable/stale binding blockers. Status probes Herdr; local configuration reads,
registration, Records, planning and workflow `ready` remain usable during outages.
Forge revalidates availability and exact identity before provisioning; provider
outages block new claims/launches without consuming Task retries. Reconciliation
can provision work, so use status rather than reconcile for read-only diagnostics.

Forge retains the exact provider identity in each Assignment. It never creates,
adopts by label, replaces or deletes a Herdr Workspace. A missing or unverifiable
identity blocks dispatch. Forge creates an unfocused dedicated tab and uses its
initial pane per Assignment. Forge explicitly supplies cwd: Task tabs run in their
bound Managed Worktree, never the Project root; Grilling tabs run at the owning
Forge Workspace root, with applicable Project references and canonical paths in
their initial instructions. Neither launch inherits Herdr's cwd policy or the
focused pane's directory, nor requires a matching Herdr filesystem root. Other
Herdr tabs and global cwd configuration remain operator-owned and unchanged.
No Herdr plugin or upstream change is required. See [the workspace ownership decision](../crates/docs/adr/0008-operator-selected-dispatch-workspaces.md).

## Capacity and selection

The remaining examples assume cwd is inside the intended Forge Workspace; add
`--workspace <id>` outside it, or use a qualified Project reference where accepted.
Workspace routing is not authentication or OS/process isolation.

Forge scans dependency-ready, single-Project Dispatched Open Tasks in ascending
Record ID (creation order), skipping blocked candidates rather than starving later
ready work. Before a new checkout it checks lifecycle, dependencies, holds,
ownership/resources, capacity, provider readiness and the next Check pin. It then
prepares `feat/forge-<task-id>` in a Task Managed Worktree, using the immutable Base
Commit. Non-stacked Tasks capture it from Project HEAD when opened; explicit
[Task stacks](stacks.md) resolve and pin it from their Base Branch during
preparation, with predecessor publication verification. The guarded
writer transaction rechecks mutable gates and atomically reserves the Check
attempt, pinned-profile Assignment and capacity before provider launch. Concurrent
requests cannot create a second live attempt. A race may leave a safely retained
checkout without an Assignment; preparation never resets or deletes it. Failed
preparation preserves its claim/evidence and hold for explicit repair.

Each Project defaults to capacity **1**. Set an override from 1 through 1024, or
explicitly unset it to restore the default. Capacity operations require `--project`:

```sh
forge dispatcher config set project-limit 2 --project alpha
forge dispatcher config get project-limit --project alpha
forge dispatcher config get --project alpha
forge dispatcher config unset project-limit --project alpha
forge dispatcher profile
```

Profiles include seeded revisions, registered immutable revisions and Task timeout
revisions. Author, preview and register a Markdown profile beneath the Forge
Workspace's `.agents/profiles/` directory as described in
[Agent Profile policy](agent-profile-policy.md); registration never selects it.
`task-profile` and `grilling-profile` select revisions independently per Workspace
and Record Type; unknown revisions are rejected. There is no automatic latest
selection. Workspace Task selection supplies authoring/lifecycle defaults and
configuration readiness, **not live Task attempt routing**. Configure each Project's
[Check Template](records.md#project-check-templates) with registered Task profile
revisions **before opening Tasks**. Every attempt, retry and remediation uses its
Check's copied revision; template or Workspace changes never rebind existing
Checks. The default profile-less baseline is usable for Manual work, not dispatch.
Missing/unusable pins return `invalid-check-profile`; restore prerequisites or
safely choose Manual, never fallback or rebind. Grilling dispatch uses the selected
Workspace Grilling revision. Existing Assignments and recovery retain their bound
revision and provider identity when selections change.

## Explicit Record dispatch

Automatic scheduling remains oldest-ready-first for Tasks only. To select a
particular eligible Task, or launch a ready interactive Grilling, use:

```sh
forge dispatcher dispatch <record-id>
forge dispatcher dispatch <record-id> --json
```

Explicit Task selection attempts the requested Task even if an older Task is
waiting. It still checks readiness, single-Project eligibility, dependencies,
Project capacity, Dispatch Holds, assignment uniqueness, and retained-resource
safety. Tasks use their Managed Worktrees; Grillings launch interactively without
a Managed Worktree, use authenticated Assignment bootstrap, and are never scheduled automatically.
Grillings may span multiple Projects within their Workspace; each scoped Project
must have capacity, and the Assignment reserves capacity in each, including while
a closed Assignment retains resources. Projectless Records remain planning-only
and are not dispatchable. Specs, Wayfinders, and generic Records are unsupported.

Repeated or concurrent requests return the existing open Assignment, reporting
`already assigned` (JSON: `already_assigned: true`) without another Agent launch
or capacity reservation. A closed Assignment with unresolved resources blocks
replacement. Explicit dispatch is not a recovery override: follow the
[recovery runbook](assignment-recovery.md) before retrying.

## Launch and acknowledgement

Forge persists a one-shot launch intent and acknowledgement deadline before
launching Pi. Pi starts without a positional prompt. Forge pins the returned
terminal identity and waits for the named Pi at the exact Workspace/tab/pane tuple
to become interactively ready with startup no longer pending, then sends the short
trusted bootstrap once through `agent.prompt`. Passing a prompt to Pi at startup
can keep Herdr's launch pending until its deadline, losing the name during a long
first turn. A successful `agent.start` response alone is not readiness.

The same durable one-shot intents cover start and bootstrap delivery: a timeout,
identity change or lost response retains resources and never retries the prompt or
launch. Successful provisioning reconciles its uncertainty marker only after the
bootstrap admission response validates the same identity. Launch arguments contain
the profile model, not rendered Task instructions or capability secrets. The
capability is delivered through an owner-only secret file outside the Agent Workspace.

Agents use `forge assignment bootstrap` with their delivered environment;
credentials are never supplied as command arguments. The authenticated bootstrap
route acknowledges and starts the assigned Record, then fetches trusted context
before returning separately labelled behavior for both Tasks and Grillings.
Acknowledgement makes the Assignment Working; it does not itself
start the Record. If bootstrap fails or is uncertain, the agent must not begin
work. Failed instruction retrieval also stops
bootstrap without permitting implementation.

The acknowledgement timeout defaults to **300000 ms**, separately from startup
timeout. Inspect the profile or create a timeout revision explicitly:

```sh
forge dispatcher profile
forge dispatcher config get acknowledgement-timeout-ms
forge dispatcher config set acknowledgement-timeout-ms 42000
forge dispatcher config unset acknowledgement-timeout-ms
```

Reads report effective values and default/override provenance. Timeout set and
unset create immutable Agent Profile revisions for future Assignments only;
existing Assignments retain their bound profiles. Unset restores **300000 ms**.

A missed deadline or interrupted uncertain launch retains the Assignment,
evidence, and capacity with a Dispatch Hold. Pre-upgrade Provisioning Assignments
without a deadline are uncertain, not automatically relaunched. Late
acknowledgement may still let the existing agent start, without clearing the hold.
**Do not infer agent absence or launch a replacement to bypass a hold.**

## Profile preflight and drift

Before a new claim, Forge checks the selected revision's direct inputs, supported
provider/profile, selected MCP configuration and neighboring CLI. A redacted
configuration blocker is distinct from a Dispatch Hold; repair the inputs or
explicitly select a supported revision and recheck status. Repair does not clear
an unrelated hold. A final direct-input check before launch can fail after
allocation; retain the resources and follow the recovery runbook. Existing
Assignment actions and recovery do not depend on source files remaining available.

Pi flags configure resource loading, but do not attest runtime identity or closed
execution. Selected MCP servers are currently unsupported; native reload is not
guarded. See [policy limits and runtime verification](agent-profile-policy.md#limits-and-verification).

## Ownership and authorization

Forge persists resource identities and revalidates them before destructive
requests. It never adopts lookalike resources. Missing or conflicting identities
are diagnostics, not permission to delete user resources.

An open Assignment Capability grants only bounded Assignment Context reads and
permitted actions for its bound work. The daemon is the authorization gateway;
closed, replaced, abandoned, or revoked Assignments immediately lose access.
Redacted authorization audits retain neither capability secrets nor Comment
bodies. The operator RPC surface remains trusted and unauthenticated; restrict
access to trusted clients. Capabilities do not protect against hostile processes
running as the same OS user.

Pi admission, idle, done, or blocked provider states only inform reconciliation.
Task completion requires an explicit authorized Forge action. Herdr observations
are wake signals: the daemon subscribes before snapshotting and reconnects after
failures, while authoritative scans recover from persisted identities. Events do
not directly mutate Assignments.

## Investigation and recovery

Start with retained evidence:

```sh
forge dispatcher assignment-info <assignment-id>
forge dispatcher status --project <slug>
forge worktree info <task-id>
forge worktree doctor
```

Follow the ordered [assignment recovery runbook](assignment-recovery.md) before
retirement, unknown-resource resolution, or `resume`. Clearing a hold does not
prove agent absence or authorize replacement. Revocation does not prove process
death or undo external actions. Closed Assignments can still reserve capacity.

Forge does not reset, rebase, delete, recreate, or automatically repair
dispatcher-created branches/worktrees. Failed setup hooks, branch collisions,
unavailable bases, invalid checkouts, and provider/workspace failures retain
diagnostics for investigation. Repeated definite provisioning failures can create
a hold; uncertain launches require the more conservative recovery runbook.

Back up existing evidence before upgrades. Owned development schemas can evolve;
pre-Workspace unowned data requires a new store, not legacy import or implicit
adoption. Forge does not adopt legacy workspace/worktree state. Automated Git
cleanup/repair, unknown-resource adoption/replacement, general cancellation beyond
retirement, and automatic shared-workspace conflict resolution are not provided.
Integration and release decisions remain explicit workflow actions.
