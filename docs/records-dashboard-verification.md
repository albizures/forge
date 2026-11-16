# Workspace Records dashboard (Tasks #10–13)

## Accessibility, recovery and documentation (Task #13)

Built on Task #12's verified Stack commit
`0a9a7fbcb8015582d8e0ccfeaa23291040aa8979`; its explicit dependency is retained.
The sections below record earlier slices, including their then-current limitations.
For current routes and user behavior see [Browser and RPC](development.md#browser-and-rpc).

### Changes and boundary regression coverage

- Cards explicitly become one column below 40rem. Fieldsets may shrink, checkbox
  labels wrap as usable units, and long text cannot force the filter controls off
  screen. Detail navigation wraps; long code blocks scroll internally. Visible
  labels, legends, native controls, textual type/status and focus outlines remain.
- Shared error pages now have viewport metadata, focus styling and labeled recovery
  navigation. Valid-query failures preserve selections/page in **Try again**, and
  provide dashboard return navigation. Out-of-range list pages offer page one with
  filters/sort preserved. Malformed queries offer a clean `/records` entry; they
  are not interpreted partially. Missing Records stay 404, operational/render
  failures stay 500, and successful empty/no-Workspace states remain distinct.
- `browser/accessibility_tests.rs` covers labeled HTML, long titles/Body/Project
  Scope, textual type/status, responsive/focus rules, missing and invalid states,
  and escaped stateful error links. An isolated failed-result seam injects backend
  and rendering failures without process-wide flags or live-store changes. Existing
  isolated SQLite failure tests and read-only/security regressions remain intact.
- README and development guidance now describe actual list/detail/status routes,
  filter/sort/page/return/refresh behavior, durable Update date semantics, and the
  unchanged trusted-client, unauthenticated loopback-by-default boundary.

### Actual browser execution

Firefox 157.0.1, headless Linux, driven through GeckoDriver 0.37.1 against a
source-built Manual Daemon on a private loopback test port and disposable SQLite
store (not the active daemon). The fixture had 55 Generic/Open Records with
500-character unbroken titles, long Body paragraphs/code and raw script text.

- Desktop 1280 CSS-pixel window width (900 outer height): keyboard-only Tab,
  arrows, Space and Enter selected Workspace, Generic Record, Open, Oldest created,
  applied the GET form, reached page two, opened details, returned and refreshed.
  All passed. Three grid columns; no horizontal document overflow. Back/Forward
  restored exact detail/list URLs; reload and Refresh retained page/selections.
  A direct external fixture edit appeared after Refresh. Raw script text did not
  execute. Desktop and narrow screenshots were inspected.
- Firefox clamps ordinary window widths to 500px, so a separate real browser
  iframe viewport measured exactly **390 × 844 CSS pixels** was used for the phone
  layout check (not mobile-device emulation). One grid column and no horizontal
  document overflow on list or long-Body/code details; controls remained visible.
  Tab focus had a computed solid outline. Keyboard detail/return/Refresh retained
  page two and selections. Missing-Record and empty-filter states stayed readable.
- Actual browser no-registered-Workspaces, zero matches, missing Record, invalid
  page, and backend failure presentations were distinct. Keyboard error return
  preserved page two for missing details, or offered page one for invalid paging.
  Dropping only the disposable fixture's Update table produced unavailable list
  and detail pages rather than empty results; this failure check used the 500px
  narrow window. HTML regressions separately cover escaping/recovery on render
  failures and long Project Scope; no screen-reader or physical phone audit ran.

The initial driver startup was retried; early harness attempts exposed native
select/navigation timing and Firefox's width clamp, not product failures. Browser
scripts/screenshots were temporary verification artifacts, not a new frontend or
shipped automation dependency.

### Task #13 verification

`cargo test -p forge-server browser::` passed 14 tests. Required gates passed:
`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace` (466 passed, 0 failed, 1 pre-existing ignored), and
`git diff --check`. An initial Clippy map/unwrap style diagnostic was corrected
before rerunning. Route names and examples were checked against the router/query
parser and the actual browser flow. No dependency, RPC/CLI, schema, listener,
authentication, mutation or live-update expansion was introduced. No push, PR,
installation or integration is part of this Implementation Check.

## Safe Record details (Task #12)

Built on Task #11's verified Stack commit
`add1e8014831dfe6546edbd9b603dd194532a4e6`, retaining its explicit dependency.
Cards now link to `/records/{id}` with the same structured Workspace, repeated
filters, sort and page parameters. Details reconstruct a local `/records` return
URL from those parameters, not from a user-provided destination. Unknown return
parameters are rejected with 400. Direct URLs require explicit Workspace selection;
the application selects a scoped repository before fetching the Record or dates.
Missing/foreign IDs and unknown Workspaces give the same non-disclosing 404;
backend/projection failures give a distinct 500. Detail responses use `no-store`.

Details include full Markdown Body, identifier, type, status, Workspace, empty or
multi-Project Scope, opening/latest Update dates and optional plain-text Completion
Resolution. Missing historical dates remain explicitly unknown. No schema, RPC,
CLI, listener, status-route, mutation, Comment or Relationship navigation changes.

### Markdown dependency and safety (ARCH-4)

`pulldown-cmark` (locked at 0.13.6) provides CommonMark parsing and HTML escaping.
The existing workspace dependencies and standard library do not parse Markdown;
hand-written parsing would enlarge both the correctness and security surface.
Only its `html` feature is enabled; default features/extensions are disabled.
Raw HTML events are converted to escaped text, never trusted HTML. Link and image
URLs are inspected after Markdown entity decoding: control/whitespace/backslash
characters and protocol-relative URLs are rejected; explicit schemes allow only
HTTP, HTTPS and mailto. Relative paths/fragments remain usable. Unsafe destinations
become empty destinations. Renderer-generated text and attributes are escaped.
Only this policy's output crosses Askama's `safe` boundary; all metadata, titles,
return URLs and Completion Resolution retain Askama autoescaping. No general HTML
sanitizer is necessary because user HTML is never admitted to the output markup.

### Acceptance evidence and limits

`crates/server/src/browser/detail/tests.rs` exercises the real daemon router and
isolated SQLite/application boundary:

- AC 8–9: a >60 KB Body through its last line, readable bold/code/links, required
  metadata, controlled opening/latest dates, empty/multiple Projects, optional
  escaped Resolution, script-bearing title, raw scripts/HTML/event handlers,
  entity/control-obfuscated and mixed-case unsafe protocols and unsafe images.
- AC 2, 7, 11: guessed foreign and missing IDs, missing Workspace selection,
  unknown Workspace, backend date failure, full repeated-filter/sort/page return
  URLs, card detail targets, rejected script/external/protocol-relative return
  destinations and invalid metadata-like query inputs. Historical dates stay unknown.
- AC 1: full Record (including Body/status/Relationships/Resolution) and Update Log
  comparisons before/after detail browsing; inherited list/status/RPC regression
  tests remain in the workspace suite. Ordinary GET links preserve browser history.

No interactive browser/script-execution, keyboard, or viewport session was run;
HTTP assertions verify inert markup and URL contracts, not a browser-engine audit.
External safe-protocol Markdown links/images are permitted; this is not an
untrusted-network privacy sandbox. Separate reads are not a cross-request snapshot.

### Task #12 verification

Final gates: `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
(463 passed, 0 failed, 1 ignored; default parallel execution), and
`git diff --check` passed. Targeted `cargo test -p forge-server browser::`
passed all 11 tests. Development runs caught and corrected fixture directory/SQL
binding issues, cross-Workspace error mapping, a fault-injection connection reopen,
and Clippy import placement/style warnings. No ignored test was added. These
are local implementation checks, not publication or installation verification.

## Filtering, sorting and pagination (Task #11)

The Task #11 Stack inherits verified Task #10 at
`7d74a13b569949d680dcd17d2d56e7135a512a73`; its explicit prerequisite remains
unchanged. Browser routes still delegate directly to the application, with no
RPC/CLI/schema/listener/status-route changes or browser mutations.

### Behavior and URL contract

- `workspace` selects exactly one Workspace; omission/empty means no selection.
- Repeated `type` parameters accept `generic`, `spec`, `task`, `wayfinder`,
  `grilling`, `initiative`. Repeated `status` accepts `open`, `in_progress`,
  `done`. Duplicate selections are deduplicated; no selections mean all.
  Values within one filter are ORed, the two filters are ANDed.
- `sort` accepts `recently-updated` (default), `least-recently-updated`,
  `newest-created`, `oldest-created`. Every order uses ascending identifier ties
  and unknown dates last. Cards label/display opening or latest Update date as
  appropriate. Comments and Relationships affect only updated ordering.
- Positive `page` defaults to 1. Complete-Workspace filtering/sorting happens in
  the application before 50-item pagination. Empty matches have one empty page.
  Out-of-range pages return 400 rather than silently clamping; after concurrent
  deletions, reapply the selections to return to page one.
- The labeled checkbox/select form uses GET and deliberately has no page input:
  applying filter/sort/Workspace changes resets page one. Previous/Next and the
  separate Refresh link serialize all selections plus the requested/current page.
  Dashboard responses set `Cache-Control: no-store`, so navigation/Refresh reads
  current application data. There is no dashboard JavaScript, HTMX or polling.
- Unknown keys, repeated scalar parameters, malformed percent escapes, invalid
  decoded text (including replacement characters), unknown enum values and
  invalid/overflow/zero/out-of-range pages produce an explicit 400 diagnostic.
  Missing Workspaces remain 404; operational/projection failures remain 500.
  Errors do not masquerade as empty results. Empty filters/results are distinct
  from no selected Workspace and no registered Workspaces.

Example round-trippable URL:
`/records?workspace=a&type=generic&type=task&status=open&status=in_progress&sort=oldest-created&page=2`.

### Acceptance evidence

Tests in `crates/server/src/browser/browsing_tests.rs` and `date_tests.rs` use
isolated stores and the real application boundary and daemon router. The large
fixture contains 185 selected-Workspace Records across all six types, with 60
Generic Records in each lifecycle status, plus a foreign-Workspace nonmatch.
Lifecycle, Body, Comment and Relationship changes use public application or
repository operations; SQL only controls fixture timestamps or simulates missing
historical Updates. A Generic + Open/In Progress filter has 120 matches and 65
local nonmatches, spanning pages of 50/50/20.

| Spec criteria | Task #11 evidence |
| --- | --- |
| AC 1–3 preserved | Retained status, isolation, escaped titles, Project Scope, read-only and error tests; new HTTP tests assert both date families and unknown dates. |
| AC 4 | Independent exact membership expectations for empty, single (every type and status), multiple and combined filters. Empty type/status selections test clearing each filter independently. All-type selections match unfiltered membership. |
| AC 5 | All four orders tested against independent fixture clock arithmetic, with controlled ties and ascending identifiers. Default matches explicit recently-updated URL. Unknown dates sort last for every order. Individual public Comment add/delete and Relationship add/remove operations move updated orders while preserving opening dates/creation orders. |
| AC 6 | Whole-inventory exact expected order/membership, total counts, page sizes, all-page traversal, no duplicates/omissions, empty matches and invalid pages. HTTP traversal also reaches exactly all 120 matches; GET form omits page so applying changes resets it. |
| AC 7 | Repeated-selection and reserved/Unicode Workspace URL encoding round trips; identical reload/shared URLs reconstruct HTML. HTTP tests assert checked/selected controls, encoded Previous/Next/Refresh targets, and page retention. |
| AC 10 | Public Body edit followed by Refresh URL reveals current title with selections/page retained. Cache policy asserted; no dashboard script/HTMX/polling. |

Browser-history verification seam: every form submission and link is ordinary
full-document GET navigation. No push/replace-state or fragment state exists.
For a real-browser check, apply selections, navigate to page two, change sort,
then use Back twice and Forward: URLs, controls, count and cards should match the
prior views for unchanged data. Reload/copy a page-two URL and compare. Automated
HTTP tests verify the state reconstruction and link/form contracts; no real
interactive-browser history, keyboard, or phone-viewport session was run.

`form_urlencoded` is now a direct server dependency (already transitive). It
provides standards-compliant repeated GET parameter decoding and URL encoding;
the standard library and Axum's scalar query projection do not supply both.
The now-unused direct `serde` dependency is removed from server only.

Limitations retained: complete inventory/date projections are in memory and use
per-Record reads, not a transactionally consistent snapshot across concurrent
external changes. Pagination guarantees no omissions/duplicates for an unchanged
dataset, not a snapshot across mutations. No detail pages, browser mutations,
search, Project filters, activity browsing, live subscriptions or authentication.

### Task #11 verification

Final runs from the bound Managed Worktree repository root:

- `cargo fmt --check` — passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed.
- `cargo test --workspace` — passed under default parallel execution: 460 passed,
  0 failed, 1 ignored across 67 test binaries/doc-test suites.
- `cargo test -p forge-server browser` — passed, 9 tests including retained
  dashboard/status regressions and the new application/HTTP integration coverage.
- `git diff --check` — passed.

An initial combined targeted-test command incorrectly supplied two test-name
filters and was rejected by Cargo before running tests; the corrected browser
command passed. Development iterations also caught test fixture/API mistakes
and a Clippy needless-pass-by-value warning, all corrected before these final
passes. No ignored test was newly introduced. These are local checks, not
publication, release, installation or migration verification.

### Review correction: Relationship operation clocks (Finding #2)

The review observed a parallel browser-test failure: Relationship endpoints can
receive different millisecond timestamps, so the target need not sort first.
`date_tests.rs` now derives exact expected updated orders independently from
public Update Logs, using timestamp direction and ascending identifier ties for
both updated sorts. It asserts that both Relationship endpoints (only the target
for Comments) receive newer Updates, unchanged Records retain their timestamps,
and opening dates/creation ordering remain unchanged. No production behavior or
operation clocks were changed; controlled fixture ties remain covered separately.

Correction verification: `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
(460 passed, 0 failed, 1 ignored; default parallel execution), three consecutive
`cargo test -p forge-server browser` runs (9 passed each), and `git diff --check`
passed. Intermediate Clippy runs flagged test function length; extracting the
Update Log assertion helper and sharing the two order assertions corrected that
before the final gates. No interactive browser session was run.

## Historical initial-slice evidence (Task #10)

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
