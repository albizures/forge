# Record Comments and Update Log

Forge will keep non-editable, deletable Comments and a Forge-authored Update Log for each Record. Every successful Record mutation, including Record opening, Body edits, lifecycle transitions, Relationship changes, and Comment additions or deletions, commits atomically with its Record Update; failed operations create no update. This preserves a truthful, ordered account of Record changes without storing full before-and-after snapshots.

## Consequences

- Comments are non-blank free text, may be added to Done Records, and may have an optional caller-supplied free-text author. A supplied blank author is invalid.
- Any Forge client may delete a Comment. The deleted content is not retained; its `comment-deleted` Record Update remains.
- Record Updates are authored only by Forge and contain a timestamp, type, and minimal type-specific references (such as a related Record or Comment identifier), rather than prior values or full content.
- Comments and Record Updates have stable numeric identifiers. Identifiers define their order when timestamps tie; list interfaces display newest first.
- Project Scope remains immutable and therefore has no scope-change Record Updates.
