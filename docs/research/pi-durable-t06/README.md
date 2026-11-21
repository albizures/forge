# T06 — explicit durable profiles and host authentication

## Disposition: complete for the controlled pre-dispatch boundary

T06's profile/authentication/configuration component is complete. Operator
preview/registration now preflight explicit host authentication, and the actual
pinned runner configuration is exercised through isolated process, provider and
tool tests. **Production durable dispatch remains disabled.** T07 may proceed
with the Check-bound launch/bootstrap/Outcome tracer; this completion does not
authorize live work, automatic recovery or release.

Final verification: **630 Rust tests passed, 0 failed, 1 pre-existing opt-in probe
ignored; 33 Node tests passed.** The completed increment and current handoff are
at the end of this document. The foundation sections below are a historical
journal; their then-missing work does not override this current disposition.
No live profiles, Forge Records, selections, real credentials or rollout changed.

## Foundation increment (historical)

The first increment implemented profile compatibility, approved context resolution
and immutable backend registration, without authentication or runner construction.
T06 was correctly left unchecked at that stage.

## Delivered

- Strict version-1 `pi-durable` profile authoring through the existing
  preview/registration catalog path. Initial compatibility is exactly
  `openai/gpt-4.1`, thinking `off`, and a unique subset of `read`, `write`, `edit`,
  `bash`. Unsupported models/thinking/tools, extensions and MCP are rejected.
  Both application preview and storage registration reject durable Grilling.
- App-owned `durable_profiles` policy and a new-launch context snapshot. Only
  explicitly approved catalog Markdown resources are loaded, after immutable
  profile behavior. Canonical regular `.md` identities, exact byte digests,
  UTF-8 and a combined 256 KiB context limit are checked. Reads in this loader
  are bounded; existing catalog discovery reads are not newly bounded.
- Snapshot provenance retains selected resource identities/digests without
  injecting names/paths as instruction headings. Repository/ancestor AGENTS,
  supporting skill files and ambient Pi settings are not implicitly included.
  Debug omits instruction contents. A snapshot is context, not authority.
- Registered durable revisions retain provider `pi-durable`, never `herdr`.
  Registration preserves existing atomic registration/selection semantics and
  immutable historical revisions; it does not migrate old Check pins. Lifecycle
  copies retain backend/policy. Existing factory readiness still rejects durable
  launches, and Herdr's flags-only launcher explicitly rejects durable policy.
- Historical profile reads do not validate mutable source inputs. New-launch
  source checks remain separate; no inspection/retirement path was changed.
  No schema, wire field, dependency, capability or shipped skill change.

## Verification

From the repository root:

