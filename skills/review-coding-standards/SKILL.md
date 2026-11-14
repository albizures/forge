---
name: review-coding-standards
description: "Review code changes for violations of the project's CODING_STANDARDS.md. Use after implementation or when a coding standards review is requested."
---

Read `CODING_STANDARDS.md` at the root of the project being reviewed, not relative to this skill's directory. If it is missing, explain that and skip the review. Do not invent standards or substitute personal preferences.

Review uncommitted changes by default (staged, unstaged, and untracked files). If a commit range is provided, review that range instead.

Report only violations of the documented standards introduced by the reviewed changes. Read surrounding code as needed for context, but do not report pre-existing violations. Review only; do not edit code.

For each finding, quote the relevant standard, cite the affected file and line, and explain the violation. Keep the report under 400 words.

End with a one-line summary of the findings and the worst issue(s), if any. If no violations are found, say so explicitly.
