# Durable Task execution (pi-durable)

Forge can run a Task Check attempt headlessly through a Forge-owned runner built on a
pinned pi-durable release, instead of a Herdr tab running the Pi CLI. This guide is the
operator entry point for that backend: setup, supported configuration, migration of
existing Checks, rollout and rollback, and the day-to-day controls. The decision is
recorded in [ADR-0014](../crates/docs/adr/0014-headless-durable-task-execution.md),
with authorization, recovery and retirement in
[ADR-0012](../crates/docs/adr/0012-authorized-durable-recovery-and-conservative-retirement.md)
and explicit pin migration in
[ADR-0013](../crates/docs/adr/0013-explicit-backend-and-check-profile-migration.md).

## Current release state

**New durable launches are withheld in every production Revision built from this source.**
`forge dispatcher durable-launch-status` reports `withheld` and
`new_launches_admitted: false`. There is no command that enables them. Eligibility
belongs to the release handoff, after the retained release verification gate
(deterministic fault coverage plus a real-provider Implementation → Review cycle)
has passed on the exact Forge Revision. A happy-path demonstration is not that gate.

While launches are withheld or rolled back, every path that would start new durable
work refuses with `invalid-check-profile`:

- explicit `forge dispatcher dispatch` and automatic scheduling of a Task whose next
  Check pins a `pi-durable` revision;
- `forge record check-profile-migrate` to a `pi-durable` revision (the target is
  preflighted through the same gate).

Profile preview and registration are still accepted, so revisions can be prepared in
advance. Everything that inspects or cleans up existing work (status, transcripts,
follow-ups, effect resolution, retirement, purge) does not consult the gate.

