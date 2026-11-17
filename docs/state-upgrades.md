# Revision State upgrades

Forgeup can explicitly carry a database from one registered Forge Revision into
another. It preserves the source database and upgrades a consistent copy. Ordinary
repository opening still rejects incompatible schemas before initialization writes;
it never migrates, resets, or rewrites an older store's marker.

Supported transfers target schema **13** from schemas **7** through **13**.
The 7 → 8 step adds an empty `task_stacks` table; the 8 → 9 step adds an empty
`ordering_only_dependencies` table; the 9 → 10 step adds an empty
`task_unchanged_results` table; the 10 → 11 step adds an empty
`forced_completions` table; 11 → 12 adds `builds_on_dependencies`.
The semantic 12 → 13 step converts legacy Stack predecessors to Builds-On and
unambiguous remaining prerequisites to Waits-For. It preserves direct indirect-
ancestor completion constraints rather than dropping them. Existing explicit
ordering-only designations remain Waits-For. Same-Spec ordinary Task dependencies
without ancestry are unresolved intent, not inferred waiting or code consumption.
Upgrades invent no execution evidence, unchanged results, forced completions,
branches, or Check pins. Existing publication evidence and commit pins are preserved. Earlier, newer,
foreign, corrupt, and structurally inconsistent stores are rejected. A same-schema
application update still needs state transfer because Revision State is isolated.

## Preconditions

- Both revisions must be registered, distinct, and have valid immutable artifacts.
- The current target implements offline upgrade interface 2. Updated Forgeup also
  accepts interface-1 targets without decision input and retains interface-1 receipt
  recovery. A source need not implement either interface, but its schema and
  ownership evidence must be supported. Install the matching updated Forgeup before
  preparing schema-13 transfers; older managers cannot interpret interface 2.
- Stop both daemons. Exclusively acquire their existing `.lock` sidecars before
  execution. A live or ambiguous managed runtime record remains a blocker even if
  its database lock is available. Direct SQLite writers and deliberate manager
  bypasses remain outside the advisory-lock boundary.
- Resolve open Assignments, unretired Assignment resources, unsafe worktree claims,
  and unexpired dispatch scan leases. A normally stopped dispatcher may leave its
  scan lease until expiry; wait for it rather than resetting ownership evidence.
- Independently verify that old agents cannot act. `--confirm-agents-stopped` is
  the operator's explicit assertion, not automatic proof from a missing heartbeat
  or daemon shutdown. No provider shutdown or external cleanup is performed.
- The destination must be zero-length or exactly pristine initialized state,
  including seed values. No merge or implicit overwrite is supported. Destination
  SQLite WAL/journal sidecars block publication; checkpoint and close them through
  their owner rather than deleting them.

## Plan, prepare, activate

Replace `old` and `new` with registered selectors or aliases:

```sh
forgeup upgrade plan --from old --to new
forgeup upgrade prepare --from old --to new --confirm-agents-stopped
forgeup upgrade status --to new
forgeup upgrade activate --to new --confirm-agents-stopped
forgeup daemon start
forge status
```

Upgrade commands emit structured JSON on stdout and diagnostics on stderr. A plan
is read-only, reports the schema steps, preservation evidence, and blockers, and
makes no reservation. Unsupported schemas or invalid contracts are errors rather
than eligible plans. Execution rechecks all mutable prerequisites under the global
registry lock and both database ownership locks.

Preparation snapshots committed SQLite state, including WAL data, into private
`state/<target>/upgrades/<id>/source.db`. The target's storage code applies the
complete schema chain in one transaction to a unique `staging-<nonce>.db`. A schema marker advances
only inside that transaction. Validation covers SQLite integrity, foreign keys,
frozen table/index/trigger contracts, Record decoding and Workspace ownership,
Stack decoding, ownership blockers, shared prerequisite cycles, endpoint/scope
eligibility, linear topology, canonical intent, and retained execution evidence.
The three transformed tables (`task_stacks`, `ordering_only_dependencies`,
`builds_on_dependencies`) have conversion-specific assertions, including exact
decoded Stack equivalence. Every other pre-existing table has an exact preservation
inventory. No preparation or fabricated execution evidence is permitted.

Incomplete Open Builds-On plans remain valid during both schema conversion and
same-schema transfer. Missing root or successor execution configuration is not
unresolved relationship intent. Inspection validates their topology and any
stored source configuration without requiring execution completeness. This
exception is limited to Open lineage without prepared commit pins or result
evidence; started work, retained publication/unchanged results, and prepared
chains still require complete configuration and the ordinary evidence checks.
Upgrade never fills missing configuration, and execution authorization remains
responsible for rejecting an incomplete plan.

