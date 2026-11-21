# Headless Task Execution with Pi-durable Specification

## Problem Statement

Forge currently executes dispatched work through Herdr-managed terminal resources and the Pi CLI. Operators must configure and reconcile Herdr Workspaces, tabs, panes, agent identities and prompt admission in addition to Forge's own Assignments, Check attempts and Managed Worktrees. This adds terminal-oriented orchestration to a workflow whose authoritative completion evidence already belongs to Forge.

Operators want a simpler headless execution backend with persisted conversations and recoverable execution, without weakening ownership, authenticated Outcomes, evidence freshness, conservative capacity accounting or retirement safety. Persisting a conversation does not establish exactly-once external effects, prove process death or certify Task completion.

## Solution

Replace Herdr plus the Pi CLI in the execution path for newly created, explicitly durable-compatible Task Assignments with a Forge-owned TypeScript runner using a pinned pi-durable release. Retain the Rust daemon as the owner of scheduling, Assignment lifecycle, Check attempts, authorization, accepted Outcomes, evidence and recovery policy.

Each durable Task Assignment receives one supervised runner process, one conversation and separate SQLite runtime storage outside its Managed Worktree. The runner uses the existing authenticated CLI bootstrap and Outcome contract. A versioned Forge-owned protocol provides execution admission, observation, recovery, follow-up delivery and retirement without exposing pi-durable types throughout Forge.

Existing Herdr Assignments finish or retire through Herdr. Grilling remains on Herdr during this migration. Old Check pins are never silently reinterpreted: moving eligible existing work to durable execution requires an explicit, validated migration operation. Operators receive CLI/RPC controls before durable dispatch becomes the default.

This definition captures the confirmed design discussion. It is a filesystem Spec artifact, approved as a design baseline for planning, not a published Forge Record, Task plan or authorization to execute work.

## User Stories

1. As an operator, I want Task agents to run without Herdr terminal resources, so that automated execution has fewer orchestration dependencies.
2. As an operator, I want Forge to remain authoritative for completion, so that a runtime's final answer cannot pass a Check.
3. As an operator, I want each Assignment isolated in its own process and conversation, so that one runtime failure does not interrupt unrelated Assignments or contaminate a review's conversation.
4. As an operator, I want execution persisted outside the worktree, so that repository operations do not remove the runtime's recovery evidence.
5. As an operator, I want runner crashes to resume the same authorized Assignment, so that a crash does not manufacture another Check attempt.
6. As an operator, I want recovery to reject revoked or stale work, so that a restart cannot revive obsolete authority.
7. As an operator, I want only one writer and executor for an Assignment, so that competing restarts do not create duplicate agents.
8. As an operator, I want execution to stop when its authorization lease expires, so that a runner cannot act indefinitely while Forge is unavailable.
9. As an operator, I want interrupted external effects surfaced as uncertainty, so that a crash does not blindly repeat a push, publication or arbitrary command.
10. As an operator, I want retirement to revoke authority before terminating execution, so that cleanup cannot leave an authorized retired agent.
11. As an operator, I want incomplete cleanup to retain capacity and diagnostics, so that replacement cannot race potentially live work.
12. As an operator, I want to inspect status and paginated transcripts without starting execution, so that investigation is read-only.
13. As an operator, I want recorded follow-up input, so that I can intervene without silently changing an Assignment's Check, profile or evidence.
14. As an operator, I want a finished conversation without an accepted Outcome clearly identified, so that incomplete protocol participation is not mistaken for completed work.
15. As a Task agent, I want the existing authenticated bootstrap and Outcome commands, so that changing runtimes does not change my execution authority or completion contract.
16. As a reviewer, I want a fresh conversation bound to the approved output evidence, so that implementation dialogue does not substitute for independent review.
17. As a profile author, I want explicit compatibility validation, so that unsupported tools, resources or authentication fail before launch rather than silently changing behavior.
18. As an operator, I want existing profile revisions and Check pins preserved, so that registering a durable profile cannot redirect historical or live work.
19. As an operator, I want a validated migration for eligible old Check pins, so that adopting the new backend is deliberate and auditable.
20. As an operator, I want existing Herdr cleanup to remain usable, so that retained resources are not orphaned by rollout.
21. As a Grilling participant, I want current Herdr-based interaction retained, so that Task migration does not remove an interaction contract that has not been redesigned.
22. As an operator, I want a matched runner and locked dependencies delivered with a Forge Revision, so that execution does not depend on arbitrary npm installations.
23. As an operator, I want model-provider credentials configured separately from Assignment capabilities, so that model access does not imply Forge authority.
24. As an operator, I want explicit execution configuration, so that ambient Pi extensions, skills or settings cannot unexpectedly affect dispatched work.
25. As an operator, I want retained protected transcripts after retirement, so that failures remain diagnosable without implying the storage is safe to disclose.
26. As an operator, I want explicit transcript deletion that preserves Forge audit history, so that runtime retention can be managed independently of completion evidence.
27. As an operator, I want rollout and rollback not to move live work between backends, so that operational recovery cannot accidentally duplicate execution.
28. As a maintainer, I want deterministic crash and retirement tests plus real-provider verification, so that happy-path execution is not the only release evidence.
29. As an operator, I want the trusted-local-host limitation stated clearly, so that process separation and leases are not mistaken for hostile-agent containment.

