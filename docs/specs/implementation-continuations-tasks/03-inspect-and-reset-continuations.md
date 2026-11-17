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

## Routing hints

Existing work-state/progress inspection, dispatcher reset-remediation precedent, app progress contracts, store task-progress state, RPC dispatcher operations, CLI human/JSON rendering, dispatch/recovery documentation.

## Boundaries

No implicit hold/resource cleanup, live reset on #37, configurable ceilings, or automatic semantic-progress claims. Preserve the Spec's separation of delivery and operations.
