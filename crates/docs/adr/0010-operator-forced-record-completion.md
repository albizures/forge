# Permit explicit operator-forced completion without rewriting assessment evidence

Obsolete Records can remain unclosable after replacement work is complete because their original Checks, parent execution gates, or legacy plans no longer apply. Retrofitting historical execution plans or inventing passing assessments would distort evidence; a separate terminal status would expand the lifecycle and its scheduling semantics.

Retain Done and permit an explicit operator-only completion override with a required Completion Resolution. It can close Open or In Progress Records despite unmet workflow/completion requirements, but cannot bypass active execution, unresolved ownership, open Manual attempts, or unsafe retained resources. Preserve all Checks, Assessments, Findings, configuration pins, and publication evidence. Persist the forced nature, operator attribution, resolution, and bypassed gates atomically with closure.

Forced Done satisfies ordinary dependency gates exactly as Done does today. Warn operators that normal reconciliation may dispatch newly eligible dependents; the override neither pauses dispatch nor supplies missing Stack or publication evidence. Closing a parent does not close its children. Assignment Capabilities do not grant the override.

The trade-off is that Done is a lifecycle closure decision, not by itself proof that requirements passed. Structured completion history and inspection must make operator overrides visible. Ordinary completion remains fully gated. This is an explicit administrative exception to ordinary lifecycle and parent-completion rules, not a migration of historical plans or authorization to falsify Assessments.

## Out of Scope

- Additional lifecycle statuses or a separate supersession feature.
- Reopening Records, retrospective force-labeling, or editing existing immutable completion metadata.
- Bulk or cascading closure.
- Legacy Stack migration or rewriting dependencies to ordering-only.
- Passing, waiving, deleting, or otherwise rewriting Checks, Findings, Assessments, profile pins, or publication proof.
- Killing agents, recovering Assignments, deleting resources/worktrees, or clearing safety state through the force operation.
- New operator authentication infrastructure or dispatch pause/confirmation workflows.
- Executing this operation on motivating live Records #1/#4/#5 as part of feature implementation, testing, or Spec publication.
- Task creation, dispatch, PR orchestration, installation, or release activation during Spec definition.

## Notes

- The human explicitly confirmed the complete settled grilling design and requested Spec creation only.
- Motivation comes from the conversation about old Spec #1 and Tasks #4/#5, replaced by Spec #6 and completed Tasks #7/#8. These are contextual examples, not source Records whose bodies generated this definition; conversation provenance has no Record ID.
- Existing dependency semantics and Spec start gating are documented by runtime ADRs 0003 and 0009. This feature preserves those normal paths while allowing explicit safe closure without execution.
- The safety requirement concerns unsafe retained resources and unresolved ownership; clean retained historical checkouts are not automatically destructive-cleanup targets. Use the existing authoritative safety assessment rather than treating Done as permission to remove worktrees.