## Acceptance criteria

### Execution and domain authority

1. **Headless durable Task execution.** An eligible Task with durable-compatible Check pins provisions a Forge-owned runner directly using pi-durable, without creating Herdr resources or launching the Pi CLI. It runs in the Task's bound Managed Worktree, not the Project Root. Durable execution does not require a Herdr Workspace binding; remaining Herdr execution retains its existing prerequisites.
   - **Verification:** Exercise explicit and automatic Task dispatch against a fake runner/provider boundary with Herdr unavailable. Assert the bound working directory, no Herdr calls or Pi CLI launch, and successful durable admission. Verify that a Grilling or retained Herdr Assignment still uses Herdr prerequisites.
2. **Forge remains authoritative.** Dependency gates, Execution Mode, Spec authorization, capacity, Check ordering, pinned profiles, freshness generations, repository evidence, Findings, Continuations, Stack inheritance and publication rules remain enforced. A runtime's idle, done or answered state never creates an Assessment or completes a Task.
   - **Verification:** Run existing dispatch/Check/Stack regressions with the durable adapter and inject runtime completion without an Outcome. Verify no pass, Task completion or unauthorized next Check.
3. **Existing authenticated agent contract.** The runner delivers protected Assignment configuration and the short bootstrap contract. Work begins only after successful `forge assignment bootstrap`; accepted Outcomes use the existing capability-authenticated submission path. Secrets are not placed in prompts, environment values, protocol logs or transcripts.
   - **Verification:** Test successful bootstrap and Outcome submission, failed/uncertain bootstrap, wrong Assignment identity, revoked capabilities and malformed/stale Outcomes. Inspect launch arguments, environment metadata and retained outputs for secret leakage.
4. **Assignment execution boundary.** Each Assignment has one runner process, one conversation and one separate SQLite runtime store outside the worktree. Independent reviews and successor Assignments do not reuse the predecessor conversation as their execution context. They receive Forge-approved bound context, evidence and handoffs.
   - **Verification:** Dispatch concurrent Assignments and an Implementation-to-Review successor. Verify distinct process, conversation and storage identities, correct per-Assignment context and failure isolation.

### Admission, persistence and recovery

5. **Versioned, idempotent runner protocol.** Launch/admission, observation, recovery, follow-up delivery and retirement have a versioned Forge-owned contract. Stable Assignment and operation identities allow retries to discover the original durable result rather than admit duplicate execution or messages. An unknown result remains explicit until reconciled.
   - **Verification:** Inject dropped requests/replies and crashes before and after durable admission and Forge result persistence. Replay the same operation; verify one conversation and one intended submission. Conflicting reuse is rejected rather than overwritten.
6. **Exclusive ownership before storage open.** Concurrent starts/restarts cannot create multiple writable runtime owners or executors for an Assignment. The host enforces exclusive ownership; SQLite transactions alone are not treated as the pi-durable ownership protocol. Process identifiers or labels alone cannot authorize adoption or termination.
   - **Verification:** Race two starts, reconnects and recoveries. Verify exactly one writable owner; the loser cannot resume tools or repair writable storage. Exercise stale ownership records and process-identity reuse without adopting an unrelated process.
