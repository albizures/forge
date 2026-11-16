# Agent Profile policy boundary

Implementation contract for Spec #614, Tasks #615–621. Operator preview,
registration and explicit selection are available. The Herdr adapter uses a
flags-only Pi argument path and protected [Assignment bootstrap and context](assignment-context.md)
for registered Task and Grilling policies. Task attempts use their immutable
Check-pinned revisions; Grillings use the Workspace selection. Dispatch preflight
checks applicable direct inputs and the neighboring Forge CLI before preparing a
worktree or allocating provider
resources. Final input validation is repeated before launch.

During #617 the human explicitly chose “use only flags for now”, superseding the
custom-loader/runtime-attestation requirements for this implementation slice
(recorded on #614). Flags configure Pi; they do not attest a closed runtime.
See the [acceptance coverage handoff](agent-profile-acceptance.md) for test
mapping and the guarantees superseded by that decision.

## Authoring and resolution

An Agent Profile is independent of Skills: its Markdown body contains role-specific
instructions, while its resource selections compose reusable workflows. Forge retains
execution authority and lifecycle rules.

Profile sources must be `.md` files beneath the selected Forge Workspace's
`.agents/profiles/` directory, for example `.agents/profiles/implementation.md`.
Nested directories such as `.agents/profiles/reviews/security.md` are supported.
Preview and registration validate canonical containment: file or directory symlinks
cannot escape this directory. The profiles directory itself (or its `.agents`
parent) cannot redirect elsewhere, even within the Workspace. This is an authoring
boundary, not a security sandbox. No special basename or neighboring `SKILL.md` is
required. No discovery, inheritance, or automatic selection is added.
Only explicitly selected Skills are included; proximity never selects a Skill.

Example `.agents/profiles/implementation.md`:

```markdown
---
version: 1
runtime: herdr/pi
model: openai/gpt-5
thinkingLevel: low
tools: [read, bash, edit, write]
skills:
  - name: implementation
extensions: []
mcpServers: []
---
Read and follow the implementation skill before working.
```

Frontmatter is strict: unknown/duplicate fields, other versions/runtimes,
unqualified models, duplicate or non-built-in tools and malformed references are
errors. Required fields are version/runtime/model/thinkingLevel/tools; omitted
resource lists mean empty. An empty tools list means no built-ins. Lifecycle
controls belong to Forge, never the document. References contain exactly one `name` or `path`.

`thinkingLevel` is required for every newly previewed or registered document,
regardless of role (Implementation, review, PR creation or Grilling). It must be
exactly one of the strings `off`, `minimal`, `low`, `medium`, `high`, or `xhigh`.
Omission, null, wrong types, YAML tags and unsupported spellings are rejected;
there is no authoring default. The example explicitly requests `low`.

This strict v1 authoring requirement does not apply retroactively to stored
revisions. Historical persisted policies without `thinkingLevel` remain readable
and launchable without a Forge-added thinking override; legacy revisions without
a resolved policy also keep their previous launch behavior. Forge neither invents
a value nor backfills stored policy. Re-previewing or re-registering an old source
is new authoring and requires an explicit choice.

`ResourceCatalog::discover(operator_root, workspace_root)` merges installed
resources beneath two explicit absolute roots. The operator root is the home
catalog root, not `.pi/agent` itself. Supported locations at either root are
`.agents/skills`, `.pi/skills`, `.pi/agent/skills`, `.pi/extensions`,
`.pi/agent/extensions`, `.pi/mcp.json` and `.pi/agent/mcp.json`. Skill names come
from SKILL.md frontmatter; extension names come from top-level entry filenames
or directory names with an `index.ts`, `index.js` or `index.mjs` entry (in that
precedence order). Supporting modules inside extension directories are not
catalog entries. MCP names
come from the `mcpServers` object. Discovery reads data only: it never imports
extensions or starts MCP servers. Native Pi ambient settings are not silently
added to this catalog.

`resolve(workspace_root, document)` resolves document arguments from the selected
Forge Workspace root and internal path references from the document directory.
Neither process cwd nor a Task checkout is a discovery base. Names must identify
one canonical member; duplicate names are not resolved by root precedence. Paths
may select outside-root catalog members, including installed symlink targets,
but cannot grant membership. Skill paths may identify SKILL.md or its directory.
MCP paths identify catalog configuration files; a file containing multiple server
entries is ambiguous and must be selected by unique server name instead.

## Operator preview, registration and selection

```sh
forge agent-profile preview .agents/profiles/implementation.md \
  --workspace development --record-type task --json
forge agent-profile register .agents/profiles/implementation.md \
  --workspace development --record-type task --json
# Use the returned revision explicitly; do not assume a latest/default revision.
forge dispatcher config set task-profile 2 --workspace development
forge dispatcher config set grilling-profile 2 --workspace development
forge dispatcher profile --workspace development
forge dispatcher profile --kind grilling --workspace development
```

The Grilling revision must first be independently registered with
`--record-type grilling`; revision numbers are per Record Type, not interchangeable
handles. Each Workspace has separate selections. Task selection supplies
configuration/authoring defaults, not live routing for existing Tasks. Pin the
registered Task revisions in the Project's Check Template before opening Tasks;
all retries/remediation retain those copied Check pins despite later selection or
template changes. A missing/unusable pin blocks without fallback or rebinding;
restore its prerequisites or safely choose Manual. Preview defaults to Task for
Forge-owned lifecycle policy; registration requires an explicit Record Type.
Both require explicit `--workspace`, even from inside a Workspace. Relative
**document arguments are sent unchanged** to the daemon and resolved from that
Workspace root. The daemon's home directory supplies the operator catalog, not
the client's home or cwd. Files must be available to the daemon.

Readable and JSON responses report runtime, model, thinking level and tools,
canonical resource identities and SHA-256 digests, effective Forge-owned lifecycle
policy, exact fingerprint coverage and limitations. JSON policy responses expose
this choice as `thinking_level` (the document field is `thinkingLevel`); historical
policies without a choice omit it. They omit behavior and MCP configuration
(including credentials). Preview has a null `revision`; registration returns the
new revision and leaves all selections unchanged. Invalid documents/catalogs
fail with redacted reasons; neither operation imports extensions, starts MCP
commands, provisions worktrees or contacts a runtime provider. JSON uses
`schema_version: 1`. Operator RPC methods are `agent_profile.preview` and
`agent_profile.register`, with `workspace`, `document` and `record_type` fields.
These are trusted operator routes, not Assignment-authenticated agent actions.

Selection stores a registered revision even when the runtime is unavailable or
not yet compatible. It does **not** certify enforcement or enable unsupported
launches. Legacy revision selections retain the existing provider validation.
`forge dispatcher status --workspace development` reports launch readiness
separately. Invalid or unavailable selections are blocked; selected nonempty MCP
configuration is unsupported by the flags-only Pi launch. Restoring direct inputs
or selecting a supported revision allows readiness to be reevaluated without clearing
an unrelated Dispatch Hold.

### Isolated executable example

Build `forge` and `forge-server` from this checkout. In one POSIX shell (substitute
an unused port if needed):

```sh
cargo build -p forge-cli -p forge-server
FORGE="$PWD/target/debug/forge"
DAEMON="$PWD/target/debug/forge-server"
fixture=$(mktemp -d)
mkdir -p "$fixture/home" "$fixture/workspace/.agents/skills/example" \
  "$fixture/workspace/.agents/profiles"
printf '%s\n' '---' 'name: example' 'description: Example skill' '---' \
  'Follow the assigned work.' > "$fixture/workspace/.agents/skills/example/SKILL.md"
printf '%s\n' '---' 'version: 1' 'runtime: herdr/pi' 'model: openai/gpt-5' \
  'thinkingLevel: low' 'tools: [read]' 'skills: [{name: example}]' '---' \
  'Read and follow the example skill before working.' \
  > "$fixture/workspace/.agents/profiles/example.md"
HOME="$fixture/home" FORGE_STORE_PATH="$fixture/store.sqlite" \
  "$DAEMON" --bind-addr 127.0.0.1:45014 > "$fixture/daemon.log" 2>&1 &
daemon_pid=$!
export FORGE_SERVER_URL=http://127.0.0.1:45014/rpc
# Wait for readiness, then run from an unrelated cwd.
for attempt in $(seq 1 100); do
  "$FORGE" status >/dev/null 2>&1 && break
  sleep 0.1
done
(
  cd "$fixture"
  "$FORGE" workspace add demo --root "$fixture/workspace"
  "$FORGE" agent-profile preview .agents/profiles/example.md --workspace demo
  "$FORGE" agent-profile register .agents/profiles/example.md \
    --workspace demo --record-type task --json
  # This isolated fresh store returns Task revision 2. Registration did not select it.
  "$FORGE" dispatcher config set task-profile 2 --workspace demo
  "$FORGE" dispatcher profile --workspace demo --json
)
kill "$daemon_pid"
wait "$daemon_pid"
unset FORGE_SERVER_URL
rm -rf "$fixture"
```

The example intentionally does not dispatch. No external credentials, Herdr
resources, MCP servers or extension execution are required. Integration tests
exercise this command sequence with isolated operator/Workspace catalogs plus
extension/MCP sentinels, two Workspaces, both Record Types, new revisions and
profile source mutation/deletion.

### Breaking source-authoring change

Legacy profile sources under skill directories are rejected by preview and
registration. Move their role instructions into `.agents/profiles/*.md`; retain
only genuinely reusable workflows as Skills. Adjust relative resource references
for the new document directory. Selected resources still use the existing catalog
membership rules and may reside outside the profiles folder.

Existing registered revisions, Assignments and Task Check pins remain unchanged and
usable: no source migration or re-registration is required to retain their meaning.
To adopt changed profiles, preview and register new revisions, then explicitly select
them and update Project Check Templates for future Tasks. Existing Task pins are never
rewritten. Deploy and activate a supporting Forge Revision before migrating live source
files or dispatch selections; older Forge Revisions require the legacy authoring layout.

## Flags-only Pi launch configuration

The registered-policy argument path in `crates/server/src/herdr.rs` uses the
existing Herdr `agent.start` request, with no wrapper, custom loader, SDK runtime,
PATH changes or machine-wide configuration changes. Legacy profiles retain
their existing model configuration. Registered policies use authenticated bootstrap
and dispatch preflight; legacy revisions also use protected bootstrap.

For registered policies the adapter:

- Validates the stored policy's direct inputs before provider allocation and
  checks them again immediately before launch.
- Splits the stored provider-qualified model into `--provider` and `--model`.
- Passes `--thinking <level>` with exactly the registered revision's
  `thinkingLevel`, through the shared Task/Grilling argument builder. Historical
  stored policies without this field receive no `--thinking` argument, preserving
  their previous reliance on Pi settings/defaults. This requests Pi's thinking
  level; it does not guarantee model-internal reasoning or provider support.
- Passes `--no-approve`, overriding ambient saved/default project trust. It does
  not pass `--no-context-files`: repository and ancestor instructions remain
  available. Instructions are not a security boundary.
- Passes `--no-extensions` and `--no-skills`, then explicit repeated
  `--extension <canonical-entry-file>` and `--skill <canonical-SKILL.md>` arguments
  for the selections. Empty lists add no paths. Skills remain on demand; their
  bodies and registered behavioral Markdown are not inserted into these flags.
- Passes `--tools <comma-separated-built-ins>` or `--no-tools` for an empty list.
- Passes `--no-prompt-templates`, `--no-themes` and `--no-mcp`.
- Passes explicit empty-string values for `--system-prompt` and
  `--append-system-prompt`. On Pi 1.5.0 these suppress ambient `SYSTEM.md` and
  `APPEND_SYSTEM.md` discovery while retaining Pi's default prompt. Herdr must
  preserve these empty arguments, not drop them.
- Terminates options with `--` and starts Pi without a positional prompt. After
  Herdr confirms interactive readiness at the exact named resource/terminal identity,
  sends the short trusted bootstrap once through `agent.prompt`. Start and prompt
  delivery share the durable one-shot launch guard; uncertainty never authorizes a
  retry. Behavior and credential-bearing MCP configuration are never argument payloads.

**Selected MCP servers are currently unsupported.** Pi 1.5.0 has no documented
per-run flag for supplying the persisted selected MCP configuration. A nonempty
`mcpServers` list is rejected rather than connecting ambient servers, ignoring
selections or putting credentials on the command line. Preview/registration can
still store such a policy; registration is not a launch-compatibility check.

### Limits and verification

This is configuration, not runtime attestation. Forge does not probe or record
which Pi installation Herdr actually resolves, enforce an exact effective model
beyond Pi's own model lookup, freeze ambient user settings/model definitions, or
guard/disable native `/reload` and subsequent resource/setting changes. User
credentials and non-contract preferences remain available; `--no-approve`
suppresses most project settings, but Pi can read project `sessionDir` before
trust resolution. Selected extensions remain trusted unsandboxed code and may
add resources, tools, MCP replacements or change prompts. `--no-mcp` disables
Pi's built-in MCP support, not a selected extension's independent MCP client.
Tool selection does not constrain bash, filesystem access, same-user processes
or arbitrary extension code. Direct-input checks are not recursive dependency
pinning or an atomic validation-to-load filesystem snapshot.

Default-parallel Rust tests cover the Herdr argument payload, unchanged legacy
launches, bootstrap/behavior separation, and rejection of drifted, unsupported
MCP and non-UTF-8 resource inputs without substitution. An opt-in test executes
the real Pi CLI with isolated HOME/agent/cwd, cleared environment (PATH retained
for Node), offline catalogs and a loopback-only fixture model using a dummy key:

```sh
FORGE_TEST_PI=/absolute/path/to/pi cargo test -p forge-server pi_flags_runtime -- --ignored --nocapture
```

It observes selected extension execution and on-demand skill advertisement,
empty resource/tool lists, an actual allowed `read` in the execution directory,
repository/ancestor instructions, ambient prompt/template suppression and no
ambient extension/MCP process start (fixtures include all four MCP exposure
modes). It does not allocate Herdr tabs or test Herdr's shell argument escaping,
malicious extensions, reload protection or runtime attestation. The test is
explicitly ignored in the portable default suite because Pi is an external
runtime; run it separately when changing the flag contract. Verified against Pi
1.5.0, not a promise that arbitrary other installations support these flags.

## Frozen revision payload

`ResolvedLaunchPolicy` is the shared application boundary for preview,
registration, preflight and runtime adapters. It retains schema version, runtime,
provider-qualified model, thinking level (when present in the stored revision),
exact built-in allowlist, behavior, canonical selected file identities and SHA-256
digests. MCP entries retain canonical catalog source
identity and the selected persisted launch configuration with its SHA-256 digest.
MCP configuration can contain credentials: adapters must redact it from previews,
audits and diagnostics rather than serializing the full persistence payload.

`WorkspaceDispatchRepository::register_agent_profile` inserts a new per-Record-Type
revision in the existing `agent_profiles` table. It copies Forge-owned lifecycle
settings from that Workspace's selected same-type revision, or seed revision 1
when none is selected. Registration never selects. Workspace Task and Grilling
selections remain independent. Both acknowledgement-timeout revision operations
copy `launch_policy_json` unchanged; old revisions are not mutated. Legacy rows
retain `launch_policy: None` without fabricating historical policy.

The profile source is not a runtime dependency. Editing or deleting it cannot alter
stored behavior/configuration, including thinking level. Later registrations do
not mutate prior revisions, Assignment bindings or Task Check pins. Source edits
alone never reconfigure an existing Assignment or Check. Adoption requires new
registration and explicit selection; Task routing additionally requires Project
Check Template pins for future Tasks, not a rewrite of existing pins.
`validate_direct_inputs` validates policy and compares only selected
SKILL.md/extension entry bytes and persisted MCP configuration. Supporting files/imports/dependencies are not recursively hashed.
After source changes an explicit new resolution/registration is required; changes
to the original MCP catalog do not replace a registered configuration.

This freezes policy, not resource execution. There is no OS/same-user sandbox,
recursive dependency pinning, external-service/model/credential freeze, or atomic
validation-to-load filesystem snapshot. Selected extensions remain trusted code.

## Bound versus live Assignment data

Dependent context/runtime Tasks must use these existing durable ownership facts:

- Assignment `id`, `record_id`, attempt number and bound `profile.record_type` /
  `profile.revision`; profile loading follows that immutable key, never a current
  Workspace selection or greatest revision.
- Persisted Assignment Workspace/provider identity and exact provider resource
  tuples. Execution cwd is the Task's bound Managed Worktree, or the Grilling's
  Forge Workspace root; a first Project root is not an execution-root proxy.
- Lifecycle policy remains part of the bound profile; `launch_policy` adds the
  frozen model/resources/behavior, not authority to alter retries or holds.
- Context envelopes keep bound identity separate from live Record,
  relationships, comments/history and complete Project Scope, with freshness
  metadata. Never expose the persisted MCP configuration as public context.
- Missing profile sources/direct files must not prevent existing authenticated actions,
  reconciliation or retirement. Direct-input validation belongs only to new
  execution and supported reload. It must not erase ownership evidence, prove
  process exit, release capacity, or replay one-shot launch.

Assignment context uses schemaVersion 1. Runtime
identity/closed-loader enforcement is not implemented under the approved
flags-only scope; no persisted runtime-attestation fields are claimed.
