# T01 — pinned pi-durable implementation contract

## Disposition: research/probing complete; T02 may proceed

The stock pi-durable 1.5.2 SQLite opener cannot provide physical read-only
observation: it attempts initialization writes. The follow-up investigation
resolved that gap with an **explicit, pinned, derived read adapter**, without
changing the installed upstream implementation or weakening exclusive ownership.
The adapter validates an existing schema, never initializes/migrates, refuses
commit/ID allocation and uses a physically read-only SQLite connection. Its
probes pass beside a writer and after SIGKILL with interrupted unsafe intent.

Other findings require Forge-owned boundaries, not weaker policy:

- A throwing `beforeRequest` hook does **not** deny model access. Guard actual
  provider requests and actual tool execution, with expiry cancellation signals.
- Upstream submission IDs deduplicate changed payloads. Forge must compare
  operation payload identity before invoking runtime admission.
- A free OS lock does not prove children stopped. Retain authority/effect/process
  blockers separately, with an independent host supervisor.

This is a concrete feasibility contract for implementation successors, **not a
production runner, accepted Check or release verification**. No production
configuration, profile selections, Forge Records, dispatch or publication changed.
No real-provider requests or credentials were used. The parent Spec is unchanged.

## Exact identities and reproduction

- Forge source inspected: `487069266b80efd36e686abba74f7aded97d78e9`.
- Selected upstream package: `@earendil-works/pi-durable@1.5.2`; direct `pi-ai`,
  `chord`, `pi-schema` dependencies also exactly `1.5.2`. Complete transitive
  identities/integrities are in [`package-lock.json`](package-lock.json).
- Source tag `v1.5.2` resolves to
  `b4afdddf44e059c67ccf5097e5317d913521b470` ([tree][source]). Probes execute
  npm's published `dist`, not a source checkout. The API is experimental
  ([upstream README][upstream-readme]).
- Registry tarball:
  <https://registry.npmjs.org/@earendil-works/pi-durable/-/pi-durable-1.5.2.tgz>.
  Registry integrity:
  `sha512-zDB3anbI/yD0IG+VVY+g5tU+c5XOVqQAFwLkXRMiqZb4p/afWro0voZtLNoRAWcDA2r73kvmH6kKin9Ka8l55g==`.
- Package requires Node `>=22.19.0` ([manifest][manifest]). Selected/tested host:
  **Node 24.18.0**, npm **11.16.0**, **Linux x64**, bundled SQLite **3.53.1**.
  Other platforms/Node versions are unsupported by this initial contract.
- Derived read implementation base SHA-256:
  `0d9d8cf56898afe5d38f0e56bd3cbcad23a8e68b942b81c39587ac731fe48642`.
  Generated module SHA-256:
  `baa010be34b278b0f439df410d243ddeef3fd4ff5296ced6bcee8e7a94c7caa1`.

From this directory, on the selected host with Python 3 for OS probes:

```sh
npm ci --ignore-scripts --no-audit --no-fund
npm test
python3 ownership_probe.py
```

`prepare-readonly.mjs` verifies the entire upstream module's base hash and
unique source replacements, then atomically writes a **new sibling derived
module** under the research fixture's ignored `node_modules`. The stock module
is untouched. This generated adapter is deliberately not a transparent patch,
SQL-filtering workaround, private-constructor trick or production installation.
T04 must incorporate an explicit maintained adapter (including source/license
provenance), and T15 must attest its exact content with the upstream package and
lockfile. Upgrades must renew these probes; do not rely on stable internal APIs.

Dependencies were installed with scripts disabled. Tests inject provider auth
and synthetic responses; they do not use Pi configuration or real credentials.
Runtime stores/mutation markers use isolated temporary directories. Node tests
have a 20-second timeout; host-process synchronization uses bounded readiness
channels, not sleeps. Python is a research test dependency, not a runner dependency.

Source identity commands used:

```sh
npm view @earendil-works/pi-durable@1.5.2 version engines dependencies dist --json
git ls-remote --tags https://github.com/earendil-works/pi.git '*1.5.2*'
git clone --depth 1 --branch v1.5.2 https://github.com/earendil-works/pi.git /tmp/forge-durable-t01/upstream
```

## Observed evidence

Final Node result: **19 tests, 19 pass, 0 fail**.

| Retained probe | Observed result |
| --- | --- |
| Explicit OpenAI auth resolution | Injected synthetic API key resolves; Assignment capability path alone supplies no model auth. No network request. |
| Catalog/profile | `openai/gpt-4.1` exists in the pinned catalog, no reasoning; explicit instructions reach the prompt. Unapproved worktree `AGENTS.md` canary is not discovered automatically. |
| Coding tools | Explicitly registered `write`, `read`, `edit`, `bash` execute in the supplied cwd; shell uses explicit `/bin/sh` and restricted test environment. File bytes and tool results checked. |
| Hook denial | Throwing generation hook still sends one request: hooks alone fail the authorization requirement. |
| Provider boundary | Guard denies stream/simple/deferred-fetch/image/classifier paths after expiry; cancellation remains allowed. Generation and manual compaction cross the guard. |
| Expiry | Fake-clock expiry aborts active synthetic generation, prevents tool execute after expiry during preflight, cancels a cooperative tool child and denies later generation/reopen work. |
| Admission dedupe | Same request ID with changed text returns original submission. Forge conflict checking is required. |
| Stock physical read-only open | Fails with `attempt to write a readonly database`, SQLite error 8; preserved as a characterization assertion, not hidden. |
| Logical observer comparison | Harness rejects submit/resume and sees writer commits, but storage still opens writable. This comparison is **not** the selected production mechanism. |
| Derived read adapter | SQL audit sees SELECT-only reads; commit/mint/submit/resume reject; newest-first pagination remains stable across new writer commits. A fresh page sees the new entries. |
| Incompatible read storage | Missing, empty, older and newer schemas reject; no file creation or source-file byte changes. |
| Interrupted unsafe effect | SIGKILL occurs after one mutation marker but before tool result. Reopen/inspect does not execute tools. Resuming upstream returns an interrupted result without rerunning the tool; marker remains single. |
| Read-only crash inspection | Derived observer reads interrupted intent without changing persisted task records or executing/recovering it. |
| Clock | Linux boottime parser rounds up conservatively, rejects malformed/unavailable data, and reads this host's `/proc/uptime`. |

Final `python3 ownership_probe.py`: **4 tests, OK**:

1. Race two contenders: exactly one acquires `flock` before writable SQLite open;
   exactly one admission row; reuse preserves the same lock inode despite stale
   PID text.
2. Release owner descriptor with a child still alive: another contender can get
   the lock. This negative result proves lock freedom is not cleanup evidence.
3. External supervisor terminates a CPU-stalled process and its controlled child
   group independently of the runner's event loop; exit and pipe closure observed.
4. `O_NOFOLLOW` rejects a symlink lock without modifying its target.

The original observation test failed before Harness creation. Source trace:
SQLite storage open → schema migration → unconditional `INSERT OR IGNORE` in
Chord's migration initializer ([SQLite open][sqlite-open], [storage][sqlite-storage],
[Chord migrations][chord-migrations]). The derived adapter removes the migration
path rather than ignoring its errors. It reuses pinned upstream read/decoding
logic and validates exact schema version 2 before reading metadata.

These are deterministic/local-process mechanism probes. The unsafe mutation is
a local file, not a hosting push/publication endpoint. Resuming upstream after
an interrupted tool permits further model continuation: **Forge must instead
inspect and persist uncertainty before enabling scheduling**. Persistence is not
exactly-once execution, and an abort signal is not process-death evidence.

## Selected implementation seams