7. **Authorized same-Assignment crash recovery.** Recovery resumes the existing conversation and Check attempt only after Forge validates current Assignment authority, profile/binding identity, applicable evidence/freshness gates, exclusive ownership and unresolved effects. It does not create another Check attempt or accepted Continuation. Safe recovery can be automatic; uncertainty blocks it with retained diagnostics.
   - **Verification:** Restart after a safely persisted boundary and verify unchanged Assignment/attempt/conversation identities. Repeat after retirement, freshness invalidation, authorization failure and ownership uncertainty; verify no model request or tool action resumes.
8. **Bounded authorization during execution.** A runner must hold a valid Forge authorization lease to admit new model requests or tool calls. It cannot indefinitely renew locally while Forge is unavailable. Expiry prevents new work, initiates cancellation of active execution and preserves uncertainty until reconciliation; expiry is not proof that child processes exited.
   - **Verification:** Use a fake clock and interrupt daemon connectivity during idle, model generation and a long-running command. Verify no post-expiry admissions, cancellation attempts, retained blockers where exit is uncertain and no unauthorized restart. Document and test the selected lease/renewal timing.
9. **Conservative interrupted effects.** Arbitrary shell commands are not automatically replayed after interruption. An uncertain external effect is exposed and blocks automatic recovery until inspected/resolved. Only explicitly justified replay-safe operations may replay. Resolution does not automatically rerun the interrupted command or establish exactly-once semantics.
   - **Verification:** Kill the runner after a fake external mutation but before its result commits. Verify the effect occurs once and the command is not automatically retried. Test interrupted pushes/publication with controllable external endpoints and require evidence-based resolution before further execution.
10. **No recovery-by-destructive-worktree-repair.** Restart, retirement and runtime cleanup preserve worktree contents and existing retained-work authorization rules. They do not commit, reset, stash, clean or discard repository changes to make recovery eligible.
    - **Verification:** Interrupt an Assignment with tracked, staged and untracked changes. Verify bytes and Git state remain preserved, with the expected existing freshness/salvage diagnostics rather than automatic adoption or cleanup.

### Retirement and operator intervention

11. **Revocation precedes retirement cleanup.** Retirement durably revokes Assignment authority before stopping the runner, cancelling runtime work and terminating its supervised process group. Forge verifies cleanup using retained ownership evidence. A successful abort, missing heartbeat, missing pane, absent runner or expired lease alone is insufficient evidence that all relevant work is safe.
    - **Verification:** Assert ordering with a fake runner and supervised processes. Exercise delayed abort, surviving child, unavailable runner and identity mismatch. Verify authority is already revoked, resources retained and capacity reserved until reconciled. Existing explicit safety-override rules remain auditable, not automated proof of death.
12. **Uncertain resources continue to reserve capacity.** Closed Assignments with unresolved launch, ownership, external-effect or retirement uncertainty retain recovery blockers across daemon/runner restart. No replacement launches merely because a process exits or a conversation ends.
    - **Verification:** Persist each uncertainty class, restart and attempt dispatch. Verify retained diagnostics, capacity and rejection of replacement until the relevant safety gates pass.
13. **Missing Outcome is unresolved work.** A final answer without an accepted Outcome produces a visible missing-Outcome diagnostic. It does not synthesize an Assessment, Continuation, Task completion or successor launch. Explicit authorized intervention remains possible.
    - **Verification:** End a conversation with plausible completion prose but no accepted Outcome; also test a rejected Outcome. Inspect Task/Assignment state and attempt follow-up, retirement and reconciliation.
14. **Read-only CLI/RPC inspection.** Operators can inspect persisted Assignment status, backend/resource identities, lease/recovery/retirement diagnostics and paginated committed transcripts through Forge CLI/RPC. Inspection neither submits input nor starts/resumes scheduling and works when the original runner is stopped. An unavailable runner is shown as uncertainty rather than inferred progress.
    - **Verification:** Inspect running, crashed, retired and missing-Outcome Assignments using both interfaces. Verify stable pagination over committed history and zero execution admissions/model/tool calls, including inspection alongside an active writer.
15. **Recorded, bounded follow-ups.** Follow-up input is durably recorded and delivered idempotently to the intended authorized Assignment. It cannot change its Check, pinned profile, attempt identity, freshness generation or evidence. Scope/evidence changes use Forge's existing invalidation and replacement rules. Revoked Assignments reject new execution input.
    - **Verification:** Retry a follow-up across an uncertain reply and verify one admission. Attempt follow-ups after revocation and requests to rebind a reviewer; verify immutable identity/evidence and normal rejection/invalidation behavior. Observe recorded input in the transcript.
