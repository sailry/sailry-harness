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

    def test_preview_tags_keep_the_full_release_version(self):
        commit = self.commit("Initial alpha")
        notes.git(self.root, "tag", "v0.1.0-alpha.1")
        entry = notes.generate(self.root, "v0.1.0-alpha.1", REPOSITORY, "2026-10-05")
        self.assertIn("## [0.1.0-alpha.1]", entry)
        self.assertIn("/releases/tag/v0.1.0-alpha.1", entry)
        self.assertIn(commit, entry)

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

    def test_manual_and_automatic_builds_require_version_tags(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        trigger = workflow.split("permissions:", 1)[0]
        self.assertIn("tags: ['v*']", trigger)
        self.assertIn("workflow_dispatch", trigger)
        self.assertIn("options: [all, host, desktop, android]", trigger)
        self.assertIn('test "$GITHUB_REF_TYPE" = tag', workflow)
        self.assertNotIn("branches:", trigger)
        self.assertNotIn("pull_request", trigger)
        publisher = (ROOT / "scripts/publish-packages.py").read_text()
        self.assertIn("--notes-file", publisher)
        self.assertNotIn("--generate-notes", workflow)

    def test_packaging_keeps_release_private(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        publisher = (ROOT / "scripts/publish-packages.py").read_text()
        self.assertIn('"--verify-tag", "--draft"', publisher)
        self.assertIn("scripts/publish-packages.py", workflow)
        self.assertNotIn("contents/CHANGELOG.md", workflow)

    def test_changelog_waits_for_publication(self):
        workflow = (ROOT / ".github/workflows/changelog.yml").read_text()
        trigger = workflow.split("permissions:", 1)[0]
        self.assertIn("types: [published]", trigger)
        self.assertNotIn("created", trigger)
        self.assertIn("ref: ${{ github.event.release.tag_name }}", workflow)
        self.assertIn("contents/CHANGELOG.md", workflow)
        self.assertIn("--tag \"$RELEASE_TAG\"", workflow)
        self.assertNotIn("release', 'create'", workflow)

    def test_publishes_both_platforms_with_distribution_signing(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("needs: [build, android]", workflow)
        self.assertIn("SAILRY_ANDROID_RELEASE: '1'", workflow)
        self.assertIn("--certificate", workflow)
        publisher = (ROOT / "scripts/publish-packages.py").read_text()
        self.assertIn("versions.release_flags(tag[1:])", publisher)
        self.assertIn("Sailry Harness", publisher)
        self.assertNotIn("build apk --release --no-pub", workflow)

    def test_linux_host_publication_is_independent(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        desktop = workflow.split("  build:\n", 1)[1].split("  host:\n", 1)[0]
        host = workflow.split("  host:\n", 1)[1].split("  android:\n", 1)[0]
        publisher = workflow.split("  publish-host:\n", 1)[1]
        self.assertNotIn("scripts/build-hosts.sh", desktop)
        self.assertNotIn("cargo-zigbuild", desktop)
        self.assertIn("*.dmg", desktop)
        self.assertIn("ubuntu-24.04-arm", host)
        self.assertIn("x86_64-unknown-linux-gnu", host)
        self.assertIn("aarch64-unknown-linux-gnu", host)
        self.assertIn("scripts/build-host.sh release", host)
        self.assertIn("scripts/check-host.py", host)
        self.assertIn("scripts/check-host-install.py", host)
        self.assertNotIn("scripts/check-host-install.py", desktop)
        self.assertIn("needs: host", publisher)
        self.assertNotIn("needs: [build", publisher)
        self.assertIn("SHA256SUMS-installer", publisher)

    def test_native_builds_install_pinned_zig(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        for job, following, build in (
            ("build", "host", "Build native Desktop and standalone Host"),
            ("host", "android", "Build one Host architecture"),
        ):
            with self.subTest(job=job):
                steps = workflow.split(f"  {job}:\n", 1)[1].split(f"  {following}:\n", 1)[0]
                setup = steps.split("mlugg/setup-zig@", 1)[1].split("      - ", 1)[0]
                self.assertIn("8d6198c65fb0feaa111df26e6b467fea8345e46f", setup)
                self.assertIn("version: '0.16.0'", setup)
                self.assertLess(steps.index("mlugg/setup-zig@"), steps.index(build))


if __name__ == "__main__":
    unittest.main()