The inventory hashes each typed stored value, independently of row order and page
layout, and includes row counts and schema definitions. It is not a substitute for
migration-specific domain tests. Schema 13 intentionally normalizes legacy
successor source JSON and predecessor columns; all prepared pins and publication
JSON remain retained. Update history keeps its original dependency terminology.
Offline validation does not certify live hosting: execution authorization still
re-observes required external evidence after activation.

Only a validated, synced staging artifact is atomically published over a pristine
destination. The receipt retains both Revision Identities, executable hashes,
source/destination inventories, migration steps, validation results, and progress.
No runtime process record or log is adopted as live target state. Registered paths,
profiles, IDs, relationships, history, retry budgets, holds, and ownership evidence
are retained; filesystem relocation, worktree repair, and external-resource cleanup
are not part of upgrading.

Preparation never selects or starts a revision. Ordinary `forgeup use new` and
managed startup refuse an unactivated target. Activation checks destination validity
and source freshness under ownership locks, then selects without starting a daemon.
If the source changed, either explicitly archive the unactivated copy and prepare
again, or deliberately accept the older snapshot:

```sh
forgeup upgrade discard --to new
forgeup upgrade prepare --from old --to new --confirm-agents-stopped
```

Alternatively, `forgeup upgrade activate --to new --confirm-agents-stopped
--accept-stale-source` records acceptance that intervening source work is omitted.
This never merges that work. `discard` accepts only an unchanged, unactivated
publication, archives it as `prepared.db`, and resets the destination to empty. It
refuses changed/user-written destinations and activated upgrades.

## Explicit legacy intent decisions

An unresolved conversion fails before publication and reports every affected
`blocked -> prerequisite` Record pair. Source state stays untouched; the target
remains unactivated. Plan reports structural compatibility and ownership blockers;
intent validation runs inside the staging conversion, not by mutating the source.

For an intentional completion-only prerequisite, create a JSON file:

```json
[{"from_id": 123, "to_id": 122}]
```

Then use the public workflow (archive a failed attempt before changing its input):

```sh
forgeup upgrade recover --to new
forgeup upgrade prepare --from old --to new --confirm-agents-stopped \
  --waits-for-decisions decisions.json
```

Only unresolved dependencies may receive decisions. Duplicate, extraneous,
started/completed, prepared, or started-Spec decisions fail closed. For code
inheritance, use the source revision's public Stack configuration workflow before
preparing a new snapshot; its normal lifecycle restrictions still apply. There is
no hand-edited storage repair, migration-created branch, or fabricated Check result.

The parsed decisions are retained in the receipt and copied to a unique private
per-attempt adapter input. Exact preparation retries must supply the same decisions;
editing the operator file cannot change an existing attempt. Changed decisions
require recovery/abandonment and a new preparation. Offline interface 2 advertises
`waits_for_decisions` and accepts an optional migrate input; interface-1 targets
default to no decision capability and never receive that input. This interface
revision is separate from storage schema 13 and the unchanged daemon Management
Protocol revision 1.

## Recovery and retries

The journal is `state/<target>/upgrade.json`. Stages are `preparing`,
`ready-to-publish`, `prepared`, `activating`, `activated`, and `discarding`.

- Exact preparation retries retain the original receipt and snapshot. Conflicting
  source/target artifacts or state fail rather than silently starting over.
- During preparation, the snapshot is published only after its working database
  is closed and validated. An incomplete snapshot reservation requires recovery.
  Each migration attempt uses a new staging pathname, retaining interrupted ones.
  A worker surviving a killed manager can finish only into its own old artifact;
  it cannot overwrite the retry's staging file or publication. The retained
  snapshot is revalidated before migrations run on a fresh copy.
- Before publication, durable intent records the validated target fingerprint.
  After a crash, publication is recognized by the destination matching that
  fingerprint; the migration is not rerun against the destination.
- `forgeup upgrade recover --to new` can recognize completed publication or archive
  an unpublished attempt as `abandoned.json`, retaining snapshot/staging evidence.
  Corrupt or ambiguous data is not treated as successful publication.
- Once activation begins, retry `upgrade activate`. The `activating` journal blocks
  daemon startup until registry selection and receipt progress are reconciled.
