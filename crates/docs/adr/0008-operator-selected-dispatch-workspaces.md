# Operator-selected dispatch workspaces and conservative agent recovery

- **Status:** Proposed
- **Decision scope:** Herdr workspace ownership, assignment liveness, and dispatch recovery
- **Supersedes:** The Forge-created workspace and deferred provider-replacement portions of ADR-005

## Context

An operator needs dispatched agents visible in an existing Herdr workspace, while each Task still requires its own Managed Worktree. Provider calls and prompt delivery can have uncertain outcomes. A missing tab, a stale heartbeat, or a revoked capability does not prove that the agent process has exited.

Automatically replacing an uncertain agent risks concurrent work in the same worktree. Automatically adopting or deleting a resource by its label risks acting on a resource Forge does not own.

## Decision

The operator selects an existing Herdr workspace with `forge dispatcher config set workspace <herdr-workspace-id>` in a selected Forge Workspace. Each Forge Workspace binds one exclusive provider identity and independently selects Task and Grilling profile revisions. The Herdr socket remains daemon-wide. Forge verifies and persists the exact identity, uses it as a shared UI container for that Forge Workspace, and never creates, adopts by label, replaces or deletes it. Dispatch requires both valid selections and a binding and is blocked if the provider is unavailable or the retained identity cannot be verified. Each assignment receives a dedicated Forge-managed tab and its initial pane; a Task agent works in its Task's Managed Worktree, not the shared workspace root. A Grilling agent explicitly starts at its Forge Workspace root with applicable Project paths in its instructions. Neither launch relies on Herdr's cwd policy or an authoritative Herdr filesystem root. No plugin or upstream Herdr change is required.

Forge reconciles using durable resource identities and authoritative evidence, not labels. Potentially live agents and uncertain resources reserve dispatch capacity, including when associated with a previously closed assignment. Forge does not replace an agent until it can establish the required safety facts. When it cannot establish process exit, it remains blocked and surfaces operator investigation rather than treating absence of a heartbeat or provider resource as proof.

Assignment-session heartbeats provide positive liveness evidence, not proof of exit or prompt readiness. Forge waits for established readiness before sending the initial prompt. An uncertain prompt outcome requires explicit, durably recorded operator resolution before another attempt.

Binding changes/removal are rejected while the owning Forge Workspace has live Assignments or retained resources, including closed uncertain attempts. Socket changes apply that guard daemon-wide and invalidate every Workspace's provider verification. Existing Assignments retain their original profile revisions and provider identities; new selections cannot redirect recovery or cleanup.

Forge preserves Managed Worktrees and assignment history during recovery. A confirmed lost agent results in a Dispatch Hold and explicit operator resume before a replacement attempt.

## Immutable launch-policy persistence (Spec #614)

An Agent Profile revision now also retains resolved launch policy, canonical
selected-resource identities, narrow direct-input fingerprints and behavioral
Markdown in the existing revision store. Registration is separate from Workspace
selection. Lifecycle-policy copies retain that launch payload unchanged. This
freezes configuration and behavior, not runtime/resource dependencies.

Assignments continue to bind the same per-Record-Type revision and exact execution
ownership facts. New-execution validation must not become a prerequisite for
existing authenticated operations, reconciliation or retirement when source files
are missing. Profile registration does not relax one-shot launch, conservative
capacity accounting or capability-delivery decisions. Registered-policy execution
uses authenticated bootstrap and Pi flags, not closed-runtime attestation; selected
MCP servers are unsupported. See [Agent Profile policy](../../../docs/agent-profile-policy.md).

## Consequences

- Operators retain ownership of the shared workspace and their other tabs.
- Workspace outages block dispatch without consuming individual Task retries.
- Safety may require indefinite recovery blocking when process exit cannot be established; capability revocation alone is insufficient.
- The Pi session integration must be delivered independently of Task checkout contents and must not silently grant directory trust.
- Prompt delivery and recovery state must survive daemon restarts.
