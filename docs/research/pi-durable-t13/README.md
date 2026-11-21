# T13 — Explicit effective Check profile migration

Controlled deliverable. Production durable dispatch and automatic recovery remain disabled; no live profile was registered and no live Task migrated.

## Delivered
- Append-only, trigger-protected `check_profile_migrations` / `check_profile_migration_changes` (added to unreleased schema 23 DDL; upgrades create them empty). `record_checks.profile_revision` (original pin) is never updated; the effective revision is the latest change, read by `checks::load`, Execution Mode and Stack validation, hence by dispatch.
- App `check_profile_migration` types and `Application::migrate_check_profiles` (target preflight before the transaction; exact retry bypasses it). Store revalidates scope, durable compatibility, ownership, open attempts and `expected_generation` in one immediate transaction with a single freshness invalidation.
- RPC `checkProfileMigrate`/`checkProfileMigrations`; CLI `forge record check-profile-migrate` and `check-profile-migrations`.
- Glossary (Check, Agent Profile) updated; ADR `0013-explicit-backend-and-check-profile-migration.md` (0013 was next free; spec's 0014 was provisional); operator docs, store README, workspaces table.

## Evidence
- `store/tests/check_profile_migration.rs` (5): unchanged/missing/foreign/stale/Herdr-target reject with no history; original pin preserved, exact replay (even with stale generation) returns original, conflicting reuse rejects, successor claim uses the new pin and the old pin is `check-selection-stale`, active ownership blocks; freshness advances once (2→3) and replay does not; triggers reject update/delete; claim-vs-migration race over 10 iterations never has two winners (the claim side may also fail for unrelated Herdr preflight reasons, so this is a weak race check).
- `cli/tests/check_profile_migration.rs`: actual CLI→daemon→store: stale, first, replay (same timestamp), conflict, unchanged, JSON/human history.
- Upgrade fixtures drop/assert the new empty tables.
- Gates: `cargo fmt --check`, clippy `-D warnings`, `git diff --check` clean; `cargo test --workspace` (real Node 24 on PATH): 855+ passed, one failure from a doc placeholder in the new docs, fixed and the CLI bin tests rerun.

## Not claimed
No controlled end-to-end daemon dispatch of the successor (store-level claim only); no transaction-failure injection beyond SQLite atomicity; stale-pass handling relies on existing `outcomes::invalidate`.