- Interrupted `discard` can be completed with `upgrade recover` or `upgrade discard`;
  the validated prepared archive must remain available before resetting state.
- Guards are scanned from retained Revision State, not just installed executables.
  Replacing/removing a revision participating in a retained receipt is refused,
  preventing accidental loss of recovery provenance. Such revisions must remain
  registered in this initial implementation; lineage-retirement administration is
  deliberately not provided.

Snapshots remain until explicit cleanup:

```sh
forgeup upgrade cleanup --to new
```

Cleanup after activation deletes only that receipt's known backup/staging files.
It does not delete the source database, receipt, lineage guards, interrupted
workers' older staging files, or abandoned attempt archives. Those artifacts remain
available for separate operator retention; establish that workers cannot still
write them before any external cleanup.
Do not hand-edit receipts or SQLite schema markers to bypass recovery.

## Rollback and older clients

After activation, restarting the source through current Forgeup requires explicit
rollback/fork acknowledgement:

```sh
forgeup use old --acknowledge-divergence
forgeup daemon start
```

The acknowledgement is durable. The source remains its own database: work performed
on the newer target does not appear there, and restarting both creates independent
histories. There is no reverse migration, automatic reconciliation, or provider
resource rollback.

**Manager compatibility fence:** first preparation advances the Forgeup registry
to schema 2. Older Forgeup executables and older Forge CLIs embedding schema-1-only
managed resolution reject that registry. Otherwise an older embedded manager could
silently bypass upgrade reservations and rollback guards. Install the new Forgeup
before using upgrades; retain it for lifecycle and recovery. Registry schema 1
remains readable without automatic modification until an upgrade is prepared.
Abandoning an attempt does not downgrade the registry.

For an older CLI after acknowledged rollback, start its daemon with the updated
Forgeup, find its endpoint using `forgeup list`, and explicitly supply
`--server-url http://HOST:PORT/rpc`. That override bypasses embedded managed
resolution. Do not interpret its availability as permission to bypass lineage or
agent-safety checks. Rollback compatibility must be rehearsed with the actual old
binaries; unit tests do not certify every historical version.

## Implementation and verification

- Forgeup owns orchestration and filesystem/process boundaries in
  `crates/forgeup/src/upgrade.rs`.
- Shared offline contracts live in `forge-app::storage_upgrade`; this dependency
  keeps manager/target types consistent without duplicating application SQL.
- Storage conversion and validation live in `crates/store/src/upgrade.rs`, with
  frozen schema contracts and version-specific migrations under `src/upgrade/`.
- The target adapter is `forge-server --state-upgrade ...`; it never binds a
  listener. It is an internal manager interface, not a supported standalone manual
  database upgrade command. Callers must enforce daemon ownership separately.
- `sha2` reuses the project's existing hashing dependency for preservation
  inventories. Rusqlite's `backup` feature provides consistent SQLite snapshots;
  copying a live main database file would omit committed WAL data.

Every future incompatible schema change must add a frozen contract, sequential
migration, compatibility advertisement, preservation tests, and failure/recovery
coverage. Never implement a historical migration by calling mutable current-schema
initializers. Storage schema and offline interface versions are separate from the
unchanged daemon Management Protocol revision 1.

Run the normal Rust gates, then build matching release executables and exercise the
real subprocess boundary in a private temporary Forgeup home:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
cargo build --release -p forgeup -p forge-cli -p forge-server
python3 -m unittest discover -s tests -p 'test_state_upgrades.py'
```

The subprocess test defaults to `target/release` (`FORGE_TEST_BIN_DIR` overrides it).
It waits for real persisted dispatch leases to expire rather than resetting them.
Debug binaries can exceed its command timeout due to hashing large unoptimized
artifacts. Tests cover each source schema 7 through 13, same-schema transfer, real
Record/Comment/Update preservation, public unresolved-intent diagnostics and
explicit decision retries, non-activation, managed startup rejection, and rollback
acknowledgement. Store fixtures cover direct/indirect ancestry, explicit waits,
standalone/cross-Project/Projectless waits, retained completed publication and
unchanged results, prepared successors, and forced-completion evidence. Rust tests
add transaction failure injection, WAL snapshots, structural incompatibility,
concurrent ownership, interrupted journal phases, stale source acceptance,
non-pristine destinations, changed publication, symlink boundaries, and retries.
These local tests are not production migration sign-off or cross-platform crash/
power-loss certification. Apply the deployment owner's backup and recovery policy.
