# Agent Profile policy boundary

Implementation contract for Spec #614, Tasks #615–616. Operator preview,
registration and explicit selection are available. Authenticated Assignment
context, preflight ordering and closed runtime loading are subsequent Tasks. **Registered policies cannot yet launch through Herdr.** This is not a
claim that outgoing Pi flags enforce the policy.

## Authoring and resolution

A `forge-agent.md` must sit beside `SKILL.md`:

```markdown
---
version: 1
runtime: herdr/pi
model: openai/gpt-5
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
errors. Required fields are version/runtime/model/tools; omitted resource lists
mean empty. An empty tools list means no built-ins. Lifecycle controls belong to
Forge, never the document. References contain exactly one `name` or `path`.

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
forge agent-profile preview .agents/skills/implementation/forge-agent.md \
  --workspace development --record-type task --json
forge agent-profile register .agents/skills/implementation/forge-agent.md \
  --workspace development --record-type task --json
# Use the returned revision explicitly; do not assume a latest/default revision.
forge dispatcher config set task-profile 2 --workspace development
forge dispatcher config set grilling-profile 2 --workspace development
forge dispatcher profile --workspace development
forge dispatcher profile --kind grilling --workspace development
```

The Grilling revision must first be independently registered with
`--record-type grilling`; revision numbers are per Record Type, not interchangeable
handles. Each Workspace has separate selections. Preview defaults to Task for
Forge-owned lifecycle policy; registration requires an explicit Record Type.
Both require explicit `--workspace`, even from inside a Workspace. Relative
**document arguments are sent unchanged** to the daemon and resolved from that
Workspace root. The daemon's home directory supplies the operator catalog, not
the client's home or cwd. Files must be available to the daemon.

Readable and JSON responses report runtime/model/tools, canonical resource
identities and SHA-256 digests, effective Forge-owned lifecycle policy, exact
fingerprint coverage and limitations. They omit behavior and MCP configuration
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
separately; registered policies currently remain blocked by the Herdr adapter.

### Isolated executable example

Build `forge` and `forge-server` from this checkout. In one POSIX shell (substitute
an unused port if needed):

```sh
cargo build -p forge-cli -p forge-server
FORGE="$PWD/target/debug/forge"
DAEMON="$PWD/target/debug/forge-server"
fixture=$(mktemp -d)
mkdir -p "$fixture/home" "$fixture/workspace/.agents/skills/example"
printf '%s\n' '---' 'name: example' 'description: Example skill' '---' \
  'Follow the assigned work.' > "$fixture/workspace/.agents/skills/example/SKILL.md"
printf '%s\n' '---' 'version: 1' 'runtime: herdr/pi' 'model: openai/gpt-5' \
  'tools: [read]' 'skills: [{name: example}]' '---' \
  'Read and follow the example skill before working.' \
  > "$fixture/workspace/.agents/skills/example/forge-agent.md"
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
  "$FORGE" agent-profile preview .agents/skills/example/forge-agent.md --workspace demo
  "$FORGE" agent-profile register .agents/skills/example/forge-agent.md \
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
companion mutation/deletion.

## Frozen revision payload

`ResolvedLaunchPolicy` is the shared application boundary for preview,
registration, preflight and runtime adapters. It retains schema version, runtime,
provider-qualified model, exact built-in allowlist, behavior, canonical selected
file identities and SHA-256 digests. MCP entries retain canonical catalog source
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

The companion is not a runtime dependency. Editing or deleting it cannot alter
stored behavior/configuration. `validate_direct_inputs` validates policy and
compares only selected SKILL.md/extension entry bytes and persisted MCP
configuration. Supporting files/imports/dependencies are not recursively hashed.
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
- Future context envelopes must keep bound identity separate from live Record,
  relationships, comments/history and complete Project Scope, with freshness
  metadata. Never expose the persisted MCP configuration as public context.
- Missing companion/direct files must not prevent existing authenticated actions,
  reconciliation or retirement. Direct-input validation belongs only to new
  execution and supported reload. It must not erase ownership evidence, prove
  process exit, release capacity, or replay one-shot launch.

The actual runtime identity/enforcement checks and context schemaVersion are
subsequent Task deliverables, not fields claimed persisted by this foundation.
