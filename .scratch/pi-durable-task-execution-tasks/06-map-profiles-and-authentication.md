# T06 — Resolve explicit durable profiles and provider authentication

## Delivered outcome

Operators can preview/register a durable-compatible profile and receive deterministic preflight results for supported configuration and authentication, without ambient Pi discovery.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC3, AC17, AC18. Build on T05 and the supported matrix established in T01. Route through existing Agent Profile preview/registration, app policy, server configuration and runner agent/environment construction.

Map model, thinking, tools, behavioral instructions, approved skills/resources and execution directory explicitly. Define repository instruction loading and forbid ambient Pi extensions, skills, prompts, settings and credentials. Reject unsupported legacy extension/MCP/resource combinations rather than assuming compatibility. Missing new-launch inputs fail before capability issuance. Existing immutable revisions remain immutable, and source loss cannot block historical cleanup.

Supply model-provider credentials through deliberately configured host mechanisms separately from Assignment Capability Delivery. Avoid secrets in protocol diagnostics, arguments, environment values used for capability secrets, tool results and persisted transcripts. Keep the actual host authentication implementation consistent with T01's tested matrix.

## Acceptance checks

- Supported profiles produce the intended resolved model/tool/context configuration.
- Unsupported combinations and missing credentials fail closed with actionable, redacted diagnostics.
- Ambient configuration changes cannot alter dispatched behavior.
- Model access and Forge capability authority remain independent.
- Historical inspection/retirement works without profile source files.

## Verification and evidence

Test preview/registration/preflight and runner configuration under isolated homes/environments. Exercise each claimed authentication method and negative case. Scan changed exposure paths for fixture credentials. Deliver a compatibility matrix and focused test results; do not register/select live operator profiles.

## Dependency

Depends on T05. Code Stack predecessor: T05.