Automatic recovery of crashed durable Assignments is not configured in the default
daemon composition either. The behavior described under
[Assignment recovery](assignment-recovery.md#same-assignment-durable-recovery-controlled-rollout-disabled)
is implemented and covered by controlled tests, not enabled operationally.

## Execution model

| Concern | Durable backend | Herdr backend |
| --- | --- | --- |
| Record Types | Task Check attempts only | Tasks and Grilling |
| Process | One supervised runner per Assignment, spawned by the daemon | Pi CLI in a dedicated Herdr tab |
| Conversation and store | One conversation and one SQLite runtime store per Assignment | Pi session files |
| Working directory | The Task's bound Managed Worktree | The Task's bound Managed Worktree |
| Runtime storage | `<workspace-root>/.forge/assignment-runtimes/<digest>`, outside the worktree and Forge's database | Not applicable |
| Workspace prerequisites | None from Herdr: no socket, binding or Workspace-wide readiness | Socket, both profile selections and a verified binding |
| Capability delivery | Protected owner-only file, same as Herdr ([ADR-0007](../crates/docs/adr/0007-assignment-capability-delivery-to-pi.md)) | Same |

The backend is chosen per Check by its **effective** pinned profile revision, never by the
Workspace Task selection. Once reserved, an Assignment keeps its backend, profile revision
and runtime identity for its whole life: changing selections, templates, migrations or the
launch gate never moves it to another backend. There is no Herdr fallback.

Forge stays authoritative. Dependency gates, Execution Mode, Spec authorization, capacity,
Check order, freshness, evidence, Findings, Continuations, Stack inheritance and publication
rules all apply unchanged. The agent must run `forge assignment bootstrap` before working and
submit its result through the existing authenticated Outcome commands
([Assignment context](assignment-context.md)). A runtime that goes idle, says it is done or
writes a plausible final answer never creates an Assessment, completes a Task or launches the
next Check. pi-durable's own internal tasks are **execution tasks**: runner machinery, not Forge
Tasks, Checks or Assignments.

Independent Reviews and Continuation successors get a fresh Assignment, conversation and
store, and receive only Forge-approved bound context and the accepted predecessor output,
never the predecessor's dialogue.

## Readiness, authorization, liveness and absence

These are different facts. Never use one as evidence of another.

| Fact | Established by | Does **not** establish |
| --- | --- | --- |
| Configuration readiness | Profile preview/registration preflight, matched components, a valid key file, `durable-launch-status` | That a launch is admitted, or that the provider accepts the key |
| Authorization | Forge's open Assignment plus a live Assignment Authorization Lease bound to the attempt, profile, freshness and host owner | That the process is alive or making progress |
| Liveness | A verified host owner, lease renewal, committed transcript entries | That the agent will produce an Outcome |
| Verified absence | The dedicated subreaper's protected receipt after the kernel reports no remaining children, reconciled by `assignment-retire` | That remote or delegated external effects did not happen |

A missing heartbeat, an expired lease, an absent runner, a successful abort, a cleared
Dispatch Hold, a revoked capability or an empty transcript page is **not** verified absence.
`dispatcher status` reports Herdr-oriented Workspace readiness; it is not a durable
readiness check.

## Supported configuration

| Area | Supported | Rejected or unsupported |
| --- | --- | --- |
| Platform | Linux x86-64; bundled Node 24.18.0; pi-durable 1.5.2 (with pi-ai and chord 1.5.2) | Other OSes, other Node versions, PATH/npm-selected runtimes |
| Record Type | Task | Grilling (`pi-durable supports Task profiles only`) |
| Model | `openai/gpt-4.1` | Any other provider or model |
| Thinking | `off` | Any other level |
| Tools | Unique subset of `read`, `write`, `edit`, `bash` | Other tools, extensions, MCP servers |
| Context | Selected Skill/Markdown resources as data, re-verified by hash, 256 KiB total | Automatic `AGENTS.md`, Pi settings, executable Pi skills |
| Model authentication | Raw OpenAI API key **file** named by `FORGE_DURABLE_OPENAI_KEY_FILE` on a manual daemon | OAuth, Pi `auth.json`/settings, key values in environment/arguments/YAML, copied Pi configuration |
| Daemon launch | Manual `forge-server`; Forgeup-managed daemon for inspection/cleanup components | Model credentials for a Forgeup-managed daemon (see below) |

**Known gap:** a Forgeup-managed daemon receives the Revision's matched runner components
but has no model-credential setting; it inherits none of the `FORGE_DURABLE_*` environment
values. Durable profile preview/registration there fails with
`durable-model-auth-unavailable`. Closing this gap is release work, not an operator workaround.

The key file must be an absolute canonical path to a regular, single-link, owner-owned
`0600` file in an owner-owned `0700` directory, containing one printable ASCII token
(1–4096 bytes) with an optional trailing newline. Diagnostics never include the key or its
path. The key reaches only the runner's in-memory provider auth: never the shell
environment, prompts, transcripts or Forge records. A model key grants no Forge authority,
and an Assignment Capability grants no model access.

See [Agent Profile policy](agent-profile-policy.md#durable-profiles-and-authentication)
for the full authoring and context rules.

## Install and run the matched runner

The runner, bundled Node and locked JavaScript dependencies are distributed as one matched
component of a Forge Revision. Build and register it with the Revision
([matched runner packaging](development.md#matched-runner-packaging)):

```sh
node runner/package-bundle.mjs --out /tmp/forge-runner-bundle --forge-commit <40-hex-commit>
forgeup register --source-label local --cli target/release/forge \
  --daemon target/release/forge-server --runner-bundle /tmp/forge-runner-bundle
forgeup runner verify <selector>
forgeup daemon start
```

`register` and `daemon start` attest the commit, runner protocol 1, Node version, lock hash
and content tree hash, and refuse a missing, modified or mismatched component with an
actionable message. `daemon start` passes the Installation's own `--durable-node` and
`--durable-effect-inspector` to the daemon. Global `PATH`, npm and the checkout are never
consulted. A Revision without a bundle has no matched components, so durable transcripts,
effect resolution and retirement reconciliation stay blocked there.
Removing a runner-bearing Revision requires `--purge-state` while its state exists, and
selecting another Revision never changes a retained Assignment's component.

For a manual daemon, name the components explicitly (both or neither; absolute paths only)
and, for profile registration, the key file:

```text
FORGE_DURABLE_NODE=/abs/path/bundle/node/bin/node
FORGE_DURABLE_EFFECT_INSPECTOR=/abs/path/bundle/runner/src/inspect-effects.ts
FORGE_DURABLE_OPENAI_KEY_FILE=/abs/owner-only-dir/openai.key
```

A partial or relative component selection stops the daemon at startup. Without components,
status still works from the store, but transcripts fail with
`durable-transcript-unconfigured` and effect resolution and retirement reconciliation keep
their blockers.

## Prepare a durable profile

Author a Task profile under the Workspace's `.agents/profiles/`:

```markdown
---
version: 1
runtime: pi-durable
model: openai/gpt-4.1
thinkingLevel: off
tools: [read, bash, edit, write]
---
Implement the bound Check.
```

```sh
forge agent-profile preview .agents/profiles/durable-impl.md --workspace personal
forge agent-profile register .agents/profiles/durable-impl.md --workspace personal --record-type task --json
forge dispatcher config set task-profile 3 --workspace personal
```

Registration selects the new revision as the Workspace **Task selection**. A selected durable
revision makes Workspace readiness report `task-profile-invalid`, which blocks Herdr Tasks and
Grilling in that Workspace. Durable Checks are reached through pins, not this selection, so
select a Herdr Task revision again afterwards (revision `3` above stands for your Herdr
revision).

To use durable execution for **new** Tasks, pin the durable revision in the Project's
[Check Template](records.md#project-check-templates) before opening them. Mixed backends across
a Task's Checks are allowed.

## Migrate existing Checks

Existing Checks keep their original pins. Changing a template, a Workspace selection or
the launch gate never moves them. The only way is an explicit audited migration:

```sh
forge record check-profile-migrate <task-id> --operation-id adopt-1 --expected-generation <current-generation> \
  --check 7=5 --actor operator --message 'adopt pi-durable' --json
forge record check-profile-migrations <task-id>
```

It requires a non-Done Task with no open attempt or Assignment and all known and unknown
resources reconciled. Live Herdr work finishes or retires through Herdr first. The original
pin, Check identity, description, order and every Assignment and Assessment stay as history;
freshness is invalidated once, so earlier passes are stale. Retry with the same operation ID
after a dropped reply; different content under that ID is rejected. While launches are
withheld or rolled back, migration to a durable revision refuses with `invalid-check-profile`.
Details: [migration rules](assignment-recovery.md#migrate-existing-check-profiles-to-durable-execution).

## Rollout and rollback

```sh
forge dispatcher durable-launch-status --json
forge dispatcher durable-launch-disable --operation-id rollback-1 --reason 'incident 42' --json
```

`durable-launch-status` reads persisted policy and starts nothing. `durable-launch-disable`
records a **sticky, immutable** rollback for the store: it survives daemon restart and Revision
switches, and a repeat returns the first rollback with `replayed: true`. There is no re-enable
command. Every new durable admission reads the rollback, and an unreadable policy fails closed.

A rollback affects new launches only. Existing durable Assignments keep their backend for
status, transcripts, follow-ups, recovery and retirement; nothing is moved to Herdr, and no
Check pin or Workspace selection is rewritten. Herdr Tasks and Grilling are unaffected. To
run a Task's remaining Checks on Herdr after a rollback, retire its durable Assignment safely,
then use an explicit compatible pin selection; switching backends is never automatic.

## Operate durable Assignments

All examples accept `--workspace <id>` and `--json`. These operator routes are trusted and
unauthenticated; restrict daemon access ([Security](../README.md#security)).

### Status and transcripts (read-only)

```sh
forge dispatcher assignment-status <assignment-id> --workspace personal
forge dispatcher assignment-transcript <assignment-id> --workspace personal --limit 50
forge dispatcher assignment-transcript <assignment-id> --workspace personal --after 42 --limit 100 --json
```

Neither command starts a runner, scheduler, model call, tool call or lease, takes host
ownership or opens storage writable. Both work for running, crashed, retired and
missing-Outcome Assignments. Transcript pages are committed entries in ascending immutable ID
order; pass the previous `next_after` as `--after`. An empty page for a live Assignment is not
end of history. Unreadable or corrupt storage reports `unavailable` with a reason code, never
an empty transcript, and is not repaired. Treat transcript content as sensitive repository data.
See [status and transcripts](assignment-recovery.md#inspect-durable-status-and-committed-transcripts-read-only).

### Follow-ups

```sh
forge dispatcher assignment-followup <assignment-id> --workspace personal --operation-id fix-1 \
  --message 'submit the result with the Outcome command' --json
```

A follow-up is recorded once in Forge, bound to the original attempt, Check, profile,
freshness and evidence; it cannot rebind any of them. When the runner is resumed under live
authority, the daemon delivers it idempotently (`delivery`: `recorded`, `unknown`, then
`delivered`). A record is not an Outcome, lease or proof that the agent acted. Revoked
Assignments, stale bindings and retained uncertainty reject new input. See
[operator follow-ups](assignment-recovery.md#operator-follow-ups).

### Missing Outcome

A conversation that ends without an accepted Outcome, including after a rejected Outcome,
gets a retained `missing-outcome` diagnostic. It holds capacity and a Dispatch Hold. Forge does
not synthesize an Assessment, Continuation, Task completion or successor. Intervene explicitly:
inspect the transcript, send a follow-up, or retire the Assignment. Clearing the hold with
`resume` does not create an Outcome.

### Authorization leases and outages

Execution needs a live Assignment Authorization Lease. Grants last 30 seconds and the daemon
renews them every 10 seconds; deadlines use the host boot clock. If the daemon is stopped or
unreachable, the runner cannot renew locally. At expiry its guards refuse new model requests
and tool calls, and an independent watchdog sends TERM to the runner's process group, then KILL
after at most 5 seconds. Forge retains capacity and diagnostics. A restarted daemon does not
renew old grants and does not restart the work automatically. Expiry is not proof that a child
process or its external action stopped. Plan for an Assignment to need recovery or retirement
after any daemon outage longer than a grant.

### Interrupted effects

Shell and file tools are not replay-safe. If a tool call's intent was committed without a
known result, Forge retains an `interrupted-effect` blocker that reserves capacity and blocks
grants. Arbitrary commands are never replayed. Investigate the real external outcome (push,
publication, file changes) and record it with evidence:

```sh
forge dispatcher assignment-resolve-effect <assignment-id> --workspace personal \
  --operation-id resolve-1 --execution-task <runtime-task-id> --conversation <conversation-id> \
  --call <call-id> --outcome applied --actor operator --evidence <receipt-reference> \
  --message 'push observed on remote' --confirm-old-agent-cannot-act --confirm-external-work-safe --json
```

`unknown` cannot be resolved. A resolution never reruns the command or authorizes recovery.
See [interrupted durable effects](assignment-recovery.md#interrupted-durable-effects-controlled-implementation).

### Retirement

```sh
forge dispatcher assignment-retire <assignment-id> --workspace personal --json
forge dispatcher assignment-info <assignment-id> --workspace personal --json
```

Retirement revokes authority **before** cleanup, then asks the original supervisor to stop
the runner's process group; its subreaper writes a receipt only when no children remain.
Forge reconciles that receipt and committed effects before releasing capacity. A surviving or
escaped child, missing proof, identity drift or unavailable components leave retirement
unreconciled (`assignment-retirement-unreconciled` or a `durable-retirement-*` diagnostic), with
capacity and hold retained; correct the cause and repeat the same command. Retirement needs no
profile source, model key or Herdr. See
[durable supervised retirement](assignment-recovery.md#durable-supervised-retirement-controlled-implementation).

### Runtime history retention and deletion

Retirement keeps the owner-only runtime history. Deletion is a separate, irreversible step:

```sh
forge dispatcher assignment-purge-runtime <assignment-id> --workspace personal --operation-id purge-1 --json
```

It deletes only the canonically mapped runtime directory of a closed, reconciled Assignment with
no retained uncertainty, hold or live owner. Retirement places a Dispatch Hold, so deletion is
refused (`runtime-purge-uncertainty-retained`) until the operator clears it with
`forge dispatcher resume <task-id>`. The retirement, crash and recovery evidence files the host
itself wrote are deleted with the runtime; anything else in the directory refuses deletion.
The result records whether this operation `removed` the directory or found it `already-absent`.
Forge's Assignment, Outcome, Assessment, migration, recovery and retirement records stay. Afterwards transcripts report `runtime-history-purged`,
and nothing recreates or resumes the store. See
[retained runtime history](assignment-recovery.md#retained-runtime-history-and-explicit-deletion).

## What is not a safe shortcut

Do not treat any of the following as proof that a replacement launch is safe: clearing a hold
with `resume`, reading a transcript, revoking a capability, an expired lease, a missing runner,
a free lock file, or a successful `assignment-retire` request without reconciled closure. Do not
edit Forge or runtime SQLite, delete lock/intent/receipt files, repair the worktree or replay a
tool command to make an Assignment look recoverable. Follow the
[Assignment recovery runbook](assignment-recovery.md).

## Trust boundary and limits

- **Trusted local host.** The runner, its tools and the daemon run as the same OS user. Shell
  tools can read anything that user can, including other Forge files. Separate processes are
  failure isolation, not a security sandbox; there is no hostile-agent containment.
- **Escaped processes.** A child that leaves the supervised process group, or work delegated to
  another service, is outside the subreaper's proof. Forge keeps the blocker; investigate manually.
- **No exactly-once effects.** Persistence, idempotent operation IDs and leases deduplicate
  Forge and runtime records, not external commands. An interrupted push may have happened.
- **No proof of death from persistence or leases.** Only the reconciled subreaper receipt counts,
  and only for this host's descendants.
- **Storage durability.** Tests cover process crashes, not power loss.
- **Sensitive data.** Runtime stores and transcripts can contain repository content and tool
  output. Do not attach them or capability files to tickets.

## Release verification requirements

Before default durable dispatch can be enabled for a Revision, the release owner needs a
retained verification report for **that exact Revision** that names the runner protocol and
dependency versions, the commands run, expected versus observed results and any unrun gates. It
must include a real-provider Implementation → Review cycle with authenticated Outcomes and exact
evidence, and deterministic coverage of duplicate admission, daemon and runner crashes, lease
expiry, interrupted effects, revoked recovery, missing Outcomes, incompatible components and
incomplete retirement. Upgrades renew it. An unrun or failed gate is a blocker. Repository gates
are listed in [development verification](development.md#verification).
