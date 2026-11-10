# Assignment recovery

Use this ordered runbook to investigate uncertain launches, revoke Assignments,
verify provider cleanup, and safely restore dispatch eligibility. For initial
configuration and launch behavior, see [dispatch operations](dispatch.md).

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
See [Workspace ownership](workspaces.md). Missing profile companions or direct
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

## Revoke and clean up known resources

```sh
forge dispatcher assignment-retire <assignment-id> --json
forge dispatcher assignment-info <assignment-id> --json
```

Retirement installs a Dispatch Hold and durable cleanup evidence, closes/revokes
the Assignment and removes its subscriptions **before** provider cleanup. It
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
assuming the command's success means recovery is complete.

## Clear the hold separately; eligibility is not a launch guarantee

After the previous Assignment is closed and all known and uncertain resources
are safely resolved, review Task state, dependency readiness, Project capacity,
workspace and worktree health. For an unassigned manual In Progress Task that
should run again, explicitly use `forge dispatcher task-reopen <task-id>`;
retirement does not complete or automatically reopen the Task.

```sh
forge dispatcher resume <task-id>
forge dispatcher status --project <slug>
forge dispatcher reconcile
```

`resume` only clears the operator-controlled hold. It does not resolve blockers,
prove agent absence, redeliver a prompt or itself authorize replacement. Its JSON
`resolves_recovery_blockers: false` and `authorizes_replacement: false` describe
operation semantics, not a live eligibility verdict. Replacement still requires
normal dispatch checks, including an eligible Open Task and no unresolved old
Assignment/resource evidence. `forge dispatcher dispatch <record-id>` applies
the same safety checks: it returns an existing open Assignment rather than
launching a duplicate and rejects replacement while old resource blockers remain.
It does not bypass a Dispatch Hold. Grillings require explicit dispatch; they are
never selected by automatic reconciliation. Inspect the resulting Assignment;
never create a manual duplicate to work around withheld capacity.

## Verification and deployment limits

This runbook is not proof of a published release, verified installation, successful
rollback rehearsal, or production migration/recovery. Provider cleanup cannot
establish universal process death, and recovery checks do not guarantee the
absence of every SQLite failure. Do not distribute capability files with
investigation evidence.

See [development verification](development.md#verification) for quality gates and
known test/lint limitations. A deployment owner must decide whether current
verification satisfies release policy; these instructions do not waive it.
