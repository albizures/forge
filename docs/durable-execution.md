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
Revision `6a2196a` has passed it ([verification record](release-verification/6a2196a.md)),
with the [accepted limitations](release-verification/6a2196a.md#accepted-limitations) recorded there;
eligibility itself is not implemented yet (see
[Enabling durable launch](#enabling-durable-launch)).

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
[Install and run the matched runner](#install-and-run-the-matched-runner)). Its model
credential is selected explicitly at start with
`forgeup daemon start --durable-openai-key-file PATH` (see
[Supported configuration](#supported-configuration)). New launches stay withheld either way.

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
| Model authentication | Raw OpenAI API key **file**: `FORGE_DURABLE_OPENAI_KEY_FILE` on a manual daemon, or `forgeup daemon start --durable-openai-key-file PATH` for a managed daemon | OAuth, Pi `auth.json`/settings, key values in environment/arguments/YAML, copied Pi configuration |
| Daemon launch | Manual `forge-server`; Forgeup-managed daemon with the Revision's packaged components (schema 2 bundle) | A managed key file without a schema 2 runner bundle |

**Managed daemon credential.** A Forgeup-managed daemon inherits none of the
`FORGE_DURABLE_*` environment values. Its model credential is only the key-file **path** the
operator passes explicitly: `forgeup daemon start --durable-openai-key-file /abs/path`.
Forgeup requires an absolute path and a schema 2 runner bundle, never reads the file, and
passes the path as `--durable-openai-key-file`; the daemon validates the file exactly as for a
manual daemon. A daemon started implicitly (for example by a CLI command that ensures the
active daemon is running) gets no key, so durable preview/registration there fails with
`durable-model-auth-unavailable` until the operator restarts it with the option. A running
daemon keeps the credential it started with: `daemon start --durable-openai-key-file` on a
running daemon refuses (`--durable-openai-key-file: the managed daemon is already running …`)
instead of reporting `running`, so run `forgeup daemon stop` first.

The key file must be an absolute canonical path to a regular, single-link, owner-owned
`0600` file in an owner-owned `0700` directory, containing one printable ASCII token
(1–4096 bytes) with an optional trailing newline. Diagnostics never include the key or its
path; a managed daemon's key-file path does appear in its process arguments, visible to
local users like its other arguments under the trusted-local-host boundary. Forge passes the key bytes only to the runner's in-memory provider auth: never the
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
[Check Template](checks.md#project-check-templates) before opening them. Mixed backends across
a Task's Checks are allowed.

## Migrate existing Checks

Existing Checks keep their original pins. Changing a template or a Workspace selection,
registering a new profile or changing the launch gate never moves them. The only way is an
explicit, audited migration that selects an *effective* revision per named Check:

```sh
forge record check-profile-migrate <task-id> --operation-id adopt-1 --expected-generation <current-generation> \
  --check 7=5 --actor operator --message 'adopt pi-durable' --json
forge record check-profile-migrations <task-id>
```

Nothing is registered, dispatched or launched. The original pin, the Check's identity,
description and order, every Assignment's bound profile and backend, and all Assessments stay
as append-only history (store schema 23). Mixed backends across a Task's Checks are
supported, and a later migration may select another durable revision.

- **Eligibility:** each selection is a registered Task profile revision whose provider and
  launch runtime are `pi-durable`, and each Check belongs to the Task. The Task is not Done,
  has no open attempt or Assignment (`attempt-active`), and all known and unknown resources are
  reconciled (`execution-mode-unsafe-ownership`). Live Herdr work finishes or retires through
  Herdr first. An unchanged revision (`check-profile-unchanged`), a Herdr or missing revision,
  or a Check from another Task rejects the whole request.
- **Freshness:** pass `--expected-generation` from the Task's work state; a stale value fails
  with `stale-work-generation`. A Task with work or Assessments has its freshness invalidated
  exactly once, so earlier passes and attempts are stale, and the receipt reports the
  generation before and after. A pristine Task keeps its generation.
- **Retries:** reuse the operation ID after a dropped reply. The original receipt returns with
  `replayed: true` and never advances freshness again. The same ID with different content
  fails with `check-profile-migration-conflict`.
- **Next attempt:** the following dispatch uses the effective revision through the ordinary
  gates (capacity, Dispatch Hold, preflight). A stale selection of the old pin is rejected as
  `check-selection-stale`.
- **Release gate:** the target revision is preflighted through the launch gate. While new
  launches are withheld or rolled back, migration refuses with `invalid-check-profile` and
  records nothing. [Rollback](#rollout-and-rollback) never rewrites pins.

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
unauthenticated; restrict daemon access ([Security](../README.md#security)). Confirmation flags
such as `--confirm-old-agent-cannot-act` are trusted operator assertions, not authenticated
identity or proof of process death. General investigation, unknown-resource resolution,
retained-work salvage and hold clearance follow the
[Assignment recovery runbook](assignment-recovery.md); this section covers what is specific to
durable Assignments.

### Status and transcripts (read-only)

```sh
forge dispatcher assignment-status <assignment-id> --workspace personal
forge dispatcher assignment-transcript <assignment-id> --workspace personal --limit 50
forge dispatcher assignment-transcript <assignment-id> --workspace personal --after 42 --limit 100 --json
```

Both commands call trusted daemon RPC (`assignmentDurableStatus`, `assignmentTranscript`).
Neither starts a runner, scheduler, model call, tool call or lease, takes host ownership or
opens storage writable. Both work for running, crashed, retired and missing-Outcome
Assignments.

- **Status** reports persisted Forge facts: backend, runtime and attempt identity, admission
  result, retained lease history (`never-granted`, `granted-unverified`, `revoked`), missing
  Outcome, retained uncertainty, capacity, retirement blockers and recorded follow-ups. Neither
  lease state nor a missing runtime proves a process is running or absent, and
  `granted-unverified` never means authority is live. Status needs only the store.
- **Transcript** pages committed root-conversation entries in ascending immutable entry ID
  order. Omit `--after` for the first page, then pass the previous `next_after`; `--limit`
  accepts 1 to 100. Live appends only add larger IDs, so a page is never reordered or
  duplicated, and `more=false` is not end of history for a live Assignment. `history` is
  `available`, `not-started` or `unavailable` with a fixed non-secret reason code. Corrupt,
  foreign or unreadable storage is `unavailable`, never an empty transcript, and is not
  repaired.
- The transcript reader is the `inspect-transcript.ts` beside the matched effect inspector,
  verified against the component hash retained in the Launch
  ([matched runner](#install-and-run-the-matched-runner)). Without those components the
  command fails with `durable-transcript-unconfigured`.
- Entries are bounded to 8 KiB (`truncated` is flagged). Tool output can contain repository
  content, so treat transcripts as sensitive. Forge adds no capabilities, credentials or store
  paths.

### Follow-ups

```sh
forge dispatcher assignment-followup <assignment-id> --workspace personal --operation-id fix-1 \
  --message 'submit the result with the Outcome command' --json
```

A follow-up is durable operator input (store schema 23, immutable and retained), bound to the
Assignment's original attempt, Check, profile revision, generation and runtime. The request
carries only the Assignment, an operation ID and text, so it cannot rebind work: a request such
as "use another commit" is stored as text and the evidence is unchanged. Scope or evidence
changes use Forge's existing invalidation and replacement rules.

- **Delivery:** a record is not an Outcome, a lease or proof that the agent acted. While the
  original runner runs under live authority, the daemon sends undelivered follow-ups: the
  launch driver after an accepted admission (it keeps the lease after a missing Outcome so this
  works), and the recovery driver after the runner reports `resumed`. It records `unknown`
  before each write and `delivered` only after the runner accepts it. The runtime request ID
  `followup:<operation-id>` deduplicates a re-send, so an `unknown` follow-up re-sent to a later
  session replays its original admission. The receipt's `delivery` field reports `recorded`,
  `unknown` or `delivered`; delivered input appears in the transcript.
- **Retries:** reuse the same operation ID after a dropped reply. The original receipt returns
  with `replayed: true`, even after the Assignment closed. Different text under that ID fails
  with `followup-operation-conflict`.
- **Rejections:** `followup-assignment-revoked` (closed), `durable-binding-stale` (attempt
  closed or generation invalidated), `followup-admission-unconfirmed`, and
  `followup-blocked-by-uncertainty` for retained interrupted-effect, authorization,
  recovery-claim, retirement or provisioning uncertainty. A message never resumes unsafe work.
- **Missing Outcome:** a follow-up is allowed as explicit intervention. It does not create an
  Outcome, retire the diagnostic, release capacity or grant a lease.
- **Limitation:** recovery resumes only an unsettled original submission. If a follow-up was
  committed after the original answer settled and that session then crashes, recovery refuses
  (`runner-recovery-intervention-required`) and the follow-up stays `unknown`. Retire the
  Assignment; nothing is replayed.

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

A later fence in a recovered directory writes `watchdog-<pid>-<start>-fence.json` instead of
`watchdog.json`. Forge accepts a receipt only under exclusive inspection, with every fence
marker in the directory settled by its own receipt, and only for the owner of Forge's last
lease. Because Forge commits a lease before delivering it, that owner's proof for an earlier
lease of the same Launch also matches. A receipt from another owner, for another Launch or for a
later lease than Forge's, a tampered receipt, and a changed or unsettled marker are refused.
Status reads `runtime_fence` (`runtime-fence-unproven` or `runtime-fence-proven`) from the host
files without taking the lock.

With crash proof or a fence receipt that matches Forge's last committed lease, the restarted
daemon continues on its own. Its ordinary reconciliation scan recovers the same attempt (same
admission, conversation and attempt) when Forge authority and effect inspection allow it. (If
the daemon committed a renewal and died before delivering it, the host's proof names the
previous lease and does not match; see
[accepted limitations](release-verification/6a2196a.md#runtime-behavior).) Alternatively, `assignment-retire` reconciles the
runtime from the same proof and releases capacity, even though this daemon never owned the host.
A committed interrupted effect still blocks recovery, and blocks retirement finalization until
it is resolved (see below). Until proof exists, recovery refuses to compete with the original
host, and retirement reports a specific code:

- `durable-runtime-owner-active`: the original host still holds ownership (its lease has not
  expired yet, or it is still killing its runner). Retry after a lease period.
- `durable-runtime-proof-unmatched`: no host holds the runtime, but no host proof matches
  its recorded owner and Launch. Investigate the receipts; do not delete them.
- `durable-runtime-fence-unproven`: a fence exists without valid proof. Investigate the runtime
  directory's `watchdog-*` diagnostics. Look for an escaped descendant of the old host, using
  `ps` and the host PID in `owner.json`. Do not delete markers or receipts, and do not kill
  processes by PID alone unless you have identified them. When the descendant ends, the host
  writes its receipt; then repeat the same command.

#### Timing and bounds

| Bound | Value | Defined in |
| --- | --- | --- |
| Lease grant / renewal / TERM-to-KILL grace | 30 s / every 10 s / 5 s | `crates/app/src/authorization_lease.rs` |
| Host `owned` evidence / admission / stop wait | 30 s / 120 s / 10 s | `crates/server/src/durable_authorization/launch.rs` |
| Live-host observation during retirement | 9 s | `crates/server/src/durable_authorization/retirement.rs` |
| Effect, recovery and transcript inspectors | 30 s deadline, then one more second to kill and reap; 20 MiB effect-inspector response | `durable_authorization/effects.rs`, `recovery_inspection.rs`, `transcript.rs` |
| Control frame on the supervisor pipe | 1 MiB | `crates/runner-host/src/control.rs` |
| Follow-up message | 16 KiB | `crates/app/src/durable_execution/followup.rs` |
| Approved profile context | 256 KiB | `crates/app/src/durable_profiles.rs` |

The lease values were selected during the initial contract research; no further rationale or
measurement was recorded. Renewing at a third of the grant means two consecutive renewals can
be lost before expiry. Deadlines use
`CLOCK_BOOTTIME` (via `/proc/uptime`, rounded **up** to the next centisecond so admission is
denied slightly early). No measurement-based tuning has been done. Changing any of these is an
implementation change that renews release verification.

### Same-attempt recovery

Assignment Execution Recovery resumes the **original** admission, conversation and Check
attempt after a proven crash or fence. It is automatic and has no operator command: whenever
matched runner components are configured (a Forgeup schema 2 runner bundle, or a manual
daemon's `FORGE_DURABLE_RUNNER_HOST`/`FORGE_DURABLE_RUNNER_ENTRY`), the daemon's ordinary
reconciliation scan attempts it for `unknown` durable Assignments. The launch gate does not
restrict it. It never creates another conversation root, input, `continue`, Check attempt or
Continuation.

Recovery starts only from the original host's own protected proof for Forge's last lease: a
crash receipt (the host reaped its worker and the kernel reported no remaining children), a
fence receipt ([above](#authorization-leases-and-outages)) or, before any grant, an ungranted
receipt ([failed launches](#failed-launches)). A still-verified original supervisor can also be
asked to stop its worker without revoking the Assignment (predecessor quiescence), which ends in
the same kind of receipt. A dead PID, a free lock, controller loss, surviving descendants,
identity drift or a supervisor lost before proof never substitute for it. Escaped children,
conflicting evidence or unavailable proof leave ownership, claim and capacity retained, and
nothing is signalled again.

With proof, the daemon claims the original candidate exclusively and spawns a new directly
owned host. Under that host's ownership it rechecks the receipt, the original pending input,
committed effects and the claimed worktree, commits a fresh recovery lease, and only then
delivers the recovery envelope. The runner bootstraps again through the original capability
and resumes that submission. A failed commit or delivery permanently fences that episode; only
another proven crash starts a new one. Revocation, expiry, changed freshness, unrelated
blockers, ownership faults and uncertain effects deny authority. The design is recorded in
[ADR-0012](../crates/docs/adr/0012-authorized-durable-recovery-and-conservative-retirement.md).

**Worktree checks.** Recovery verifies the original claimed worktree, repository and branch,
and both Base Commit and attempt-input ancestry, and fingerprints the retained bytes and index.
Dirty Implementation progress remains observed work, not a checkpoint or completion evidence;
other Checks, including Reviews, require their exact clean input. Hidden index flags are not
proof of clean input. Fsmonitor is disabled for these reads, and configured clean/process
filters, submodules and unsafe filesystem observations block recovery. A fingerprint retained
before fsmonitor was disabled can mismatch (historical fingerprints were not backfilled); that
blocks salvage revalidation and needs investigation, not acceptance or a reset. Any failure
fences the verification even if files are restored.

**After resumption.** Model retries stay disabled, so an in-flight model request may settle
unanswered; neither it nor any command is silently replayed. A Missing Outcome retains a hold
and capacity and creates no successor. A deliberate accepted Continuation and an independent
Review instead get a fresh conversation and store, with Forge-approved context, after
retirement.

### Failed launches

A durable launch that fails after its claim never strands the Assignment. The launch
claim (`provision-intent`) and the reserved runtime identity commit in one transaction:
either both exist or neither does, so a refused precondition (a stale binding, a closed
Assignment) leaves nothing behind. A claimed Assignment is never respawned
(`durable-launch-claimed`). Each later failure keeps a specific `durable-launch-failure` resource, shown in
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

`assignment-retire` then reconciles with the strongest available proof, journals it
immutably (store schema 25) and releases capacity:

- **`never-launched`:** no launch intent was committed, so Forge's own records prove that no
  host was ever spawned. Any prepared storage must also seal (`launch-sealed.json`) so no host
  can own it later; a host that holds it anyway blocks this proof.
- **`never-owned`:** the launch intent was committed but no host ever owned the directory (for
  example, a failed spawn). The directory is sealed under its exclusive lock, with no owner
  evidence.
- **`ungranted-quiescence`:** a host owned it but never received a grant; its exact ungranted
  receipt, with every marker settled. This also applies when a retirement of that host was
  lost with its daemon.
- **A grant was committed, or the host is still held by this daemon:** the ordinary paths
  ([fence proof](#authorization-leases-and-outages), [retirement](#retirement)).

Retirement accepts a `Pending` admission (never sent); an `Unknown` admission still needs the
launch reconciled. A launch claim without a runtime, left by an older Revision, is discharged by
retirement once the Assignment is closed. Exact retries return the original result. Until
proof exists, retirement reports `durable-runtime-owner-active`,
`durable-runtime-fence-unproven` or `durable-launch-ungranted-unproven` (a host owned the
directory before any grant but did not prove quiescence, for example because it was killed). As
above, investigate rather than delete evidence. A reconciled failed launch can then be purged
with `assignment-purge-runtime`.

Plan for an Assignment to need recovery or retirement after any daemon outage longer than a
grant. Under the trusted-local-host boundary, both are available once the original host has
proven quiescence.

### Interrupted effects

Shell and file tools are not replay-safe. If a tool call's intent was committed without a
known accepted result, Forge retains an `interrupted-effect` blocker. Its resource ID holds only
the execution-task, conversation and call identities; commands, arguments and results stay in
protected runtime storage. A tool call that failed before it started any filesystem write or
command (for example a `read` of a missing file, or an `edit` whose text was not found) had no
effect: the agent sees an ordinary error result and continues, and no blocker is created.

The blocker reserves capacity and blocks new grants, even for a closed Assignment. Before its
first grant, the daemon inspects retained runtime storage through the matched inspector and
retains any observed effect; missing or corrupt initialization, unavailable inspection and
unsettled intent all deny permission. A process exit, expired lease, later empty scan, cleared
Dispatch Hold or provider cleanup does not resolve the blocker, and its late arrival never
overwrites an accepted Outcome.

Investigate the real external outcome (push, publication, file changes) without replaying the
command, make the old execution and its children safe, then record a resolution with
evidence. Take `--execution-task`, `--conversation` and `--call` from the selected retained
`interrupted-effect` resource, not from another Assignment or the Forge Task ID:

```sh
forge dispatcher assignment-resolve-effect <assignment-id> --workspace personal \
  --operation-id resolve-1 --execution-task <runtime-task-id> --conversation <conversation-id> \
  --call <call-id> --outcome applied --actor operator --evidence <receipt-reference> \
  --message 'push observed on remote' --confirm-old-agent-cannot-act --confirm-external-work-safe --json
```

- **Requirements:** matched inspection components
  ([manual daemon](development.md#run-a-manual-daemon)), existing protected host-lock evidence,
  a fresh committed runtime scan and both confirmations. A missing lock, a live owner, an
  unavailable or incompatible inspector, or an unknown external outcome blocks acceptance. Use
  `not-applied` only with evidence of that conclusion; `unknown` cannot be resolved.
- **Effect:** for a **live** Assignment, the resolution keeps the original attempt and leaves
  the effect resource as an execution fence, even after hold clearance or later closure. This
  supports investigation without forcing revocation, but authorizes no renewed execution. For
  a **closed** Assignment, only that effect resource retires. Runtime intent stays unsettled,
  other resources, capacity blockers and holds remain, and no command is rerun, runtime receipt
  repaired, dirty commit adopted or recovery authorized.
- **Retries:** after a transport failure, inspect `assignment-info` and retry with the same
  operation ID and all original evidence, never the tool command. The original timestamp and
  receipt return, even after a daemon restart or loss of runtime components. Different
  evidence, or another operation for the same effect, rejects.
- **Audit:** the `interrupted-effect-resolution` audit stays visible through
  `assignment-info`. Its evidence fields are ordinary output, not protected storage: never put
  credentials, tool payloads or prompts in them.
- **RPC:** `agentDispatch_assignmentResolveEffect` with
  `{selection: {workspace, cwd?}, record: {assignment_id, resolution}}`. The resolution holds
  the operation ID, the exact `effect` identities, the conclusion, actor, evidence, message and
  both confirmation booleans. It accepts no capability, secret, runtime path or executable.

Do not use `assignment-resolve-unknown`, retained-work salvage, generic cleanup, runtime SQLite
edits or `resume` as a waiver. Never close an Assignment in SQLite to satisfy this command.

### Retirement

```sh
forge dispatcher assignment-retire <assignment-id> --workspace personal --json
forge dispatcher assignment-info <assignment-id> --workspace personal --json
```

The same `assignment-retire` used for Herdr
([runbook](assignment-recovery.md#revoke-and-clean-up-known-resources)) routes a durable
Assignment through its retained backend. It needs no profile source, model key or Herdr, and
does not depend on the launch gate: it works for live and closed Assignments after a rollback.

Retirement revokes authority **before** cleanup and journals the operation, Launch and owner
before contacting the host. The owner must match the original authorization grant, so a freshly
spawned lookalike cannot stand in. If this daemon still holds the original supervisor, it asks
it to stop: the host refuses new grants and sends TERM to the runner's process group (the
runner closes admission and aborts guarded execution), then after five seconds KILL, only while
the original leader identity is still verified. The dedicated subreaper writes an owner-only
receipt only when the kernel reports no remaining children; signal delivery, an abort
acknowledgement or leader exit alone is not a receipt. Otherwise retirement reconciles from the
original host's crash, fence or ungranted receipt. A missing controller never adopts a PID or
infers that work stopped.

Forge reads the exact receipt under exclusive host inspection and uses the matched read-only
effect inspector to retain any unsettled intent before releasing anything; missing inspection
components keep the operation pending even with a valid receipt. It then retires only the bound
runtime, its liveness diagnostics and any validated recovery reservation for that Launch. The
operator retirement marker finalizes only when every other resource is reconciled: unknown
provisioning resources and interrupted effects keep their own resolution paths. Missing
Outcomes and process cleanup never synthesize a passing Assessment. The Dispatch Hold stays
until a separate `resume`, and the worktree, index, runtime history and audit are kept.

A surviving or escaped child, missing proof, identity drift, dropped delivery or unavailable
components leave retirement unreconciled (`assignment-retirement-unreconciled` or a
`durable-retirement-*` diagnostic), with capacity and hold retained; Forge does not chase
orphan PIDs. Correct the cause and repeat the same command: it can reconcile a retained
receipt after a daemon restart without starting a runner or needing model credentials, and
accepted retries keep the original operation and timestamps. Restarted hosts deny grants once
retirement intent exists. The proof covers this host's descendants on supported Linux, not
remote effects or work delegated to an unrelated service. Never delete a lock, intent or
receipt file, or edit SQLite, to make cleanup look verified.

### Runtime history retention and deletion

Retiring or closing an Assignment never deletes its runtime history. The runtime directory
(`<workspace>/.forge/assignment-runtimes/<digest>`, owner-only `0700`, files `0600`) holds the
conversation, tool output and runtime evidence, which can contain sensitive repository content
even with credentials redacted. It stays readable through `assignment-status` and
`assignment-transcript` until an operator deletes it. Deletion is a separate, irreversible
step; Forge keeps no copy:

```sh
forge dispatcher assignment-purge-runtime <assignment-id> --workspace personal --operation-id purge-1 --json
```

- **Eligibility:** the Assignment is closed; its retirement is reconciled (or its admission was
  rejected); no resource is unretired, including unknown identities, interrupted effects,
  recovery claims and missing-Outcome markers; and no Dispatch Hold exists. Retirement places a
  hold, so clear it first with `forge dispatcher resume <task-id>`. Otherwise the purge is
  rejected (`runtime-purge-assignment-open`, `runtime-purge-uncertainty-retained`,
  `runtime-purge-retirement-unreconciled`, `runtime-purge-admission-unknown`) and nothing is
  journaled or deleted.
- **Deletion:** the directory comes only from the canonical mapping of the immutable store
  identity, never a caller path. The host takes the same exclusive ownership lock the
  supervisor and worker hold, so a live or still-held owner refuses
  (`runtime-purge-owner-active`). Every entry must be a recognized, owner-only, single-link
  regular file: the runtime database and its `-wal`/`-shm`/`-journal`, `check-runtime.json`,
  and the files runner-host's own lifecycle writes (`owner.json`, an interrupted
  `owner.<pid>.pending`, `watchdog.json`, `watchdog-<pid>-<start>-fence.json`,
  `watchdog-<pid>-<start>-<term|kill>-<sent|unverified>.json`,
  `watchdog-<pid>-<start>-quiescence-unproven.json`, `watchdog-receipt-<sequence>.json`,
  `ungranted-receipt-<pid>-<start>.json`, `launch-sealed.json`, `retirement-intent.json`,
  `retirement-receipt.json`, `recovery-crash-<sequence>.json`,
  `recovery-quiescence-intent.json`, `recovery-quiescence-receipt.json` and
  `recovery-proof-<64 lowercase hex>.json`). Numeric parts must be canonical decimals in range.
  Redirected, foreign, linked or unrecognized content (`runtime-purge-storage-unsafe`,
  `runtime-purge-boundary-invalid`) refuses before any file is removed. Data goes first, then
  configuration, diagnostics, recovery evidence, fencing markers, the retirement receipt and
  owner evidence, with `owner.lock` last, then the directory. No sibling runtime or worktree is
  touched.
- **Journal and retries:** intent is recorded first (store schema 23; append-only, final once
  `purged`). A completed purge also records (schema 24) whether this operation `removed` the
  directory or found it `already-absent`; an absent path is not proof that Forge deleted it, and
  purges completed before schema 24 show the outcome as unrecorded. A failure is retained as a
  diagnostic (`state: retained`) shown in status; retry with the same operation ID, which
  revalidates eligibility and resumes a partial deletion. A different operation ID is
  `runtime-purge-conflict`. A completed purge replays unchanged and never recreates storage.
- **Kept:** the Assignment, Outcomes, Assessments, migration, recovery and retirement audit,
  accepted operation facts, follow-up records and Managed Worktree contents.
- **Afterwards:** `assignment-transcript` returns `unavailable` with reason
  `runtime-history-purged` without starting a reader, and `assignment-status` shows the
  deletion. Nothing synthesizes an empty store or makes recovery, resume or replay executable
  again.

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

## Enabling durable launch

Readiness for a Revision is not enablement. Today only the test-only
`LaunchControl::enable_controlled` admits launches; production composition always uses the
withheld gate. Launch eligibility must be implemented and reviewed as its own change, and only
for a Revision with a retained verification record. After that, adopt in this order:

1. Install the Revision's matched components and keep every older Revision's bundle
   registered while it still has retained durable Assignments
   ([install](#install-and-run-the-matched-runner)).
2. Supply the model key file to the daemon (manual environment or
   `forgeup daemon start --durable-openai-key-file`; stop a running managed daemon first).
3. Preview and register the durable profile, then select a Herdr Task revision again
   ([prepare a profile](#prepare-a-durable-profile)).
4. Pin it per Check: new Check Template pins, or `check-profile-migrate` for eligible
   existing Checks ([migrate](#migrate-existing-checks)).
5. Roll out on new Assignments only, watching `durable-launch-status`, `assignment-status`,
   `assignment-transcript` and `assignment-info`. Never redirect live or uncertain work.

Rollback is [`durable-launch-disable`](#rollout-and-rollback).

## Release verification and accepted limitations

Enablement needs a retained verification record for the exact Revision. The requirements, what
ends readiness, and the upstream pi-durable constraints to re-check on any dependency upgrade
are in [Durable release verification](release-verification/README.md). The limitations accepted
for the current release are in
[Revision `6a2196a`'s record](release-verification/6a2196a.md#accepted-limitations).
