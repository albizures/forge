# Work through the map

Use this branch when the user supplies an existing Forge Wayfinder id or URL.

1. **Load the map.** Run `forge show <wayfinder>` and `forge history <wayfinder>`. Orient on Destination, Notes, Decisions so far, Not yet specified, and Out of scope.
2. **Choose one child.** If the user named one, inspect it. Otherwise use `forge ready` as the executable frontier. Use `--include-hitl` only when intentionally selecting Grilling. Grilling children are selected by explicit user direction or map context.
3. **Claim and start.** Run `forge start <child>`.
4. **Resolve the child.** For Grilling, call `/grilling` and `/domain-modeling` and wait for the human. For research, call `/research`. For prototype, call `/prototype`. For ordinary Tasks, do only the prerequisite work described.
5. **Record completion.** Add the outcome with `forge comment <child> --message-file <file>` or `--message -`, then run `forge done <child> --resolution <slug>`.
6. **Update the map when needed.** Add a Forge comment to the Wayfinder with the linked gist, graduated fog, newly specifiable children, or out-of-scope boundary. Use `forge edit` only when the canonical map body must become the updated index.

Stop after this one non-research child. Other sessions may be working on the same map concurrently, so reload Forge state before future writes.
