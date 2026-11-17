# T03 — Inspect and explicitly reset continuation budgets

## Authority and input

Parent: [local Spec](../implementation-continuations.md). Read the [shared execution contract](README.md#shared-execution-contract). Project: `factory/forge`. **Depends on T02; Builds-On T02.** Inherit T02's verified bounded-continuation implementation.

Spec AC: **12–13**, operator verification aspects of **9–11, 20–21**.

## Outcome

An operator can inspect continuation accounting and handoff evidence through supported human/JSON interfaces and explicitly renew allowance with durable attribution, without silently resetting unrelated safety state or launching an agent.

## Scope

- Expose per-Check continuation total, consecutive-unchanged count, accepted checkpoint/handoff context, exhaustion diagnostics and reset history through public read interfaces. Preserve structured machine output and useful human diagnostics.
- Add an explicit operator reset with nonblank actor and explanation. Retain an append-only reset audit and historical outcomes. Document exact reset effects and public command/RPC spelling.
- Implement the settled reset of continuation accounting independently of remediation and interrupted retries. Renew allowance only through this explicit operation, not through ordinary resume, body changes, invalidation or mode/dispatch retries.
- Keep counter reset distinct from clearing a Dispatch Hold, verifying old-agent absence, resolving retained resources, or dispatching. Explain the safe ordered operator workflow; clearing a hold alone cannot bypass exhausted counters.
- Do not weaken capability boundaries by exposing operator reset as an Assignment-owned mutation. Preserve Workspace/Project/Task/Check routing and identity isolation.
- Persist audit and reset atomically, assess wire/storage compatibility, and update affected help and examples. Report evidence as changed/unchanged, not certified meaningful progress.

## Observable acceptance and verification

1. Human and JSON inspection show consistent active, unchanged-stalled, total-exhausted, and reset states with useful checkpoint context and retained history.
2. Blank attribution or explanation rejects without mutation. Successful reset durably records exactly the authorized accounting effects across reopen.
3. Reset does not erase Assessments, Findings, prior handoffs, evidence, interruption/remediation history, or resource blockers; it cannot launch an agent or authorize replacement.
4. Exhaustion survives ordinary hold clearing/resume and remediation reset. After explicit accounting reset and the required separate safety/hold steps, normal eligibility may resume.
5. Public operator requests cannot be impersonated through an Assignment capability; unrelated scope cannot mutate the selected counters.

Run public CLI/RPC and persistence tests plus the shared root gates. Include concurrency/failed-transaction coverage for durable audit, and report actual results/unrun gates.

## Implementation evidence

- Status: complete (local source implementation only).
- Input commit: `5ca90d2a4d63a8375d4f36e18dcb55695a7e0bbf` (T02's implementation and recorded verification), in
  the user-authorized `forge/` checkout. No managed Assignment was created.
- Output commit: `41d4054f2f6a0e4631c30eb2739e318590fbed95`.
- Public inspection: `forge record check work <task> --workspace <id>` and
  `--json` expose totals, consecutive-unchanged accounting, retained checkpoint
  and full handoff, exhaustion diagnostics and append-only reset history. Human
  output explicitly distinguishes evidence change from meaningful progress;
  structured WorkState output remains separate from diagnostics.
- Public reset: `forge dispatcher reset-continuations <task> --check <check>
  --workspace <id> --actor <operator> --message <explanation> [--json]`, through
  selected operator RPC `record.resetContinuations`. Nonblank attribution and
  explanation are mandatory. Workspace, Task and Check identities are checked
  in the writer transaction. The operation resets only total and consecutive-
  unchanged counters and atomically appends timestamped attribution. It retains
  checkpoint/handoff, historical outcomes, Assessments, Findings, freshness,
  remediation/interruption budgets, holds, ownership and resource blockers.
  It never launches or authorizes a replacement. Repeated interventions append
  separate audit rows; outcome replay continues to return original accepted results.
- Safety workflow and trust boundary are documented in `docs/dispatch.md` and
  `docs/assignment-recovery.md`: investigate, explicitly reset if justified,
  independently retire/reconcile old ownership and resources, inspect, then
  explicitly resume. Reset itself requires no ownership transfer because it
  transfers no authority and clears no safety state. Operator attribution is a
  trusted-interface assertion, not authenticated human identity. Assignment
  capabilities have no reset operation.
- Compatibility: schema **16** permits zero continuation totals and adds
  `check_continuation_resets`. Ordinary old-store opening rejects before writes;
  explicit isolated upgrades support schemas 7–16, retain actual counter/checkpoint
  bytes through the table rebuild, and add no invented resets. Same-schema
  snapshots retain history. `CheckContinuations.resets` defaults empty when reading
  old stored JSON; historical accepted responses are not rewritten. Strict older
  clients must upgrade alongside the server. Management Protocol is unchanged.
  Storage contract and upgrade documentation are updated.
- Verification covers active, unchanged-exhausted, total-exhausted and reset human/
  JSON inspection; blank actor/explanation; missing and unrelated Check identity;
  unrelated Workspace rejection; injected audit failure with counter rollback;
  concurrent serialized resets and durable attribution after reopening; retained
  checkpoints, budgets, holds, assignments and replay; exhaustion surviving resume,
  remediation reset and body changes; and normal same-Check selection after explicit
  reset plus separate resume/retirement gates. Public typed RPC accepts operator
  reset/inspection and rejects Assignment-route impersonation. Documented CLI
  commands parse through the operator-guide test. Upgrade tests verify revision-15
  rejection without writes, actual budget/checkpoint preservation and no fabricated
  audit, plus atomic revision-16 stamp-failure rollback.
- Final root gates passed: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
  (**545 passed, 0 failed, 1 existing ignored**), and `git diff --check`.
  Initial fixture/schema/format/lint failures were corrected before the final run.
  No shipped skills changed, so skill-only gates were not applicable; the existing
  ignored test was not run. Local verification is not release readiness.
- Implementation and coding-standards self-review: no unresolved findings.
- Blockers: none for T03. Recovery/salvage remains T04–T05; no live installation,
  activation, state migration/reset, profile registration/selection, Check repinning,
  #37 recovery/dispatch, branch reset, merge or force-push was performed.

## Routing hints

Existing work-state/progress inspection, dispatcher reset-remediation precedent, app progress contracts, store task-progress state, RPC dispatcher operations, CLI human/JSON rendering, dispatch/recovery documentation.

## Boundaries

No implicit hold/resource cleanup, live reset on #37, configurable ceilings, or automatic semantic-progress claims. Preserve the Spec's separation of delivery and operations.