16. **Protected retention and explicit deletion.** Retirement preserves runtime storage by default with owner-only access. A separate explicit deletion operation is allowed only after verified cleanup and safe storage ownership; it cannot delete an active runtime store or erase Forge Assignment, Outcome, Assessment, migration or recovery audit history.
    - **Verification:** Inspect permissions, retire and reopen retained history, reject deletion with active ownership or unresolved cleanup, then delete eligible runtime storage. Confirm Forge audit/evidence remains inspectable and cannot be replayed as authority to recreate work.

### Configuration, compatibility and migration

17. **Explicit durable-compatible profiles.** New durable execution uses an explicitly registered compatible Agent Profile revision. Preflight validates model, thinking, tools, approved resources, instructions/context loading and runtime compatibility. Unsupported or missing inputs fail closed without ambient Pi discovery, implicit Herdr fallback or new capability issuance. Missing source resources do not prevent historical inspection, reconciliation or retirement.
    - **Verification:** Exercise compatible profiles and unsupported extension/skill/tool/model/context configurations. Modify ambient Pi settings and verify they do not affect execution. Remove original profile sources and verify existing Assignment inspection/retirement still works while new dispatch rejects as appropriate.
18. **Explicit provider authentication.** Model authentication is supplied through deliberately configured host mechanisms, independently of Assignment capabilities. The supported authentication matrix is documented and tested; copying the operator's entire Pi configuration is not a compatibility strategy. Missing credentials fail preflight without disclosure.
    - **Verification:** Test every claimed supported authentication mechanism with isolated configuration and negative cases. Verify model credentials grant no Forge authority, capabilities grant no model-provider access and secrets are absent from durable records and diagnostics.
19. **Matched release components.** The runner, pinned pi-durable release and locked JavaScript dependencies are distributed as a matched Forge Revision component. Preflight rejects missing or incompatible components with an actionable diagnostic. Revision selection and installation locate the matched runner rather than an arbitrary npm/PATH installation.
    - **Verification:** Install/select a Revision and dispatch with its packaged runner. Test missing runner, incompatible protocol/dependencies and Revision switching with retained Assignments. Existing cleanup never silently switches to incompatible runtime code.
20. **Explicit Check-pin migration.** Existing immutable profile revisions and historical Check/attempt bindings are never rewritten or silently reinterpreted. An explicit validated operation permits eligible existing Checks to select durable-compatible revisions, only after active ownership is absent and known/unknown resources are reconciled. Migration records prior/new effective pins, invalidates freshness and stale passes/attempts, and preserves historical evidence. New Assignments use the explicitly selected compatible pin; current template/Workspace selection alone cannot redirect old Checks.
    - **Verification:** Migrate an eligible Task and verify durable next dispatch, retained historical pins and invalidated evidence. Reject active, uncertain, cross-scope and incompatible migrations. Retry a migration without duplicate history or freshness advancement; reject conflicting reuse. Verify unrelated Check identity, description and order remain unchanged.
21. **Backend-specific rollout and rollback.** Existing Herdr Assignments finish or retire through their original adapter and identities. Grilling remains explicitly routed through Herdr. Default durable Task dispatch is enabled only after the verification gate; disabling it stops new durable launches without moving existing durable Assignments to Herdr. A different-backend successor requires safe cleanup and explicit compatible profile/pin selection.
    - **Verification:** Operate mixed-backend Assignments during rollout, change defaults and disable durable launches. Verify old cleanup remains correctly routed, Grilling behavior is unchanged and no fallback creates a duplicate Assignment. Test unsupported backend configuration as a fail-closed prerequisite.
22. **Release verification gate.** Retained verification includes a real-provider Implementation-to-Review cycle with authenticated Outcomes and exact evidence, plus deterministic tests for duplicate admission, daemon and runner crashes, lease expiry, interrupted effects, revoked recovery, missing Outcomes, incompatible components and incomplete retirement. Default dispatch cannot be enabled based only on a happy-path demo. Upgrades renew the applicable verification before rollout.
    - **Verification:** Produce a reproducible verification report identifying the tested Forge Revision, runner/protocol/dependency versions, commands, expected versus observed behavior and unrun gates. Verify changed implementation/review output, review evidence binding and no next Check before accepted Outcome and safe retirement.
