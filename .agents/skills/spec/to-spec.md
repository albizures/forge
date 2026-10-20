# Define a Spec

Turn the current conversation into exactly one Forge Spec. This branch is synthesis, not an interview.

## Process

1. Explore the repo to understand its current state, if needed. Use the project's domain glossary vocabulary and respect ADRs in the area being touched.
2. Synthesize the current conversation into the semantics of `spec-template.md`. Include acceptance criteria, relevant constraints, and testing seams when already clear. Do not pause only to confirm seams.
3. Render the Spec title and body as Markdown.
4. Confirm Forge project scope with `forge here`. If Forge cannot infer a project-scoped Spec, ask the human to register or select the project before publishing.
5. Publish exactly one Spec with `forge new spec --title <title> --body -` or `--body-file <markdown-file>`. Include `--project`, `--initiative`, or `--generated-by` when the context warrants it.

Do not create a GitHub issue directly or create child Tasks in this branch. Stop after the Spec is created unless the user explicitly requested definition and planning in the same turn; then continue through the router to `to-tasks.md` using the newly created Spec.
