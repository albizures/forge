# Dispatcher operations

Forge's daemon owns dispatch state, provider resources, and reconciliation.
The CLI neither opens dispatch storage nor contacts Herdr directly. Herdr is a
provider adapter: provider state is never proof of Forge Task completion.

## Configure dispatch

The adapter requires Herdr Socket API protocol **22**, schema version **1**, and
an operator-selected existing Herdr workspace. Configure an absolute socket path
then select that provider workspace. Managed Worktrees use the fixed
`<forge-workspace-root>/.forge/worktrees/<project-slug>/<record-id>` layout:

```sh
forge dispatcher config set herdr-socket /absolute/path/to/herdr.sock
forge dispatcher config set workspace your-workspace-id
forge dispatcher config get
forge dispatcher reconcile
```

`herdr-socket` is the **daemon-to-Herdr Unix socket**, not the CLI-to-Forge
RPC URL. CLI endpoint resolution remains unchanged: `--server-url` overrides
`FORGE_SERVER_URL`; without either override, Forgeup resolves the Active Forge
Revision and its Managed Daemon. The CLI never connects directly to Herdr.

Relative socket paths are rejected. Workspace identity is an explicit argument;
`HERDR_WORKSPACE_ID` is not required. `config get [key]` reports effective values
and default/override provenance, not reachability. Both provider keys default to
unconfigured. Use `config unset herdr-socket` or `config unset workspace` to
remove an override explicitly; a setter missing its value fails rather than
clearing it. Safe removal blocks new provisioning until required configuration
is restored; it does not retire existing Agents. Socket/Workspace changes are rejected while
unresolved Assignments or retained resources depend on them: inspect `status`
and `assignment-info`, then follow safe recovery before retrying. A bounded reconciliation
probes the provider and progresses eligible assignments; its response reports
lease acquisition and assignments scanned. Socket/protocol/provider failures
produce diagnostics under the daemon's durable retry policy.

Forge retains and verifies the selected workspace identity. Missing or unverifiable
workspace state blocks dispatch. Forge creates an unfocused dedicated tab and
uses its initial pane per Assignment; Task tabs run in their Managed Worktree,
never the Project Registered Folder. Forge does not own or delete the shared
workspace. See [the workspace ownership decision](../crates/docs/adr/0008-operator-selected-dispatch-workspaces.md).

## Capacity and selection

Forge selects the oldest dependency-ready, single-Project Open Task when Project
capacity is available. It atomically creates a Provisioning Assignment to reserve
capacity, then prepares `feat/forge-<task-id>` in a Task Managed Worktree. Its
immutable Base Commit is captured from the Registered Folder's `HEAD` when the
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
General model/prompt/profile overrides are not provided; acknowledgement timeout
has its own grouped configuration key below.

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
Specs, Wayfinders, and generic Records are unsupported.

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

Task Agents run the generated `forge assignment bootstrap` invocation using
their delivered environment. The lower-level `forge assignment task-start`
retains its Assignment Capability contract; operators should use bootstrap
rather than manually copying credentials.

The bootstrap authenticates its acknowledgement, separately starts the Task with
bounded retries, and only then retrieves rendered instructions through an
assignment-scoped authenticated read. Acknowledgement makes the Assignment
Working; it does not itself start the Task. If start cannot be confirmed, the
agent must not begin implementation. Failed instruction retrieval also stops
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

Back up existing databases before upgrades; durable dispatch facts migrate
automatically. Forge does not adopt legacy workspace/worktree state. Automated Git
cleanup/repair, unknown-resource adoption/replacement, general cancellation beyond
retirement, and automatic shared-workspace conflict resolution are not provided.
Integration and release decisions remain explicit workflow actions.
