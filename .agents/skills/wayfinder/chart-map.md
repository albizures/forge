# Chart the map

Use this branch when the user brings a loose destination.

1. **Name the destination.** Call `/grilling` and `/domain-modeling` to pin down what the map is finding its way to. The destination fixes scope.
2. **Map the frontier.** Grill breadth-first across open decisions, prerequisite research/prototypes/tasks, and fog. If there is no fog and the route fits one session, do not create a map; ask how the user wants to proceed.
3. **Confirm Forge scope.** Run `forge here` when the map is project-scoped. If scope is ambiguous, ask the human before creating records.
4. **Create the Wayfinder.** Use `forge new wayfinder --title <title> --body -` or `--body-file <markdown-file>`. Include Destination, Notes, empty Decisions so far, Not yet specified, and Out of scope sections.
5. **Create the frontier.** Create sharp Forge children under the Wayfinder with `--parent <wayfinder-id>`:
   - collaborative HITL decisions with `forge new grilling`;
   - AFK investigation with `forge new task --kind research`;
   - cheap artifacts with `forge new task --kind prototype`;
   - prerequisite work with ordinary `forge new task`.
6. **Wire dependencies.** Use repeatable `--depends-on` or `forge deps add`; create blockers before dependents when ids are needed.
7. **Start independent research.** Dispatch research children in parallel. For each, call `/research`, record the outcome with `forge comment`, and complete it with `forge done <task> --resolution completed`.

Stop after charting. Charting creates the map and frontier; it does not resolve a HITL or other non-research child in the same session.
