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

The daemon's production composition contains the complete durable launch path: an
admitted dispatch commits the runtime identity and launch intent, prepares protected
storage and the runner's input, spawns the matched `forge-runner-host` and returns; a
daemon-owned driver then delivers and renews the authorization lease and admits over the
supervisor's private pipe. Same-attempt recovery from exact crash proof, follow-up
delivery and retirement use that same composition. It is assembled only when matched
runner components are configured (see below), and it is still behind the gate above:
configuring components never admits a launch.

A Forgeup-managed daemon of a Revision with a schema 2 runner bundle composes this
path from the Installation's packaged components (see
[Install and run the matched runner](#install-and-run-the-matched-runner)). It still has
no model-credential setting (see the known gap below), so it cannot validate a durable
profile. The behavior described under
[Assignment recovery](assignment-recovery.md#same-assignment-durable-recovery-composed-with-matched-components-launches-withheld)
therefore remains a controlled-test capability, not an operational one.

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
| Daemon launch | Manual `forge-server`; Forgeup-managed daemon with the Revision's packaged components (schema 2 bundle) | Model credentials for a Forgeup-managed daemon (see below) |

**Known gap:** a Forgeup-managed daemon receives the Revision's matched runner components,
including the packaged host, but has no model-credential setting; it inherits none of the
`FORGE_DURABLE_*` environment values. Durable profile preview/registration there fails with
`durable-model-auth-unavailable`. Closing this gap is release work, not an operator workaround.

The key file must be an absolute canonical path to a regular, single-link, owner-owned
`0600` file in an owner-owned `0700` directory, containing one printable ASCII token
(1–4096 bytes) with an optional trailing newline. Diagnostics never include the key or its
path. Forge passes the key bytes only to the runner's in-memory provider auth: never the
shell environment, prompts or Forge records, and provider events are redacted. This is not
containment: under the trusted-local-host boundary, a `bash` or `read` tool running as the
same user can read the key file itself, and that tool output is stored unredacted in the
runtime store and transcript and sent to the provider. Keep the key file outside every
Managed Worktree and treat transcripts as sensitive. The key file *path* is in the
manual daemon's own environment (`FORGE_DURABLE_OPENAI_KEY_FILE`), which processes the
daemon starts with an inherited environment (hooks, `git`, `gh`) also see; the durable
host is started with a cleared environment. A model key grants no Forge authority,
and an Assignment Capability grants no model access.

See [Agent Profile policy](agent-profile-policy.md#durable-profiles-and-authentication)
for the full authoring and context rules.

## Install and run the matched runner

The runner, its `forge-runner-host` supervisor, the bundled Node and the locked JavaScript
dependencies are distributed as one matched component of a Forge Revision. Build and
register it with the Revision
([matched runner packaging](development.md#matched-runner-packaging)):

```sh
cargo build --release -p forge-runner-host
node runner/package-bundle.mjs --out /tmp/forge-runner-bundle --forge-commit <40-hex-commit> \
  --runner-host target/release/forge-runner-host
forgeup register --source-label local --cli target/release/forge \
  --daemon target/release/forge-server --runner-bundle /tmp/forge-runner-bundle
forgeup runner verify <selector>
forgeup daemon start
```

`register` and `daemon start` attest the following, and refuse a missing,
modified or mismatched component with an actionable message:

- the commit, runner protocol 1 and Node version;
- the lock hash and content tree hash;
- the packaged host's compatibility and Revision;
- the runner component hash.

`daemon start` passes the Installation's bundle as `--durable-runner-bundle`. It
also passes, as `--durable-retained-runner-bundle`, every other registered
Revision's bundle that still verifies. Global `PATH`, npm and the checkout are
never consulted.

Before any allocation, each durable dispatch checks every matched component again
(launch preflight). It refuses with `durable-components-incompatible` when any of
these holds:

- a component is missing or was replaced after daemon start;
- Node is not v24.18.0;
- the host reports another protocol, runtime schema or Revision;
- the runner sources no longer attest the composed hash.

**Retained Assignments.** An Assignment keeps the runner components that launched
it. After you switch Revisions, the daemon uses an earlier Revision's bundle for
that Assignment's transcripts, effect inspection, recovery and retirement, matched
by the Launch's component hash. If no configured bundle matches, those operations
refuse with `durable-components-unavailable`, which names the hash. They never
fall back to the current Revision's components.
`forgeup runner resolve --component-hash <hash>` finds the Revision that shipped
it. Register that Revision (and keep its bundle intact), then restart the daemon.
A Revision without a bundle, or with an inspection-only schema 1 bundle, cannot
launch. Removing a runner-bearing Revision requires `--purge-state` while its
state exists.

For a manual daemon, name the components explicitly (both or neither; absolute paths only)
and, for profile registration, the key file:

```text
FORGE_DURABLE_NODE=/abs/path/bundle/node/node
FORGE_DURABLE_EFFECT_INSPECTOR=/abs/path/bundle/runner/src/inspect-effects.ts
FORGE_DURABLE_OPENAI_KEY_FILE=/abs/owner-only-dir/openai.key
```

A manual daemon composes durable launch and recovery only when it is also given the matched
host and runner entry, both or neither, together with the Node and effect inspector above.
The recovery and answer inspectors are the entry's siblings in the same `runner/src`
directory, and the component hash recorded on each Launch is attested from that directory
at startup:

```text
FORGE_DURABLE_RUNNER_HOST=/abs/path/bundle/host/forge-runner-host
FORGE_DURABLE_RUNNER_ENTRY=/abs/path/bundle/runner/src/check-entry.ts
```

This selects components only; new launches stay withheld. A manual daemon has no
retained-bundle setting: it serves existing Launches only when their component hash
matches these components, and otherwise refuses with `durable-components-unavailable`.

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
freshness and evidence; it cannot rebind any of them. While the original runner runs under live
authority (the launching session, including after a missing Outcome, or a recovered session),
the daemon delivers it idempotently (`delivery`: `recorded`, `unknown`, then
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
and tool calls, and an independent watchdog writes a fence marker (`watchdog.json`), sends TERM
to the runner's process group, then KILL after at most 5 seconds. Expiry alone is not proof that
a child process or its external action stopped, and a restarted daemon never adopts, signals or
renews the old host.

What happens next depends on the original host's own proof:

- **Ordinary exit.** When the daemon dies, the runner normally sees its input close and exits.
  The original host, which is the dedicated subreaper of its runner, then records crash proof
  for the last lease.
- **Fenced, then proven.** If the runner does not exit (a hung tool or shutdown), the
  watchdog fences and kills it. Once the original host has positively reaped its runner and the
  kernel reports no remaining descendant, it writes a protected
  `watchdog-receipt-<lease sequence>.json`. That receipt is bound to the fence marker's exact
  bytes, the owner identity and the last lease. The host then exits, releasing ownership.
- **Fenced, unproven.** An escaped child (one that left the process group), a signal the host
  could not verify, or changed ownership evidence produce no receipt. The host keeps ownership
  and capacity stays reserved. It writes a
  `watchdog-<pid>-<start>-quiescence-unproven.json` diagnostic, and status reports
  `runtime_fence: runtime-fence-unproven` (also listed under uncertainty). If an escaped child
  later really exits, the kernel proof follows and the receipt is written then.

With crash proof or a fence receipt, the restarted daemon continues on its own. Its ordinary
reconciliation scan recovers the same attempt (same admission, conversation and attempt) when
Forge authority and effect inspection allow it. Alternatively, `assignment-retire` reconciles the
runtime from the same proof and releases capacity, even though this daemon never owned the host.
A committed interrupted effect still blocks recovery, and blocks retirement finalization until
it is resolved (see below). Until proof exists, recovery refuses to compete with the original
host, and retirement reports a specific code:

- `durable-runtime-owner-active`: the original host still holds ownership (its lease has not
  expired yet, or it is still killing its runner). Retry after a lease period.
- `durable-runtime-fence-unproven`: a fence exists without valid proof. Investigate the runtime
  directory's `watchdog-*` diagnostics. Look for an escaped descendant of the old host, using
  `ps` and the host PID in `owner.json`. Do not delete markers or receipts, and do not kill
  processes by PID alone unless you have identified them. When the descendant ends, the host
  writes its receipt; then repeat the same command.

### Failed launches

A durable launch that fails after its claim never strands the Assignment (T26). The launch
claim (`provision-intent`) and the reserved runtime identity commit in one transaction:
either both exist or neither does, so a refused precondition (a stale binding, a closed
Assignment) leaves nothing behind. A claimed Assignment is never respawned
(`durable-launch-claimed`). Each later failure keeps a specific diagnostic, shown in
`assignment-status` under `retirement_blockers` (and as `launch-failed` under uncertainty):

| Code | Failure point |
| --- | --- |
| `durable-launch-runtime-input-failed` | Protected runtime storage or `check-runtime.json` was not written. |
| `durable-launch-intent-refused` | The launch intent was refused (the Assignment closed or moved on). |
| `durable-launch-spawn-failed` | The matched host could not be spawned. |
| `durable-launch-owned-invalid` | The host sent no valid `owned` evidence for the matched components. |
| `durable-launch-bind-failed` | The spawned host could not be bound through the interrupted-effect gate. |
| `durable-launch-grant-failed` | The first grant could not be committed or delivered. |

The daemon never drops a live host. After a failure before any grant, it closes the host's
private pipe and reaps it. The host treats the closed pipe as a lost issuer: it fences
(`watchdog.json` with code `runner-issuer-lost` and no last lease), stops its runner, proves
as subreaper that no descendant remains, writes `ungranted-receipt-<pid>-<start>.json` and
exits. The same happens when the launching daemon dies before the first grant.

`assignment-retire` then reconciles with the strongest available proof and releases capacity:

- **No launch intent was committed:** Forge's own records prove that no host was ever spawned.
  Any prepared storage must also seal (`launch-sealed.json`) so no host can own it later; a
  host that holds it anyway blocks this proof.
- **The launch intent was committed but no host ever owned the directory** (for example, a
  failed spawn): the directory is sealed under its exclusive lock, with no owner evidence.
- **A host owned it but never received a grant:** its exact ungranted receipt.
- **A grant was committed, or the host is still held by this daemon:** the ordinary paths above
  (host retirement receipt, crash proof or fence receipt).

Exact retries return the original result. Until proof exists, retirement reports
`durable-runtime-owner-active`, `durable-runtime-fence-unproven` or
`durable-launch-ungranted-unproven` (a host owned the directory before any grant but did not
prove quiescence, for example because it was killed). As above, investigate rather than delete
evidence. A reconciled failed launch can then be purged with `assignment-purge-runtime`.

Plan for an Assignment to need recovery or retirement after any daemon outage longer than a
grant. Under the trusted-local-host boundary, both are available once the original host has
proven quiescence.

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
`forge dispatcher resume <task-id>`. The retirement, crash, watchdog-fence and recovery
evidence files the host itself wrote are deleted with the runtime; anything else in the
directory refuses deletion.
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
- **No proof of death from persistence or leases.** Only the original subreaper's own receipts
  count (retirement, crash or post-fence quiescence), and only for this host's descendants.
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