23. **Honest operational/security documentation.** Documentation covers setup, explicit profile migration, status/transcripts, follow-ups, authorization outage behavior, interrupted-effect investigation, retirement, deletion and rollback. It explicitly states the trusted-local-host boundary, separate-process limitations and absence of exactly-once or process-death guarantees from persistence/leases alone.
    - **Verification:** Run documented operations against retained integration fixtures and the release verification environment. Review claims against observed behavior and ensure no documented safety shortcut treats inspection, a hold clearance or credential revocation as proof of exit.

## Implementation Decisions

- Keep Forge's Rust application/domain/store ownership. Extend the provider-neutral dispatch port and server adapter selection only where needed; do not move Task lifecycle or Check completion into pi-durable.
- Use a small separately supervised TypeScript runner directly consuming pi-durable, not the Pi CLI. Pin the exact dependency and isolate it behind a versioned Forge-owned protocol.
- Use one process, conversation and SQLite runtime store per Assignment initially. Keep runtime storage outside Managed Worktrees and separate from Forge's database. Treat Forge/runtime persistence as separate commits requiring stable identities, idempotency and reconciliation, not a distributed atomic transaction.
- Persist Assignment-to-backend/runtime ownership so mutable current configuration cannot redirect existing observation, recovery or cleanup. Host-enforced exclusive ownership precedes writable storage opening or scheduling.
- Keep runtime storage and ownership recoverable across daemon/runner restart and compatible Revision operations. Do not choose an ownership/locking or process-supervision mechanism solely because pi-durable persists execution state.
- Start and resume runtime work only through Forge authorization. Use bounded renewable leases during execution; lease expiry initiates cancellation, blocks new work and retains uncertainty until verified reconciliation.
- Treat pi-durable internal tasks as execution machinery, not Forge Tasks, Checks or Assignments. Runtime tasks cannot accept Outcomes, adopt repository evidence or authorize successors.
- Keep existing protected capability delivery and CLI bootstrap/Outcome commands. Capability acknowledgement, successful bootstrap, heartbeat, lease validity and verified process absence remain distinct facts.
- Distinguish authorized recovery of the same Assignment from a deliberate Implementation Continuation. A Continuation remains an accepted Forge handoff followed by safe retirement and a separately authorized successor with a fresh conversation.
- Configure arbitrary shell tools as non-replay-safe. Detect and retain interrupted-effect uncertainty independently of whether pi-durable presents an interrupted tool result. Automatic recovery must not bypass the inspection gate by immediately asking the model to repeat the command.
- Revoke before cleanup; supervise the runner and its process group; preserve blockers if execution may survive. Same-user processes can escape supervision, so cancellation acknowledgement or runner absence is not universal proof of safety.
- Map explicitly compatible Agent Profiles to model/thinking/tool choices, approved instructions/resources and execution directory. Define repository context loading explicitly. Existing Pi extension/skill APIs and provider authentication are not assumed compatible with pi-durable.
- Add an explicit, auditable Check-profile migration exception to current immutable-pin behavior. Preserve immutable Check identity/description/order and historical pin/attempt evidence; record effective replacement selection with freshness invalidation. Do not implement this as a direct overwrite of historical rows or an undocumented template side effect.
- Keep durable Task readiness independent of Herdr-specific container prerequisites. Preserve original Herdr binding guards and cleanup for remaining Herdr Assignments and Grilling.
- Add operator controls through Forge CLI/RPC first. Read-only observation must not start scheduling or take writable ownership; committed transcript pagination must preserve relevant ordering and identity.
- Retain protected runtime history after retirement; runtime deletion is a separate operation after cleanup, not part of Assignment retirement. Forge's durable audit remains separate.
- Extend Forge Revision packaging/attestation/preflight as necessary to locate and validate matched runner components. Packaging must account for the supported JavaScript runtime and locked dependencies; silently using whichever npm installation is available is prohibited.
- Preserve the trusted-local-host security boundary. This migration does not introduce hostile-agent isolation, provider-container ownership or permission to expose stored repository content.

## Testing Decisions

