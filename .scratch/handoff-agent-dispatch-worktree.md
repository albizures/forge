# Handoff: autonomous V1 Task-agent dispatch

## Status

A grilling/design session established a deliberately small V1. No production implementation has been made. The working tree contains glossary work in [`crates/CONTEXT.md`](../crates/CONTEXT.md); inspect its diff before further edits.

The current adapter dispatches Pi from the Project main Worktree. Relevant current code:

- Dispatch context supplies only `registered_project_folder`: [`crates/app/src/lib.rs`](../crates/app/src/lib.rs) (`AgentDispatchContext`, `agent_dispatch_context`).
- Herdr finds a workspace matching that folder and creates its tab with that cwd: [`crates/server/src/herdr.rs`](../crates/server/src/herdr.rs) (`workspace_id`, `provision`).
- Managed Worktrees are separately/manual created and not consulted by dispatch: [`crates/app/src/lib.rs`](../crates/app/src/lib.rs) (`create_managed_worktree`).
- Current `claim_next_agent_assignment` claims an Open Task and changes it to In Progress, so it cannot implement the V1 lifecycle directly: [`crates/store/src/lib.rs`](../crates/store/src/lib.rs).

Existing provider-design context is in [`docs/research/herdr-adapter-socket-contract.md`](../docs/research/herdr-adapter-socket-contract.md). It describes the old per-Project existing-workspace model and needs revision if implementation proceeds.

## V1 settled design

1. The **Dispatcher** (Forge’s scheduling/dispatch application service) creates Task worktrees itself. Operators do not pre-create ordinary Task worktrees and agents never run in a Project’s Registered Folder/main Worktree.
2. A selected Task uses a deterministic branch named `feat/forge-<record-id>` and a Managed Worktree. The branch is based on the Project Registered Folder’s `HEAD` **at dispatch time**; Forge records that chosen commit as checkout evidence.
3. A Project has capacity 1 by default. The Dispatcher selects the oldest dependency-ready Open Task when capacity is available. A replacement attempt reuses the Task’s existing successful checkout rather than resetting or recreating it.
4. The V1 Dispatcher is autonomous: relevant Task/dependency/capacity changes trigger bounded scheduling, with periodic reconciliation as a fallback. Endpoint/workspace failures create no new work.
5. V1 has one Forge-created, reusable Forge Dispatch Workspace with dedicated Herdr tab/pane resources for assignments. Implement only the lifecycle needed to create/reuse it and fail new dispatch safely when it is unavailable.
6. Do not introduce a separate Dispatch Reservation. Create one durable `Provisioning` Agent Assignment immediately before the first checkout or provider side effect; it occupies Project capacity. If checkout or provisioning fails, close it and leave the Task unstarted/unassigned for a later retry.
7. The Assignment ID is the stable provider idempotency/resource identity. Persist each created provider resource immediately so reconciliation can discover or retire the same attempt after an uncertain response.
8. After successful provisioning, only the bound Assignment Capability starts its Open Task. Task start is idempotent.
9. A Task Agent Assignment uses its Task’s Managed Worktree. Keep the checkout and branch after success, failure, replacement, or completion; V1 performs no automatic destructive Git cleanup.
10. If a branch collision, missing base commit, failed checkout/setup, or invalid checkout is found, fail safely with diagnostics and require manual operator/Git recovery. Do not silently adopt another branch, change the base, rerun an unsafe hook, reset, rebase, or delete evidence.
11. Reject adding a dependency to an actively assigned In Progress Task. This avoids needing to stop a live agent in V1.

### Minimal V1 lifecycle

```text
oldest dependency-ready Open Task, capacity 1
  → persist Provisioning Assignment (holds capacity)
  → create feat/forge-<record-id> worktree from Registered Folder HEAD
  → provision idempotently in the shared Forge Dispatch Workspace
  → bound agent starts Task
  → agent completes Task
  → retire provider resources and retain worktree/branch
```

## Explicitly deferred or simplified

These are intentionally **not** settled for V1. A later grilling session should decide them before they are implemented.

1. **Immutable base at Task opening** — V1 captures the base at dispatch. Decide later whether Task opening must capture an immutable base commit, how it is stored, and how delayed work should handle stale bases.
2. **Separate Dispatch Reservation** — V1 uses the Provisioning Assignment as the capacity holder. Decide later whether checkout preparation needs its own expiring durable reservation and atomic handoff.
3. **Dispatch Hold and retry-budget policy** — V1 may use existing reconciliation/retry behavior and diagnostics. Decide later about durable holds, retry scope/reset, explicit resume, and preventing unbounded replacement attempts.
4. **Operator cancellation/retirement** — V1 rejects dependency changes during active work rather than providing a hold/stop/retire workflow. Decide later how to stop an agent, clean up resources, preserve capacity safety, and resume.
5. **Setup-hook recovery** — Prefer omitting Project Setup Hooks from V1. If retained, a failed hook is manual recovery only. Decide later whether hooks must be idempotent and what repair can safely retry.
6. **Automated worktree/branch repair and cleanup** — V1 only diagnoses and requires manual Git recovery. Decide later about safety assessments, partial-checkout repair, divergent branches, worktree removal, branch deletion, force confirmation, and audit reasons.
7. **Missing/invalid checkout during active work** — V1 diagnoses and leaves recovery to an operator. Decide later whether this should trigger an automatic hold and assignment/resource retirement.
8. **Workspace ownership sophistication** — V1 creates/reuses a workspace and fails safely. Decide later about durable Revision-State identity, authoritative snapshot reconciliation, reset commands, close failure behavior, and migration of legacy assignments.
9. **Configuration changes while work is active** — V1 must not create new work when configuration is unusable. Decide later whether configuration loss/change retains capacity/resources for reconciliation, and how Project capacity reductions interact with active assignments.
10. **Manual versus agent-owned In Progress state** — V1 requires the Assignment Capability to start dispatched work. Decide later whether manual Task start is forbidden globally and how an unassigned In Progress Task is classified for replacement.

## Suggested implementation seam

Keep scheduling, checkout preparation, and provider provisioning in one Dispatcher application service. Its inputs should be Task eligibility, per-Project capacity, Managed Worktree state, and Agent Assignment state; Herdr should remain an adapter that receives a prepared cwd and stable Assignment identity.

The existing store operation that atomically claims an Open Task and starts it should be replaced with a transactionally safe creation path for a `Provisioning` Assignment. The application service then performs checkout/provider effects and closes the attempt on failure.

## Suggested next steps

1. Confirm this V1 scope before implementation.
2. Update the Herdr adapter contract and tests for the Task Managed Worktree cwd and shared dispatch workspace.
3. Define the Dispatcher module boundary, then implement store/RPC/CLI and lifecycle changes with tests.
4. Resume grilling from **Explicitly deferred or simplified** only when V1 needs those capabilities.
