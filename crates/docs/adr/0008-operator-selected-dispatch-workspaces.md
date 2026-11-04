# Operator-selected dispatch workspaces and conservative agent recovery

- **Status:** Proposed
- **Decision scope:** Herdr workspace ownership, assignment liveness, and dispatch recovery
- **Supersedes:** The Forge-created workspace and deferred provider-replacement portions of ADR-005

## Context

An operator needs dispatched agents visible in an existing Herdr workspace, while each Task still requires its own Managed Worktree. Provider calls and prompt delivery can have uncertain outcomes. A missing tab, a stale heartbeat, or a revoked capability does not prove that the agent process has exited.

Automatically replacing an uncertain agent risks concurrent work in the same worktree. Automatically adopting or deleting a resource by its label risks acting on a resource Forge does not own.

## Decision

The operator selects an existing Herdr workspace. Forge verifies and persists its exact identity, uses it as a shared UI container, and never deletes it. Each assignment receives a dedicated Forge-managed tab and its initial pane; the agent works in its Task's Managed Worktree.

Forge reconciles using durable resource identities and authoritative evidence, not labels. Potentially live agents and uncertain resources reserve dispatch capacity, including when associated with a previously closed assignment. Forge does not replace an agent until it can establish the required safety facts. When it cannot establish process exit, it remains blocked and surfaces operator investigation rather than treating absence of a heartbeat or provider resource as proof.

Assignment-session heartbeats provide positive liveness evidence, not proof of exit or prompt readiness. Forge waits for established readiness before sending the initial prompt. An uncertain prompt outcome requires explicit, durably recorded operator resolution before another attempt.

Forge preserves Managed Worktrees and assignment history during recovery. A confirmed lost agent results in a Dispatch Hold and explicit operator resume before a replacement attempt.

## Consequences

- Operators retain ownership of the shared workspace and their other tabs.
- Workspace outages block dispatch without consuming individual Task retries.
- Safety may require indefinite recovery blocking when process exit cannot be established; capability revocation alone is insufficient.
- The Pi session integration must be delivered independently of Task checkout contents and must not silently grant directory trust.
- Prompt delivery and recovery state must survive daemon restarts.
