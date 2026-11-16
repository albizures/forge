# Permit explicit operator-forced completion without rewriting assessment evidence

Obsolete Records can remain unclosable after replacement work is complete because their original Checks, parent execution gates, or legacy plans no longer apply. Retrofitting historical execution plans or inventing passing assessments would distort evidence; a separate terminal status would expand the lifecycle and its scheduling semantics.

Retain Done and permit an explicit operator-only completion override with a required Completion Resolution. It can close Open or In Progress Records despite unmet workflow/completion requirements, but cannot bypass active execution, unresolved ownership, open Manual attempts, or unsafe retained resources. Preserve all Checks, Assessments, Findings, configuration pins, and publication evidence. Persist the forced nature, operator attribution, resolution, and bypassed gates atomically with closure.

Forced Done satisfies ordinary dependency gates exactly as Done does today. Normal reconciliation may dispatch newly eligible dependents; the override neither pauses dispatch nor supplies missing Stack or publication evidence. Closing a parent does not close its children. Assignment Capabilities do not grant the override. Dependency-consequence response warnings and the full operator runbook are a separate follow-up slice.

The trade-off is that Done is a lifecycle closure decision, not by itself proof that requirements passed. Structured completion history and inspection make operator overrides visible. Ordinary completion remains fully gated. This is an explicit administrative exception to ordinary lifecycle and parent-completion rules, not a migration of historical plans or authorization to falsify Assessments.

## Persistence and safety boundary

An additive `forced_completions` row links to the exact status-transition Record Update. Its operator authority label, supplied Completion Resolution, and observed workflow denials commit in the same immediate SQLite transaction as status and the Update. Historical normal completion has no such row. Already-Done force requests are non-mutating, including requests with different or absent resolutions.

The existing trusted, unauthenticated operator API attributes these requests to `operator`; this is audit attribution under that authority boundary, not an authenticated personal identity. Capability request shapes remain strict and cannot select this override.

The transaction reuses Execution Mode handoff's authoritative Assignment/resource ownership guard for every Record type, independently rejects open Manual attempts and reserving/cleanup-required Worktree Claims, and never clears Dispatch Holds or changes Execution Mode. Safe Dispatched work is not rejected solely because ordinary Task completion requires Manual mode. Active historical Worktree Claims remain retained: closure is not removal, and removal's independent local Git safety assessment remains mandatory before any later destructive operation. Missing publication and unpublished repository results are bypassable workflow conditions, not permission to delete them.
