---
name: wayfinder
description: Chart or work a Forge Wayfinder map with Grilling and Task children, resolving at most one non-research child per session.
disable-model-invocation: true
---

# Wayfinder

Wayfinding finds the route to a **destination** rather than charging at it. Use this orchestration skill as the user-facing workflow for a Forge **Wayfinder** map, then read exactly one companion branch before acting:

- Loose destination or unresolved idea → `chart-map.md`
- Existing Wayfinder id/URL → `work-map.md`

Before creating or operating the map, use `/forge` as the workflow reference for current command, scope, relationship, readiness, lifecycle, and comment rules.

## Shared model

- The map is the canonical index: detailed answers live in child comments/history; the map keeps the destination, gist, and links.
- Wayfinder is planning by default. Children resolve decisions, gather facts, or do small prerequisite work that unblocks decisions.
- Chart only what can be stated sharply now. In-scope fog remains in **Not yet specified** until it becomes a sharp child; ruled-out work belongs in **Out of scope**.
- Every map and child has a title. In human-facing narration and map notes, refer to children as “<title> (#<id>)”, not just “<id>”.
- Forge is authoritative for relationships, readiness, lifecycle state, comments, and history.
- Resolve at most one non-research child per session. Independent research children may run in parallel.
- When the route becomes clear implementation work, hand it to a Spec/Tasks flow rather than building inside the map.

## Route the request

1. Identify whether the request supplies a loose destination or an existing Wayfinder.
2. Read `chart-map.md` or `work-map.md` completely before acting.
3. Preserve the branch's stopping point and Forge relationship rules.