- Test public behavioral contracts and persisted safety invariants, not private function layouts or incidental pi-durable task names.
- Cover application dispatch/Check transitions, store persistence/restart, server adapter/protocol integration, CLI/RPC operator behavior, profile preflight, packaging and real runner/tool execution.
- Reuse existing patterns for scripted provider sockets, uncertainty injection, capability/bootstrap tests, capacity reservation, workspace dispatch, Check attempts and retained-work recovery. Replace Herdr-specific fixtures only where the new backend genuinely differs; retain Herdr regression coverage during mixed-backend operation.
- Use fake clocks for lease validity, renewal loss, expiry, retry timing and restart behavior. Test interruption at commit and external-effect boundaries, including after an effect succeeds but before result persistence.
- Test real process behavior in addition to protocol fakes: duplicate starts, orphaned/surviving children, termination ordering, storage ownership, abrupt runner/daemon death and read-only observation beside a writer.
- Simulate Git/hosting external effects with controlled endpoints to count actual mutations. Provider request/submission deduplication alone is not evidence that an arbitrary command is exactly-once.
- Use isolated credential/configuration environments. Test both positive compatibility claims and fail-closed unsupported combinations without depending on a developer's ambient Pi installation.
- Maintain criterion-specific verification above and a retained release verification report. Clearly label deterministic versus real-provider coverage, observed behavior, unresolved failures and unrun gates.
- Do not claim physical power-loss durability from process-crash tests. Verify/document the selected SQLite configuration and its actual guarantees.

## Domain changes

Proposals below are part of this Spec only; applying them is implementation work. The actual Forge Runtime glossary currently lives in `crates/CONTEXT.md`, linked from the root context map; do not create a competing root glossary.

### Glossary

**Target: `crates/CONTEXT.md` — add**

```markdown
**Assignment Execution Recovery**:
Resumption of an interrupted Agent Assignment under its existing Check attempt and bound authority after Forge validates that continuation is safe. It is distinct from a deliberate Continuation and does not authorize a replacement Agent Assignment.
_Avoid_: Continuation, replacement attempt, automatic retry

**Assignment Authorization Lease**:
A time-bounded grant permitting an Agent Assignment to continue authorized execution, subject to revocation and the Assignment's bound scope. Expiration withdraws that permission but does not prove that the Agent or its external actions have stopped.
_Avoid_: Assignment Heartbeat, proof of exit, Assignment Capability
```

**Target: `crates/CONTEXT.md` — change `Forge Workspace`**

```markdown
**Forge Workspace**:
A Forge-owned boundary with a fixed filesystem root that groups Projects and Records and owns dispatch profile selections and applicable provider bindings. A provider's user-interface container is not the Forge Workspace's filesystem or execution identity.
_Avoid_: Managed Worktree, Herdr Workspace, task workspace
```

**Target: `crates/CONTEXT.md` — change `Check`**

```markdown
**Check**:
A named Task completion criterion with immutable identity, description and order, initially copied with its optional Agent Profile revision from a Check Template or provided as a mandatory baseline. Every Check is required for completion; its selected profile supplies dispatched-agent configuration, and an explicit validated profile migration preserves historical pins while invalidating prior evidence rather than silently reinterpreting the original revision.
_Avoid_: executable check, check runner
```

**Target: `crates/CONTEXT.md` — change `Agent Profile`**

```markdown
**Agent Profile**:
An immutable, revisioned Forge-owned configuration for a Record Type containing agent launch policy and role-specific behavioral instructions, subordinate to Forge-owned execution and lifecycle rules. Task Checks select revisions initially copied from Check Templates and replaceable only through explicit validated migration; Forge Workspaces select Grilling revisions, while each Agent Assignment retains its originally bound revision.
_Avoid_: agent configuration, mutable profile, record profile
```

**Target: `crates/CONTEXT.md` — change `Forge Dispatch Workspace`**

```markdown
**Forge Dispatch Workspace**:
An existing operator-selected Herdr Workspace bound exclusively to one Forge Workspace within a daemon/store for Herdr-backed dispatch. It contains dedicated Agent Assignment tabs but is neither a Forge-owned filesystem root nor a prerequisite for other execution backends.
_Avoid_: Forge Workspace, Project workspace, task workspace
```

Keep the existing `Task`, `Continuation` and `Assignment Session` meanings. In technical integration documentation, qualify pi-durable's internal tasks as execution tasks; they are not a second domain Task type and do not require a generic programming-term glossary entry. A one-conversation-per-Assignment runner policy does not change the existing historical/bootstrap association semantics of Assignment Session.

