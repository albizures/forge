# Define a Spec

Synthesize settled discussion into exactly one published Forge Spec. This mode is synthesis, not another interview; publication means approval for planning.

## 1. Gather settled context

Read the conversation and any referenced Wayfinder, Grilling, or other source Records, including relevant Comments and Update Logs. A Wayfinder skill is not required: use the Forge CLI's record commands, inspecting their help for syntax.

Explore the repository for missing facts. Use its domain glossary vocabulary and respect relevant ADRs. Separate settled decisions from proposals and unanswered questions.

If a material unresolved decision or contradiction affects the outcome, scope, or acceptance criteria, stop before publication and return it to grilling. Do not invent an answer. Minor unknowns may remain explicitly recorded when they do not prevent a coherent definition.

## 2. Render the Spec

Read [spec-template.md](spec-template.md) completely and fill its sections from the settled findings:

- State the problem and solution from the user's perspective, with user stories covering the defined scope.
- Number testable acceptance criteria and describe verification per criterion. Include constraints, settled implementation/testing decisions, known testing seams, and out-of-scope work.
- Follow the template's rules for implementation detail; do not manufacture decisions merely to fill a section.
- Put proposed glossary blocks and ADRs in **Domain changes**, or write "None". Capture these proposals in the Spec rather than applying them to the checkout's domain documents.

The heading supplies the Spec's title within its Markdown Record Body; Forge has no separate title flag. Once the body faithfully captures the settled discussion, publish without a separate preview/approval checkpoint.

## 3. Publish and report

Use the Forge skill to select the correct CLI and endpoint. Confirm the intended Workspace and single Project with `forge here`; if scope cannot be determined, ask the human to select or register it before writing.

The following is a command-shape example, not a script. Substitute the inspected Project reference and supply the rendered Markdown on stdin:

```sh
forge spec open --project personal/alpha --body -
```

Use repeatable `--generated-by` for actual source Records when applicable; a conversation alone has no Record ID. Provenance is not parentage or an execution dependency. If an Initiative attachment is warranted by the existing context, use the documented relationship command after creation; do not invent creation flags.

Report the new Spec ID and a concise summary. Leave source Records and the Spec's lifecycle unchanged. Stop here unless the user explicitly requested task creation too; then read [to-tasks.md](to-tasks.md) and continue with this Spec.
