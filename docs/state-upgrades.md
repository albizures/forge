# Revision State upgrades

Forgeup can explicitly carry a database from one registered Forge Revision into
another. It preserves the source database and upgrades a consistent copy. Ordinary
repository opening still rejects incompatible schemas before initialization writes;
it never migrates, resets, or rewrites an older store's marker.

Initially supported: schema **7 → 8**, and **8 → 8** transfers. Schema 8 adds an
empty `task_stacks` table; existing Tasks remain non-stacked. Earlier, newer,
foreign, corrupt, and structurally inconsistent stores are rejected. A same-schema
application update still needs state transfer because Revision State is isolated.

## Preconditions

- Both revisions must be registered, distinct, and have valid immutable artifacts.
- The target must implement offline upgrade interface 1. A source need not implement
  that interface, but its schema and ownership evidence must be supported.
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
stack decoding, execution blockers, and exact preservation inventories of every
pre-existing table. Invented stack membership is forbidden.

The inventory hashes each typed stored value, independently of row order and page
layout, and includes row counts and schema definitions. It is not a substitute for
migration-specific domain tests. Future migrations that intentionally transform
existing tables need explicit preservation assertions for those transformations.

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
artifacts. Tests cover schema 7 → 8, same-schema transfer, real Record/comment/history
preservation, managed startup rejection, and rollback acknowledgement. Rust tests
add transaction failure injection, WAL snapshots, structural incompatibility,
concurrent ownership, interrupted journal phases, stale source acceptance,
non-pristine destinations, changed publication, symlink boundaries, and retries.
These local tests are not production migration sign-off or cross-platform crash/
power-loss certification. Apply the deployment owner's backup and recovery policy.