| Area | Inspected API/host seam | Successor implementation obligation |
| --- | --- | --- |
| Writable open/start | `Harness.open` reconciles interrupted tasks via `openTasks`; `resume`, submit and waits start scheduling ([Harness][harness]). | Validate authority/identity/effects and acquire ownership before writable open, not only before `resume`. No waits during inspection. |
| Read-only observation | Derived adapter over Node `mode: 'read-only'`, exact-schema validation; `Harness` also has `readOnly: true`. | Own-only path validation; no source migrations, recovery or scheduler admission. Preserve the verified decoder/pagination behavior. |
| Submission identity | `requestId` lookup rejects differing type but not changed content ([submissions][submissions]). | Transactionally persist operation ID, canonical payload fingerprint and original accepted result. Exact retries return original; conflicts reject before runtime call. Unknown replies remain unknown. |
| Unsafe replay | Tool intent commits arguments/replay before execute; resumed replay requires both recorded and current policies to be safe ([tool][tool]). | Initially mark all admitted tools unsafe. Inspect nonterminal unsafe intent before writable recovery; persist independent uncertainty before effects. Resolution never reruns the command. |
| Generation/retry | Hooks catch/report exceptions and continue ([scheduler][scheduler]); actual requests go through Models/provider ([generation][generation]). | Guard provider dispatch after auth resolution, plus actual execute entry of every tool. Combine runtime and lease signals. Initially disable SDK and harness retries, deferred requests, OAuth refresh and custom network-capable extensions. Later support needs renewed probes. |
| Compaction | Uses `models.completeSimple`, bypassing generation's `beforeRequest` ([compaction][compaction]). | Same provider gate; tested with manual compaction. Begin with automatic compaction disabled; re-enable only with focused threshold/retry tests. |
| Nested/code execution | Tools can invoke nested calls; code mode exposes classifier/image access ([tool][tool], [code-mode models][code-models]). | Initial registered coding tools do not need arbitrary nested/custom code. Reject code mode, subagents, custom task kinds/extensions and deferred work. Any future admitted nested tool must also be wrapped at actual execute. |
| Cancellation | Model runtime signal and Chord `withAbortSignal` reach provider/tool; cooperative child cancellation verified. | Independent supervisor handles stalled event loop/daemon outage. Uncertain/escaped processes retain blockers; no abort-as-death inference. |
| Storage compatibility | SQLite migrations end at version 2 and apply automatically on stock writable open ([migrations][migrations]). | Manifest/schema preflight before writable initialization; reject incompatible retained stores without implicit migration. Derived read open does this already. |
| Durability | Node uses WAL, `synchronous=NORMAL` ([Node adapter][node-db]). | Process-crash durability only; do not claim physical power-loss guarantees. |

## Ownership and lease mechanism

- Initially support one trusted local Linux host and a local filesystem with
  kernel `flock` support, not NFS/shared-host storage. Use a persistent lock inode
  opened without symlink following, in an owner-only canonical Assignment runtime
  directory outside the worktree. Acquire exclusive nonblocking ownership before
  opening writable storage. Never unlink/recreate a lock to clear it.
- The **supervision boundary**, not a PID label, holds ownership and retains
  Assignment/incarnation, host boot ID, process start identity and group facts.
  If a runner dies, the supervisor must retain ownership until owned execution is
  reconciled. If the supervisor dies, Forge's independent durable blockers still
  prevent replacement even if the lock becomes available. PIDs/labels never
  authorize adoption or termination by themselves.
- Implement an independent Rust host supervisor/watchdog, rather than relying on
  a JS timer or the daemon remaining alive. The OS signaling mechanism was
  probed with a stalled process. Its production lifetime, retained identity and
  crash behavior are T04/T05/T09 work, with their required failure tests.
- Select **30-second grants, renewal every 10 seconds**; only Forge can renew.
  Bind each grant to Assignment/attempt, profile, freshness, incarnation, host boot
  and sequence. An expired/revoked grant cannot be renewed into restarted work;
  late replies cannot revive it. Restart requires fresh Forge authorization.
