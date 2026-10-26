# ADR-007: Deliver Pi Assignment Capability configuration through launch environment

- **Status:** Accepted
- **Date:** 2026-10-25
- **Decision scope:** Forge dispatch and Pi subscription extension integration

## Context

A Pi extension needs the owning Agent Assignment Identifier, Forge endpoint, and Assignment Capability to manage Record Update Subscriptions. Supplying the capability in a prompt or environment value exposes a bearer secret to transcript or process-environment surfaces; requiring a model-facing setup tool couples each session to a manual bootstrap step.

## Decision

Forge dispatch will launch Pi with environment variables carrying the Assignment Identifier, capability endpoint, and absolute owner-only capability-secret-file path. The Pi extension reads the capability value from that file at runtime. Forge never writes the capability value into a prompt, a tool argument or result, or an environment variable.

## Consequences

- The dispatch adapter and Pi extension share an explicit, machine-readable integration contract.
- The extension has no usable subscription-management tools outside an Assignment Capability Delivery context.
- Capability-file ownership, permissions, and cleanup remain Forge responsibilities.
- Changing the variable names or delivery method requires coordinated Forge and extension compatibility work.