```sh
cargo test -p forge-app --test durable_profiles
cargo test -p forge-store --test workspace_dispatch durable_registration
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Final repository gates pass: **623 Rust tests passed, 0 failed, 1 pre-existing
opt-in probe ignored**. Five new tests cover the compatibility matrix/redacted
errors; approved-only context despite ambient changes and removed profile source;
resource drift/size/loss and retained provenance; persisted-policy bypass attempts;
backend/history/lifecycle-copy persistence across store reopen and Grilling
rejection; and explicit Herdr refusal. The compatibility/context/bypass cases are
three tests, backend/history is one, and Herdr refusal is one.

No runner source changed. Standalone Node suite/build, clean npm installation,
real-provider authentication, installed components and live dispatch were not
run. The default Rust suite includes the existing composed Node/host authorization
process tests; this is not new profile-to-runner/authentication evidence.

## Implementation self-review and remaining work

Task: “Map model, thinking, tools, behavioral instructions, approved skills/resources
and execution directory explicitly.” Compatibility and approved context are
implemented, but actual guarded tool/provider construction and execution-directory
mapping into the runner are **not implemented** in this increment.

Task: “Supply model-provider credentials through deliberately configured host
mechanisms separately from Assignment Capability Delivery” and “Missing new-launch
inputs fail before capability issuance.” Host credential configuration, positive
and negative authentication preflight, protected runtime injection and exposure
scans remain **missing**. Profile preview/registration intentionally do not claim
to validate credentials. No capability issuance path was enabled.

Task: “Historical inspection/retirement works without profile source files.”
Immutable profile reads remain source-independent and tested. Actual durable
Assignment retirement integration belongs to the later execution/retirement
increments and is not established by this profile test.

Summary: T06 is partial; authentication and real guarded runner mapping are the
blocking omissions. No scope creep or incorrect implemented requirement found.

Standards self-review: no introduced violation found. App owns compatibility and
context policy; the store enforces type/backend persistence transactionally;
filesystem reads precede the registration transaction. No history is rewritten,
uncertainty cleared, secret introduced or dispatch enabled. This is not the
independent T18 review.

## Successor handoff within T06

1. Add deliberately configured host API-key authentication consistent with T01;
   deny ambient Pi credentials, OAuth and unproved mechanisms. Keep model keys
   separate from capability files, arguments, environment values and transcripts.
2. Construct the supported pinned runner model/tool/context/cwd/env configuration
   explicitly; apply the existing authorization guards to every provider/tool and
   keep retries/compaction/nested extensions disabled as required by T01.
3. Exercise actual preflight/configuration with isolated homes and credential
   fixtures, scan exposure paths, and verify missing credentials fail before new
   capability issuance at the T07 launch boundary. Do not mistake profile
   registration or this context snapshot for launch permission.
4. Renew repository and runner gates, complete T06 evidence, then advance to T07.

## Completed authentication and runner-construction increment

### Delivered interfaces

- App owns `AgentProfile::durable_execution_configuration`, including compatible
  Task/backend/model identity, and the typed credential-free resolved configuration.
  A shared Rust/Node `runner/test/profile-v1.json` fixture checks the serialized
  boundary. Explicit cwd must be canonical and present; T07 must additionally
  bind that path to the actual Assignment worktree.
- `forge_runner_host::model_auth::preflight` validates the deliberate raw-key-file
  mechanism using the existing no-follow/nonblocking protected-file reader. It
  neither reads Pi settings nor creates files. Every auth failure is a fixed
  redacted diagnostic. There is no new dependency or secret-bearing record.
- Manual server configuration explicitly accepts the **path** environment setting
  `FORGE_DURABLE_OPENAI_KEY_FILE`; `DaemonConfig::durable_model_key_file` and the
  daemon builder are the library equivalents. Managed process inputs do not
  inherit this setting. Missing credentials do not prevent daemon startup or
  historical reads/cleanup; they block new durable operator preview/registration.
  Configuration Debug output omits the selected credential path.
- The existing operator preview/register handlers check local authentication for
  durable profiles only. Registration persists the same resolved policy that was
  preflighted, without a second mutable document resolution. Repository source
  validation remains outside the atomic registration/selection transaction.
  Herdr authoring does not acquire a new authentication prerequisite.
- `forge_server::durable_profiles::prepare` composes app configuration with
  explicit host auth preflight. It receives no capability/repository mutation
  port and creates no authority or runtime files. The production launch boundary
  remains disabled; T07 must call it before issuing capabilities.
- `runner/src/profiles.ts::createProfileRuntime` validates configuration/context
  and rechecks exact approved Markdown bytes and key-file protection. It builds
  an explicit pinned OpenAI provider, in-memory allowlisted auth context, four
  guarded coding tools, bound cwd, `/bin/sh` and restricted shell environment.
  It opens no storage, conversation or scheduler. The configured key exists only
  in an auth closure, not child env, arguments, profile data or prompts.
- Every admitted provider/tool crosses the existing lease guard at actual dispatch
  or execute. Tools are explicitly `unsafe` for replay. Unsupported models,
  OAuth, custom extensions, MCP, image/classifier/deferred requests, inherited
  Pi auth/settings, provider/SDK retries and automatic compaction are denied or
  disabled. Exact key echoes in provider events/errors are redacted before they
  reach Harness storage. This is not arbitrary secret discovery or hostile-agent
  containment; trusted same-user tools can still read host files deliberately.
- Capability environment injection allows only the existing four metadata/path
  names. Raw capability/model secrets are not env values; neither the credential
  file path nor model access substitutes for a Forge capability or execution
  lease. A supplied matched CLI path contributes its directory to explicit PATH.
- The local component digest now covers `profiles.ts`. No runner wire-v1 envelope,
  runtime schema, Forge storage schema or public RPC response shape changed.
  The additive Rust daemon configuration field and manual help are documented;
  matched installed-component support remains T15, not a claim of this increment.

### Supported initial compatibility and authentication matrix

| Input | Supported behavior / evidence |
| --- | --- |
| Runtime and role | Explicit registered `pi-durable` v1 Task only; Grilling and implicit Herdr conversion reject. |
| Model / thinking | `openai/gpt-4.1`, thinking off only; actual dispatch rejects other catalog models. |
| Tools | Unique subset of read/write/edit/bash; exact registrations guarded and unsafe, including direct/nested entry semantics from the existing T05 guard. |
| Context | Immutable profile behavior plus explicit Markdown snapshot/provenance, combined 256 KiB, hash/UTF-8/canonical validation; no AGENTS/Pi discovery. |
| Host auth | Explicit canonical raw API-key file, owner-owned single-link regular 0600 file under canonical owner-owned 0700 parent. One printable ASCII token of 1–4096 bytes, optional final LF. |
| Auth injection | OpenAI-only in-memory auth-context resolution; empty in-memory credential store, no OAuth/store discovery. Key is not shell env or authority. |
| Unsupported auth | Ambient OPENAI_API_KEY/Pi auth files, OAuth/ADC/AWS profiles, copying configuration trees and other providers reject or cannot supply auth. |
| Mutable source/key loss | Blocks new authoring/preflight/configuration; retained profile reads and original backend cleanup do not invoke these gates. |
| Security / readiness | Trusted local Linux host only. Local configured auth is not proof of provider permissions. Real-provider and installed-release readiness remain T19/T15. |

### Verification and observed results

From the repository root:

```sh
(cd runner && npm ci --ignore-scripts --no-audit --no-fund)
(cd runner && npm run prepare-reader && npm run check && npm test)
cargo build -p forge-runner-host
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Final checks pass: **630 Rust tests passed, 0 failed, 1 pre-existing opt-in probe
ignored; 33 Node tests passed**. This includes seven new Rust tests since the
foundation (one app wire/configuration test and six server tests), and six new
Node tests. Default-parallel workspace verification also reruns the pre-existing
composed lease/store/supervisor/Node outage tests. Clean npm installation succeeds
with scripts disabled; pinned dependencies and the derived reader are unchanged.