- Use shared Linux **CLOCK_BOOTTIME** coordinates, including suspend time. For
  synchronous JS admission checks, `/proc/uptime` exposes this clock truncated to
  centiseconds; round it **up** to the next centisecond, denying slightly early.
  Validate same host boot/time namespace; unavailable/malformed/backward clock
  readings fail closed. The supervisor uses the same clock for its watchdog.
  The kernel implementation is [here][linux-uptime]; parsing/rounding is probed.
- At expiry: deny new provider/tool admissions, abort combined execution signals,
  and independently initiate group termination. Bound TERM grace to 5 seconds,
  then attempt KILL; retain uncertainty until ownership/absence/effects are
  reconciled. Neither elapsed grace nor successful signaling proves cleanup.
  Production renewal/outage/clock-race tests remain in T05, not satisfied by the
  research Lease class, which only models externally supplied grants.

## Protocol and packaging contract

Use a Forge-owned versioned envelope; do not export pi-durable task types into
Rust domain/application interfaces. Initial version is **1**. Required identity:
Forge Revision/component hash, protocol/schema versions, Assignment/Check attempt,
profile/evidence binding, execution incarnation, conversation/store and operation.

- `admit`: stable launch operation ID, immutable payload fingerprint; one reserved
  root conversation/store per Assignment. Reconcile Forge/runtime commits by ID.
- `observe` / `transcript`: read-only queries, stable newest-first cursor tied to
  store/conversation and committed high-watermark; no lease or scheduling grant.
- `recover`: new operation ID for an authorized recovery decision, same Assignment,
  Check attempt and conversation; validates blockers/identity before writable open.
- `follow-up`: immutable recorded input ID as request ID, exact replay/conflict
  semantics, no rebinding of Check/profile/freshness/evidence.
- `retire`: stable revocation/retirement operation identity; authority revocation
  commits before stopping execution. Independent deletion requires safe cleanup.

Persist accepted results and operation history atomically in Forge; never infer
acceptance from final prose or runtime completion. A dropped response leaves an
explicit unknown result until reconciled. No capability/model secrets belong in
payload fingerprints, logs or transcripts.

The matched Forge Revision must include the pinned Node runtime, runner bundle,
protocol version, upstream dependency closure/lock integrity, maintained read
adapter identity, runtime schema identity and component hashes. Locate components
relative to the selected/retained Revision, never arbitrary npm/PATH. Keep the
original compatible cleanup component for retained Assignments across switching.
T15 implements distribution/attestation; these probes do not claim an installed
Revision already supports it.

## Selected initial profile/auth/resource matrix

This is the narrow implementation target, not live-provider release readiness.
T06 preflight must reject everything outside it before capability issuance, with
no implicit Herdr fallback. Expanding it requires explicit new evidence.

| Input | Initial contract / evidence |
| --- | --- |
| Model/thinking | Explicit `openai/gpt-4.1`, thinking `off`; catalog lookup tested. Real-provider validity/permissions remain T19 verification. Faux is probe-only. |
| Model authentication | Explicit allowlisted API-key context, independent of Assignment capability. OpenAI auth resolution positive/negative probes pass. No operator-wide credential store, ambient credential discovery or OAuth. |
| Instructions/context | Explicit immutable profile instructions plus approved Markdown resource contents/identities rendered into `agent.instructions`; explicit `agent.cwd` and execution env. No automatic AGENTS/ancestor/Pi settings discovery. Explicit prompt and non-discovery tested. T06 implements bounded loading/hash validation and provenance. |
| Skills | Approved Markdown-only resource translation, not Pi CLI skill loading or executable extension compatibility. Missing/unsupported resources reject new dispatch; historical cleanup does not reload them. |
| Tools | Explicit `read`, `write`, `edit`, `bash`, wrapped at execute and marked unsafe. Tested in isolated cwd. Shell/environment selection explicit; cwd is **not** a sandbox. Protected capability env names/secret path must still be delivered through existing Forge contract. |
| Unsupported | Other models/thinking/providers, ambient auth, OAuth/ADC/AWS profiles, custom Pi/durable extensions, code mode, image/classifier model access, subagents, custom nested tools/tasks, deferred provider jobs, provider/SDK automatic retries. No silent dropping of unsupported fields. |
| Historic sources missing | Observation/reconciliation/retirement use retained binding/component facts, not mutable profile source. Source validation only gates new dispatch. |