### ADRs

Proposed filenames follow the current highest ADR number; check numbering again when applying them.

**Proposed `crates/docs/adr/0012-headless-durable-task-execution.md` — full text**

```markdown
# Headless durable execution behind Forge-owned Assignment authority

Forge Task execution currently couples durable work ownership to Herdr terminal resources and Pi CLI processes. Use a pinned pi-durable release through a matched, separately supervised TypeScript runner behind a versioned Forge-owned protocol, while the Rust application retains all scheduling, Check, evidence and authenticated Outcome authority; give each Assignment its own process, conversation and separate runtime store to isolate failures and preserve independent reviews.

This trades additional runner packaging and cross-store reconciliation for removal of terminal-container orchestration and recoverable execution. It partially supersedes the Herdr-only provisioning requirements of ADR-005 and ADR-008 for durable Task Assignments, preserves ADR-007's protected capability delivery, and retains Herdr routing and cleanup for existing Assignments and Grilling. Separate processes are not security sandboxes, and runtime completion is never Task completion.
```

**Proposed `crates/docs/adr/0013-authorized-durable-recovery-and-conservative-retirement.md` — full text**

```markdown
# Authorize durable recovery without treating persistence as proof of safety

Durable execution can resume work after its authority has been revoked or after an external effect occurred without a recorded result. Require exclusive execution ownership, Forge authorization before scheduling and bounded authorization leases during execution; arbitrary shell commands are not replay-safe, and uncertain effects block automatic recovery pending inspection.

Revoke authority before retirement cleanup and preserve capacity blockers until owned execution and resources are reconciled. This sacrifices availability during outages or ambiguous effects to prevent duplicate or obsolete agents from acting; expired leases, abort acknowledgements and missing runners are not proof of process death. Safe crash recovery retains the same Assignment and attempt, while accepted Continuations remain distinct domain handoffs.
```

**Proposed `crates/docs/adr/0014-explicit-backend-and-check-profile-migration.md` — full text**

```markdown
# Migrate backend selection explicitly without rewriting historical execution

Immutable Check profile pins make changing default execution backends insufficient for existing work. Permit explicit validated profile migration only after ownership and resource reconciliation, retain original and replacement selections as history, and invalidate freshness and stale evidence; preserve Check identity, description, order and every existing Assignment's original profile/backend binding.

This deliberately introduces a narrow exception to immutable effective Check pins rather than silently interpreting Herdr profiles as durable profiles or abandoning existing Tasks. Rollout and rollback affect new execution only: live or uncertain Assignments always recover or retire through their original backend, and another-backend successor requires safe cleanup and explicit compatible selection.
```

## Out of Scope

- Rewriting the Rust daemon or moving Forge domain/lifecycle authority into pi-durable.
- Migrating live conversations or Assignments between Herdr and pi-durable.
- Migrating Grilling interaction or replacing Herdr for every workflow in this release.
- Terminal attachment, a graphical operator interface or parity with all Pi CLI extensions and settings.
- General arbitrary-tool replay or exactly-once external execution.
- Automatic acceptance of prose answers as Outcomes, automatic missing-Outcome replacement or synthesized Continuations.
- Automatic destructive worktree repair, unreported commit adoption, bypassing retained-work authorization or changing Stack/publication semantics.
- Ambient Pi configuration discovery, blanket credential copying or assuming all provider authentication methods work.
- Hostile-agent containment, containers, per-agent OS identities or protection against arbitrary malicious same-user processes.
- Distributed/multi-host execution, shared multi-Assignment runner optimization or a generic workflow engine replacing Forge.
- Automatic transcript deletion, erasure of Forge audit history or physical power-loss guarantees beyond the verified storage configuration.
- Task creation, implementation, dispatch administration or publication of this filesystem artifact as a Forge Record.

## Notes

### Settled scope versus implementation investigation

The design is settled; several technical details require source inspection, implementation design and verification rather than assumed compatibility:

