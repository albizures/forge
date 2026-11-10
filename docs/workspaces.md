# Forge Workspaces

A **Forge Workspace** groups Projects and Records beneath one fixed canonical
filesystem root. It owns Task/Grilling profile selections and a binding to an
existing Herdr Workspace. A **Herdr Workspace** is an operator-owned terminal/UI
container, not Forge's filesystem boundary. No Herdr plugin or upstream change is
required.

## Start with local planning

Create an existing directory containing a main Git checkout at
`/repos/personal/api`, with an `origin` remote and an initial commit. Substitute
those paths below. These commands work before profiles or Herdr are configured:

```sh
forge workspace add personal --root /repos/personal --name 'Personal projects'
forge workspace list --json
forge workspace show --workspace personal --json
forge add project /repos/personal/api api --workspace personal
forge here --project personal/api --json
forge spec open --body 'Deliver the API' --project personal/api
forge ready --workspace personal --planning --json
```

Use the returned Spec ID with `forge task under-spec --spec <spec-id> --body
'Implement the API' --workspace personal`. See [Records](records.md) for planning,
relationships and lifecycle; dispatch configuration is not a prerequisite.

## Manage registrations and selection

```sh
forge workspace here
forge workspace here --workspace personal
forge workspace rename personal 'Personal work'
forge workspace add empty --root /repos/empty
forge workspace remove empty
```

These are independent examples: `/repos/empty` must exist and must not overlap
any other Workspace. IDs and roots are immutable; only the nonblank display name
can change. Roots must resolve at registration. Equal, ancestor and descendant
canonical roots, including aliases through symlinks, are rejected.

Selection is explicit `--workspace <id>` first, otherwise canonical client cwd
containment. A qualified Project reference such as `personal/api` also selects
its Workspace. Conflicting explicit selectors fail; explicit selection overrides
cwd even when cwd belongs to another Workspace. There is no persisted default.
Outside registered roots, commands requiring selection need an explicit selector;
bare Record IDs do not select a Workspace. Discovery (`workspace list`) and
registration do not require a selected Workspace. The daemon never substitutes
its own cwd for the client's.

A local Project slug is unique only within its Workspace: `personal/api` and
`work/api` are independent identities. Within a selected Workspace, `--project
api` selects its local Project. Without an explicit Project, `forge here` selects
the deepest containing canonical Project root. A Workspace directory outside all
its Projects has no inferred Project. Creation still requires its documented
Project or parent arguments; cwd is not implicit creation scope.

Selection is routing for a trusted operator, **not authentication or OS/process
isolation**. Keep the unauthenticated operator endpoint restricted to trusted
clients. Worktrees separate checkouts, not hostile processes running as your user.

## Filesystem and Record boundaries

A Project root may equal the Workspace root or nest beneath another Project
root, but must be a distinct main Git checkout with an `origin` remote. A checkout
subdirectory, duplicate canonical root, external path or symlink escape is
rejected. Neither manually created nor Forge-created linked Worktrees can be
registered as Projects. See [Projects](projects.md).

Managed Worktrees have exactly this layout, including for nested Projects:

```text
<workspace-root>/.forge/worktrees/<project-slug>/<record-id>
```

The reserved subtree is relative to the Workspace, not each Project. Other
`.forge` metadata directories are not reserved against Project registration.
Placement overrides and a global worktree base are unavailable. Forge validates
canonical existing ancestors before Git/filesystem effects, rejecting redirected
or dangling symlinks; force and recovery cannot bypass that validation. Unrelated
manual Git operations are not policed. See [Managed Worktrees](worktrees.md).

Every Record, including Projectless Records, Initiatives and Wayfinders, belongs
to one Workspace. Multi-Project scope, children, dependencies, provenance and
Initiative membership cannot cross Workspaces. Empty Project scope means
Workspace-local planning, not daemon-wide work.

## Dispatch settings and readiness

| Owner | Settings |
| --- | --- |
| Daemon/store | Herdr socket endpoint |
| Forge Workspace | Task profile revision, Grilling profile revision, existing Herdr binding |
| Project | Concurrency limit and setup hook |
| Immutable profile revision | Acknowledgement timeout and other supported profile configuration |

Use [dispatch setup](dispatch.md#configure-dispatch) after local planning.
`dispatcher config get` labels ownership and effective values. Both supported
profile selections and a binding are required, even for only one dispatch type.
`dispatcher status` distinguishes configuration completeness from current
dispatchability: missing/invalid profiles, absent binding, unreachable provider,
missing/unverifiable identity and stale verification have separate blockers.
`ready` is workflow readiness, not a promise of agent provisioning. Local
registration, Records and configuration remain usable during provider outages.

The binding verifies an exact existing Herdr identity and is exclusive within
the daemon/store. Forge never creates, adopts by label, replaces or deletes that
container. Task launches explicitly use their bound Managed Worktree as cwd;
Grilling launches explicitly use the Forge Workspace root and include applicable
Project references and canonical paths in their instructions. Neither depends on
Herdr's cwd policy, a focused pane, or equality with an alleged Herdr root. Other
tabs and Herdr's global cwd configuration remain unchanged.

Profile selection changes affect future Assignments only. Existing attempts,
retries and recovery retain their original immutable revisions and provider
identities. Binding changes/removal are blocked by that Workspace's live
Assignments and unresolved resources, including closed attempts. Socket changes
apply that guard daemon-wide and invalidate all Workspace provider verification;
repeat `dispatcher config set workspace <bound-id>` in each Workspace for fresh
verification. Availability and identity are checked again before provisioning.
Follow [Assignment recovery](assignment-recovery.md), rather than changing
configuration to bypass a hold or uncertain resource.

## Non-destructive removal and development stores

Removal requires no Projects, Records, Assignments or retained resources; clear
profile selections and the binding as well. Clearing configuration does not
retire resources. Removal deletes only an empty registration, never root files,
repositories or Herdr containers. Workspace-root relocation, Project reparenting
and cascading deletion are unavailable.

There is no legacy-installation migration/import requirement. Owned development
schemas can evolve while retaining their durable facts, but pre-Workspace unowned
data is rejected with an actionable new-store diagnostic, not silently adopted
into a default Workspace. Preserve existing evidence before changing stores; a
new store is not a way to reconcile an uncertain old agent.

Author and register immutable profile revisions using [Agent Profile policy](agent-profile-policy.md);
registration never changes a Workspace selection. For the delivered agent's
bounded bootstrap, actions and subscriptions, see [Assignment context](assignment-context.md).