New evidence covers:

1. Operator preview/registration rejects absent auth before creating a revision,
   then accepts the explicitly configured file with redacted responses; later
   source/key removal leaves immutable reads available and creates no capability.
2. Fixed diagnostics for malformed/oversized/non-ASCII/empty/missing keys,
   restrictive parent/file permissions, symlinks and hardlinks. Profile backend,
   Record Type, model and direct-source mismatches reject before runtime creation.
3. Actual server-prepared serialized input reaches an isolated Node child and
   resolves model auth, while zero grants still prohibit provider execution.
4. Actual coding tools read/write/edit/bash in the configured worktree; shell
   output contains selected capability metadata/path but no model key. All four
   tools deny effects at exact lease expiry, and unapproved env keys reject.
5. Actual pinned OpenAI SDK request construction uses the explicit Authorization
   header. Intercepted 429 errors make exactly one request despite caller retry
   options; a controlled SSE success decodes correctly. Other models and deferred,
   image/classifier paths reject without a request. No external provider called.
6. Actual pinned Harness persists a redacted echoed-key failure; SQLite/WAL/SHM
   files, payloads, responses and changed diagnostics are scanned for synthetic
   canaries. Approved context reaches the request; ambient AGENTS does not.
7. Isolated HOME/Pi credential/settings/ambient-key canaries cannot supply missing
   model auth or alter explicit runner choices. Direct model access and capability
   metadata remain independent of a valid Forge execution lease.

Initial compile/lint runs caught an event-stream type-only export, documentation
backticks and fixture formatting/style issues. Runtime tests caught passing stored
AgentState names where Harness expects AgentChange registrations; the factory now
returns the correct typed AgentChange. The first success SSE fixture omitted the
text delta events; the fixture was corrected and the full suite rerun. These
failures are not counted as passing evidence. No real-provider request, installed
Revision, live dispatch, authenticated bootstrap or accepted Outcome was exercised.

### Completion self-review and T07 handoff

Implementation review against T06: supported profiles produce explicit tested
model/tool/context/cwd/env construction; unsupported inputs and missing local auth
fail closed with redacted results; ambient Pi inputs cannot supply auth or change
behavior; model access does not supply a Forge capability/lease; historical reads
and existing cleanup routes remain source-independent. No scope creep or incorrect
implemented requirement found. **T06 is complete at the plan's pre-dispatch
boundary; no T06 blocker remains.** Real launch, capability-ordering integration
and authenticated bootstrap/Outcome evidence belong to T07, not this component
completion. Summary: no outstanding T06 findings; production/release work remains
explicitly gated.

Coding-standards self-review: no introduced violation found. App owns policy and
resolved configuration; host owns credential-file I/O; server composes these and
operator routes. Registration keeps FS/auth work outside transactions and commits
immutable policy/selection atomically. No secrets are added to DB/wire/env values,
no historical binding is rewritten, and no cleanup or capacity fact is inferred.
This is not independent T18 review.

T07 must bind the exact Check/profile/worktree/configuration, use the matched
protected host setup, invoke preflight before new capability issuance and recheck
mutable inputs at launch. Behavioral instructions must not enter the agent's
execution context before successful authenticated bootstrap. Reuse the T05 grant
service and T06 factory without exposing control/auth injection as agent APIs.
Do not reinterpret module construction, local key configuration or profile
registration as launch permission, accepted Outcome or production readiness.
