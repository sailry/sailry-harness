"""Release notes use isolated Git histories, never product tags or remote writes."""

import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("release_notes", ROOT / "scripts/release-notes.py")
notes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notes)
REPOSITORY = "https://example.invalid/product"


class ReleaseNotes(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        notes.git(self.root, "init", "--initial-branch=main", "--template=")
        notes.git(self.root, "config", "user.name", "Fixture")
        notes.git(self.root, "config", "user.email", "fixture@example.invalid")

    def commit(self, subject):
        notes.git(self.root, "commit", "--allow-empty", "-m", subject)
        return notes.git(self.root, "rev-parse", "HEAD")

    def test_first_release_includes_root_commit(self):
        commit = self.commit("Initial product")
        notes.git(self.root, "tag", "v0.1.0")
        entry = notes.generate(self.root, "v0.1.0", REPOSITORY, "2026-10-05")
        self.assertIn("## [0.1.0]", entry)
        self.assertIn("2026-10-05", entry)
        self.assertIn(f"- Initial product ([{commit[:7]}]({REPOSITORY}/commit/{commit}))", entry)
        self.assertNotIn("Full changelog", entry)

    def test_uses_tagged_history_and_previous_release(self):
        old = self.commit("Previous release")
        notes.git(self.root, "tag", "v0.1.0")
        self.commit("docs: update changelog for v0.1.0")
        new = self.commit("Fix [selection] and *spacing*")
        notes.git(self.root, "tag", "-a", "v0.2.0", "-m", "Release")
        self.commit("Not yet released")
        entry = notes.generate(self.root, "v0.2.0", REPOSITORY, "2026-10-05")
        self.assertIn(r"Fix \[selection\] and \*spacing\*", entry)
        self.assertIn(new, entry)
        self.assertIn("compare/v0.1.0...v0.2.0", entry)
        self.assertNotIn(old, entry)
        self.assertNotIn("update changelog", entry)
        self.assertNotIn("Not yet released", entry)

    def test_prepends_entries_without_losing_history(self):
        existing = "# Changelog\n\nIntroduction\n\n## [0.1.0](url)\n\n- Earlier work\n"
        entry = "## [0.2.0](url)\n\n- New work\n"
        result = notes.update(existing, entry, "v0.2.0")
        self.assertEqual(result, "# Changelog\n\nIntroduction\n\n" + entry + "\n" + existing.split("Introduction\n\n")[1])
        self.assertEqual(notes.update(result, entry, "v0.2.0"), result)

    def test_rejects_branches_and_invalid_version_tags(self):
        self.commit("Initial product")
        for tag in ("main", "vtest", "v1", "v1.2.3/other"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                notes.generate(self.root, tag, REPOSITORY, "2026-10-05")
        with self.assertRaises(subprocess.CalledProcessError):
            notes.generate(self.root, "v0.1.0", REPOSITORY, "2026-10-05")

    def test_workflow_only_builds_for_version_tags(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        trigger = workflow.split("permissions:", 1)[0]
        self.assertIn("tags: ['v*']", trigger)
        self.assertNotIn("workflow_dispatch", trigger)
        self.assertNotIn("branches:", trigger)
        self.assertNotIn("pull_request", trigger)
        self.assertIn("--notes-file", workflow)
        self.assertIn("contents/CHANGELOG.md", workflow)
        self.assertNotIn("--generate-notes", workflow)


if __name__ == "__main__":
    unittest.main()