## Local Forge seams inspected

Paths are relative to the repository root. Existing durable support is not assumed.

- [`crates/app/src/lib.rs`](../../../crates/app/src/lib.rs):
  `AgentDispatchAdapter` supplies probe/provision/observe/recover/retire;
  factory selection uses immutable profile provenance. `validate_preflight` is
  separate from historical adapter selection. `AgentAssignmentInfo` reserves
  capacity for unretired resources after closure. Keep lifecycle authority here.
- [`crates/server/src/lib.rs`](../../../crates/server/src/lib.rs):
  factory and `CapabilityDeliveryAdapter` are concretely Herdr-bound. Preflight
  calls `pi_launch_arguments`; provision verifies retained workspace before
  protected capability delivery. T02 separates wiring without deleting Herdr
  guards. Bootstrap resolves matched CLI and delivers a secret path, not value.
- [`crates/app/src/workspace_dispatch.rs`](../../../crates/app/src/workspace_dispatch.rs):
  readiness/provisioning have workspace-binding assumptions beyond the adapter.
- [`crates/app/src/agent_profiles.rs`](../../../crates/app/src/agent_profiles.rs):
  explicit thinking/skills/extensions require compatibility validation distinct
  from historical cleanup prerequisites.
- [`crates/store/src/schema_compatibility.rs`](../../../crates/store/src/schema_compatibility.rs):
  Forge schema revision 19, validation before initialization/stamp. T03 adds
  intentional schema compatibility, not edits to live persistence.
- [`crates/forgeup/src/lib.rs`](../../../crates/forgeup/src/lib.rs):
  Installation/registration/hash validation/attestation currently cover CLI and
  daemon binaries, not the matched JS component closure described above.

## Verification limits and self-review

T01 acceptance: exact upstream/runtime identity retained; each selected safety
mechanism has a source/host seam and bounded probe, or an explicitly unsupported
input; initial resource/auth matrix and packaging/protocol obligations are explicit.
The original physical-read-only blocker is resolved by the selected derived
adapter. Remaining work belongs to the planned production implementation and
verification Tasks; no settled policy has been weakened.

Self-review found no added production scope or introduced coding-standards
violation. This is not independent T18 review. T04/T05 must convert the research
mechanisms into maintained, versioned production code and test their integration;
the research scaffolds are not an authorization service or durable supervisor.

Checks run: isolated npm install and clean `npm ci --ignore-scripts`, repeated
`npm test` (19/19), Python OS probes (4/4), source/registry inspection, repository
`git diff --check` and research whitespace checks. Rust fmt/clippy/workspace tests,
real-provider cycle, packaged Revision tests, controlled hosting-endpoint effects,
full Spec acceptance tests and physical power-loss testing were **not run**.

[source]: https://github.com/earendil-works/pi/tree/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable
[manifest]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/package.json
[upstream-readme]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/README.md
[sqlite-open]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/src/storage/sqlite/node.ts
[sqlite-storage]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/src/storage/sqlite/storage.ts
[chord-migrations]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/chord/src/sqlite/migrations.ts
[node-db]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/chord/src/sqlite/node.ts
[harness]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/src/harness/harness.ts
[submissions]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/src/harness/submissions.ts
[tool]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/src/harness/tool.ts
[scheduler]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/src/harness/scheduler.ts
[generation]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/src/harness/generation.ts
[compaction]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/src/harness/compaction.ts
[code-models]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/src/codemode/models.ts
[migrations]: https://github.com/earendil-works/pi/blob/b4afdddf44e059c67ccf5097e5317d913521b470/packages/durable/src/storage/sqlite/migrations.ts
[linux-uptime]: https://github.com/torvalds/linux/blob/v6.12/fs/proc/uptime.c
