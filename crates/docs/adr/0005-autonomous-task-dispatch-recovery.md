# ADR-005: Autonomous Task dispatch preserves recoverable evidence

- **Status:** Accepted
- **Date:** 2026-10-25
- **Decision scope:** Dispatcher lifecycle, Git worktree safety, and Herdr provider ownership

## Context

A Task agent must work in a Task-specific checkout without competing with a human in a Project Registered Folder. Dispatch and provider calls may crash, time out, or return uncertain results. Recreating a checkout or adopting a similarly named provider resource after such a failure can lose evidence, exceed capacity, or direct an agent at the wrong work.

## Decision

Forge autonomously selects the oldest dependency-ready Open Task scoped to one Project while that Project has capacity. It atomically creates a durable Provisioning Assignment before any checkout or provider side effect; that assignment is the sole capacity holder. The Base Commit is captured from the Project Registered Folder HEAD when the Task opens. The Dispatcher derives `feat/forge-<task-id>` and prepares the claimed Managed Worktree from that immutable commit.

Forge persists Assignment, Worktree Claim, Base Commit, Dispatch Hold, shared Dispatch Workspace, and every provider resource identity. Herdr is an adapter behind a provider-neutral port: Forge creates one retained neutral-root Dispatch Workspace, gives each Assignment a dedicated tab and pane whose cwd is the Task Managed Worktree, and uses the Assignment ID as provider identity. Recovery accepts only the complete persisted resource tuple; labels and names are hints, never adoption authority. Provider events wake reconciliation but do not mutate assignment state or establish Task completion.

On a branch collision, unavailable base, failed setup hook, invalid or missing checkout, provider/workspace failure, or uncertain cleanup, Forge preserves the branch, checkout, durable resource identities, and diagnostics. It closes failed provisioning attempts and, after three failures, applies a Dispatch Hold requiring explicit operator resume. Assignment retirement is explicit and releases capacity only after its durable retirement outcome. Only the Assignment Capability may start its dispatched Open Task; manual In Progress Tasks must be reopened before they can be dispatched.

## Consequences

- Task agents never receive a Registered Folder cwd.
- Restart and reconciliation recover Forge-owned work without duplicate provisioning or resource adoption.
- Operators investigate retained Git/provider evidence using dispatch, worktree, resume, reopen, and assignment-retirement commands rather than relying on automatic destructive repair.
- Existing SQLite stores migrate the new durable lifecycle facts automatically; legacy resources are not adopted.

## Explicitly deferred

This decision does not add automated worktree or branch cleanup/repair, automatic provider-resource replacement, mutable Task profiles, a general cancellation workflow beyond assignment retirement, or automatic resolution of shared-workspace conflicts. It also does not treat provider `done` as Forge Task completion.

## References

- [Herdr adapter socket contract](../../../docs/research/herdr-adapter-socket-contract.md)
- Spec #502, “Autonomous Task-agent dispatch with recoverable worktrees”
