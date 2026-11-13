"""Validate skills as independent directory installs, without a Forge checkout."""

import re
import shutil
import tempfile
import unittest
from pathlib import Path
from urllib.parse import unquote, urlsplit


SKILLS = Path(__file__).resolve().parents[1] / "skills"


def local_links(document):
    """Ignore illustrative links inside fenced code examples."""
    in_fence = False
    for line in document.splitlines():
        if line.lstrip().startswith(("```", "~~~")):
            in_fence = not in_fence
        elif not in_fence:
            for target in re.findall(r"\[[^\]]*\]\(([^\s)]+)\)", line):
                url = urlsplit(target)
                if not url.scheme and not url.netloc and url.path:
                    yield unquote(url.path)


def validate_links(package):
    package = package.resolve()
    for document in package.rglob("*.md"):
        for link in local_links(document.read_text()):
            target = (document.parent / link).resolve()
            if not target.is_relative_to(package):
                raise AssertionError(f"{document.name}: link escapes skill: {link}")
            if not target.exists():
                raise AssertionError(f"{document.name}: missing reference: {link}")


class SkillPortabilityTests(unittest.TestCase):
    def test_independent_installs(self):
        skills = sorted(path.parent for path in SKILLS.glob("*/SKILL.md"))
        self.assertTrue(skills, "no skills found")
        for source in skills:
            with self.subTest(skill=source.name), tempfile.TemporaryDirectory() as root:
                # Each skill is installed alone, with neither sibling skills nor docs.
                package = Path(root) / "installed-skill"
                shutil.copytree(source, package)
                validate_links(package)

    def test_rejects_checkout_and_sibling_dependencies(self):
        for link in ("../../docs/manual.md", "../forge/SKILL.md", "missing.md"):
            with self.subTest(link=link), tempfile.TemporaryDirectory() as root:
                package = Path(root) / "skill"
                package.mkdir()
                (package / "SKILL.md").write_text(f"Read [reference]({link}).\n")
                with self.assertRaises(AssertionError):
                    validate_links(package)

    def test_local_companions_and_illustrative_links(self):
        with tempfile.TemporaryDirectory() as root:
            package = Path(root)
            (package / "SKILL.md").write_text(
                "Read [reference](reference.md#details).\n"
                "```markdown\n[example](../project/file.md)\n```\n"
            )
            (package / "reference.md").write_text("# Details\n")
            validate_links(package)

    def test_spec_declares_forge_dependency(self):
        spec = (SKILLS / "spec" / "SKILL.md").read_text()
        self.assertIn("Required skill: `forge`", spec)
        self.assertIn("available skills", spec)
        self.assertIn("npx skills add albizures/forge --skill forge", spec)
        self.assertIn("If unavailable, stop", spec)


if __name__ == "__main__":
    unittest.main()
