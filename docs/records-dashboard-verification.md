# Workspace Records dashboard: initial slice (Task #10)

## Behavior and boundaries

- `/records` uses an explicitly submitted `workspace` query parameter. With no
  selection it shows a selector, not a cwd-derived or aggregate inventory.
  `/records?workspace=<id>` is reproducible; normal GET navigation and Refresh
  retain the selected Workspace. `/` remains the full daemon status page and
  `/status` remains its HTMX fragment. Both pages link to each other.
- Cards include every Record Type and status, scoped through the application.
  They display identifier, first-line plain-text title, type, status, Project
  Scope (or **Projectless**) and latest Update date in UTC. Standard leading
  ATX heading markers (`#` through `######`, followed by whitespace or end of
  line) are removed. Askama escapes titles and metadata; Body HTML is not rendered.
- Creation dates project the opening (`created`) Update, not the earliest
  unrelated change. Updated dates project the greatest durable Update timestamp,
  including added/deleted Comments and Relationship changes. No schema, persisted
  title, RPC, CLI, listener exposure, or mutation behavior changes.
- Recently updated is descending by timestamp, then ascending by Record
  Identifier. Unknown dates sort last. Missing historical dates remain `None`;
  visible unknown-date labels never substitute the current time.
- Workspace enumeration, scoped inventory, and projection failures propagate to
  an explicit error response, not a successful empty list. Unknown Workspace IDs
  return 404. Registered-empty, no-selection, no-Workspaces, and failure responses
  are distinct.

## Acceptance evidence

| Spec criteria | Evidence |
| --- | --- |
| AC 1–2 | HTTP tests use the complete daemon router, two isolated Workspace registrations, all six types, all statuses, and Projectless/project-scoped Records. Switching the query changes membership; foreign Records never enter the selected inventory. Existing status/browser tests still pass. Before/after application inventories and complete Update Logs are identical after browsing. |
| AC 3, security foundation | HTTP assertions cover required card labels, heading removal, first-line-only titles, script-tag escaping, UTC date text and Projectless labels. No stored title is added. |
| AC 5 foundation | Store tests control opening, Comment-added/deleted and Relationship Update timestamps explicitly; assert opening/latest projections and application ordering. HTTP tests control ties and verify ascending identifiers, missing-date labels and unknown-last ordering without sleeps. Historical absence of an opening Update is not inferred from later changes. |
| AC 7 foundation | Workspace selector is a labeled GET form. Reopening identical Workspace URLs produces identical HTML; switching Workspace changes inventory. No selection shows no cards. |
| AC 11 | HTTP tests distinguish missing Workspace (404), empty Workspace, no Workspaces, and projection storage failure (500). Storage/application tests assert projection failures propagate. |
| AC 12 foundation | Semantic main/nav/section/article/headings and labeled definition lists; explicit label/select/button controls, visible keyboard focus, responsive auto-fit grid with a single-column narrow layout and wrapping text. |

Tests live in `crates/server/src/browser/tests.rs` and
`crates/store/src/record_browsing.rs`. Application projection and ordering are
exercised through real isolated SQLite stores, not a browser-local fake.

## Verification executed

Final runs from the bound Managed Worktree repository root:

- `cargo fmt --check` — passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed.
- `cargo test --workspace` — passed, default parallel execution; 454 passed,
  0 failed, 1 ignored across 67 test binaries/doc-test suites.
- `git diff --check` — passed.
- `cargo test -p forge-server browser` — passed, 3 tests including the retained
  status/browser regression test.
- `cargo test -p forge-store record_browsing` — passed, 2 projection/application
  tests (other suites filtered out).

Initial verification exposed pre-existing profile-fixture drift: two
`ResolvedLaunchPolicy` initializers omitted `thinking_level` (compile failure),
while the Stack integration profile omitted required `thinkingLevel` (parse
failure). Minimal test-only fixture updates unblock the workspace gates. No
profile registration, selection, or live configuration was changed.

## Dependencies and limitations

`serde` derives the Axum query shape using the existing workspace serialization
technology. `time` formats UTC RFC 3339 dates; existing dependencies and the Rust
standard library provide no calendar-date formatter. Out-of-range values retain
an explicit Unix-millisecond representation instead of a fabricated date.

This is the first read-only tracer slice, not the full Spec: no type/status
filters, alternative sorting, pagination, Record details, Markdown rendering,
search, Project filters, browser mutations, polling or authentication. Cards
are semantic noninteractive articles until a later detail-page slice. The initial
inventory is unpaginated and uses per-Record date projections; list and date reads
do not promise a single snapshot across concurrent external mutations.

No interactive browser/phone-viewport or assistive-technology session was run;
responsive/keyboard foundations are implemented in HTML/CSS and inspected, not
claimed as end-to-end visual/accessibility certification. Back/Forward behavior
uses standard GET URLs; no automated real-browser history test was run. Local
checks are not deployment, publication, or release verification.