- Exact pi-durable release/API, supported JavaScript runtime, dependency packaging and storage upgrade compatibility.
- Concrete runner protocol schemas, stable operation IDs, ownership/fencing mechanism and exclusive read/write storage access.
- Lease duration, renewal cadence and bounded cancellation behavior, including daemon restart and uninterruptible/escaped processes.
- Supported provider authentication, resource/skill/extension translation and explicit repository instruction loading.
- A durable uncertainty representation and operator resolution path for interrupted external effects, without weakening existing safety confirmations.
- Revision switching/upgrade behavior for retained stores, original runtime identity and cleanup components.
- The precise persisted migration representation and CLI/RPC command shape for explicit effective Check-pin replacement.

These are not permissions to weaken the acceptance criteria. If upstream behavior cannot satisfy a safety gate, retain a blocker and return the material trade-off for explicit decision before rollout. Do not silently broaden compatibility, add fallback or manufacture evidence.

### Current repository constraints

- Current Check documentation describes immutable Check pins and no individual Check editing. The explicit migration operation in this Spec is a deliberate new exception, not an existing command; update affected domain and operational documentation together.
- Current provider selection and readiness contain Herdr-specific assumptions beyond the nominal provider-neutral trait. Durable execution must not accidentally inherit Herdr Workspace prerequisites, and removing those prerequisites must not weaken retained Herdr cleanup guards.
- Existing source-profile validation must remain separate from recovery/retirement prerequisites. Lost source resources cannot make historical cleanup impossible.
- Existing operator RPC is trusted local authority, not agent authentication. New operator controls do not expand Assignment capabilities or claim a hostile-agent security boundary.
- pi-durable documentation examined during feasibility work calls the API experimental, describes single-writer host ownership, distinguishes replay-safe tools from interrupted calls and supports persisted submissions/recovery. Revalidate these contracts against the pinned source; README claims are not the project's release verification evidence.

### Relevant skills

- `forge` — public Record, dispatch, profile and recovery operations; avoid persistence edits.
- `domain-modeling` — reconcile proposed vocabulary and ADRs with the runtime context.
- `review-implementation` — compare delivered behavior and verification against this definition.
- `review-coding-standards` — review implementation against the project's coding standards.

### Relevant files and sources

Paths below are relative to the Forge repository unless noted otherwise.

- `crates/app/src/lib.rs` — provider-neutral dispatch port, Assignment facts and observation types.
- `crates/app/src/assignment.rs` — Assignment application behavior.
- `crates/app/src/workspace_dispatch.rs` — dispatch selection and orchestration.
- `crates/server/src/herdr.rs` — current provider launch, observation, recovery and cleanup; retained Herdr support.
- `crates/server/src/lib.rs` — adapter factory, capability delivery, reconciliation and server integration.
- `crates/server/src/agent_profiles.rs` — server profile integration.
- `crates/store/src/task_attempts.rs` — Check-attempt persistence and evidence boundaries.
- `crates/store/src/workspace_dispatch.rs` — durable dispatch/resource state.
- `crates/server/src/task_check_attempt_tests.rs` — existing Check-driven integration tests.
- `crates/server/src/workspace_recovery_tests.rs` — provider uncertainty and conservative recovery tests.
- `crates/server/src/assignment_actions_tests.rs` — authenticated actions and lifecycle boundaries.
- `crates/server/src/retained_work_tests.rs` — retained-work safety coverage.
- `crates/forgeup/` — matched Revision installation/runtime distribution integration.
- `crates/build-info/` — build and Revision identity.
- `crates/CONTEXT.md` and `GLOSSARY-MAP.md` — actual domain language and context routing.
- `crates/docs/adr/0005-autonomous-task-dispatch-recovery.md` — durable ownership and retained evidence.
- `crates/docs/adr/0007-assignment-capability-delivery-to-pi.md` — protected capability delivery.
- `crates/docs/adr/0008-operator-selected-dispatch-workspaces.md` — conservative resource recovery and Herdr binding.
- `docs/dispatch.md`, `docs/assignment-recovery.md`, `docs/assignment-context.md`, `docs/agent-profile-policy.md` and `docs/workspaces.md` — operational contracts requiring coordinated updates.
- `docs/profiles/implementation-continuations.md` — accepted Continuations versus crash recovery.
- `CODING_STANDARDS.md` — implementation conventions.
- https://github.com/earendil-works/pi/tree/main/packages/durable — upstream package; pin an exact revision/release before implementation verification.
- https://github.com/earendil-works/pi/blob/main/packages/durable/README.md — upstream documented execution, storage and replay semantics; mutable reference, not an API stability guarantee.
