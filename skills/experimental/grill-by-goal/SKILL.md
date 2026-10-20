---
name: grill-by-goal
description: A goal-first grilling session that derives a complete decision tree.
disable-model-invocation: true
---

Run a goal-first design session. Apply `/domain-modeling` throughout: sharpen domain language, update the relevant `CONTEXT.md` as terms resolve, and offer ADRs only under that skill's criteria.

Maintain a **design tree**. Its nodes are decisions; a goal and its constraints support the decisions derived from it. Work the tree in rounds. The **frontier** is every decision whose settled prerequisites are sufficient except for goal context. Group frontier decisions that need the same missing goal context into one question.

Start every round with `Settled decisions:` followed by a complete numbered list of the decisions currently settled. For every goal-derived decision, include `Settled because:` with the relevant goals. If none are settled, write `Settled decisions: none yet.` Then ask the whole frontier and wait for the user's response.

The first round asks for the plan's desired outcomes and, for each, why it matters or the constraint it serves. This establishes the initial frontier.

Format each goal question as:

```
❓ **Q1** - **<goal-question title>**: <the goal context needed; ask for outcomes, constraints, priorities, or trade-off criteria, never an implementation choice>

Related settled decisions:
- <only direct settled prerequisites and constraints; or `none yet`>

➡️ This will settle: <the decision or decisions this context will settle>
```

After an answer, derive and record the decisions it settles immediately. State their goal-based rationale in the next round's settled list. The human supplies goal context; the agent settles the decisions. Ask a follow-up goal question whenever the context cannot distinguish between plausible decisions.

When goals conflict, ask for the priority or trade-off criterion before deriving affected decisions. Treat an explicit correction, or a statement that contradicts a recorded goal or rationale, as a changed goal: list every affected decision as invalidated, remove it from the settled list, and recompute the frontier. Treat non-conflicting elaboration as additional context.

Finding facts is the agent's job. Investigate the environment rather than asking the human for facts that can be found. A running investigation is an unsettled prerequisite; ask all other frontier questions while it runs.

The session is done when every goal has traceable necessary decisions and no unresolved decision could materially affect achieving a goal. State that the frontier is empty and wait for the human to confirm shared understanding before acting on the design.
