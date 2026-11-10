# Dispatcher operations

Forge's daemon owns dispatch state, provider resources, and reconciliation.
The CLI neither opens dispatch storage nor contacts Herdr directly. Herdr is a
provider adapter: provider state is never proof of Forge Task completion.

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
default/override provenance without contacting Herdr. The acknowledgement timeout
belongs to the selected immutable Task profile revision, not an independent
Workspace setting. `profile [--kind task|grilling]` inspects that Workspace's selected
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

Forge selects the oldest dependency-ready, single-Project Open Task when Project
capacity is available. It atomically creates a Provisioning Assignment to reserve
capacity, then prepares `feat/forge-<task-id>` in a Task Managed Worktree. Its
immutable Base Commit is captured from the Project root's `HEAD` when the
Task opens.

Each Project defaults to capacity **1**. Set an override from 1 through 1024, or
explicitly unset it to restore the default. Capacity operations require `--project`:

```sh
forge dispatcher config set project-limit 2 --project alpha
forge dispatcher config get project-limit --project alpha
forge dispatcher config get --project alpha
forge dispatcher config unset project-limit --project alpha
forge dispatcher profile
```

The profile identifies provider, agent kind, model, and prompt configuration.
The current catalog contains seeded Task and Grilling revisions, plus Task timeout
revisions created by configuration. `task-profile` and `grilling-profile` select
existing supported revisions independently per Workspace; unknown revisions are
rejected atomically. General model/prompt overrides and a new profile-document
format are not provided. Changing a selection affects future claims only;
existing Assignments and recovery retain their original revision and provider identity.

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
a Managed Worktree or Task bootstrap and are never scheduled automatically.
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
launching Pi. Launch arguments contain a short bootstrap and profile model,
not rendered Task instructions or capability secrets. The capability is delivered
through an owner-only secret file outside the Agent Workspace.

Agents use `forge assignment bootstrap` with their delivered environment;
credentials are never supplied as command arguments. The authenticated bootstrap
route acknowledges and starts the assigned Record, then fetches trusted context
before returning separately labelled behavior. Grilling launch integration is
still pending. Acknowledgement makes the Assignment Working; it does not itself
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
