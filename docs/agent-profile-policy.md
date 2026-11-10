# Agent Profile policy boundary

Implementation contract for Spec #614, Task #615. Operator CLI, authenticated
Assignment context, preflight ordering and closed runtime loading are subsequent
Tasks. **Registered policies cannot yet launch through Herdr.** This is not a
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
