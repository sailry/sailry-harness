"""Release notes use isolated Git histories, never product tags or remote writes."""

import ast
import importlib.util
import json
import os
from contextlib import chdir
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import unittest
from unittest.mock import patch

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

    def test_prefers_matching_changelog_entry(self):
        self.commit("Internal implementation detail")
        notes.git(self.root, "tag", "v0.1.0-alpha.1")
        entry = "## [0.1.0-alpha.1](release-url)\n\n### Added\n\n- Native workspace\n"
        changelog = "# Changelog\n\n## [Unreleased]\n\n- Future work\n\n" + entry + "\n## [0.1.0]\n\n- Stable work\n"
        generated = notes.generate(self.root, "v0.1.0-alpha.1", REPOSITORY, "2026-10-05", changelog)
        self.assertEqual(generated, entry)
        self.assertEqual(notes.update(changelog, generated, "v0.1.0-alpha.1"), changelog)

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

    def test_rejects_branches_and_invalid_version_tags(self):
        self.commit("Initial product")
        for tag in ("main", "vtest", "v1", "v1.2.3/other"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                notes.generate(self.root, tag, REPOSITORY, "2026-10-05")
        with self.assertRaises(subprocess.CalledProcessError):
            notes.generate(self.root, "v0.1.0", REPOSITORY, "2026-10-05")


class ChangelogText(unittest.TestCase):
    def test_matches_exact_version_and_rejects_empty_entry(self):
        changelog = "## [0.1.0-alpha.10](url)\n\n- Later preview\n\n## [0.1.0](url)\n\n- Stable release\n"
        self.assertIsNone(notes.extract(changelog, "v0.1.0-alpha.1"))
        for changelog in ("## [0.1.0-alpha.1](url)", "## [0.1.0-alpha.1](url)\n",
                          "## [0.1.0-alpha.1]\n\n## [0.1.0]\n\n- Other release\n"):
            with self.subTest(changelog=changelog), self.assertRaisesRegex(ValueError, "must contain release notes"):
                notes.extract(changelog, "v0.1.0-alpha.1")

    def test_prepends_entries_without_losing_history(self):
        existing = "# Changelog\n\nIntroduction\n\n## [0.1.0](url)\n\n- Earlier work\n"
        entry = "## [0.2.0](url)\n\n- New work\n"
        result = notes.update(existing, entry, "v0.2.0")
        self.assertEqual(result, "# Changelog\n\nIntroduction\n\n" + entry + "\n" + existing.split("Introduction\n\n")[1])
        self.assertEqual(notes.update(result, entry, "v0.2.0"), result)


class ReleaseWorkflows(unittest.TestCase):
    def workflow(self, name):
        return (ROOT / f".github/workflows/{name}.yml").read_text()

    def test_desktop_requires_tag_and_own_gate(self):
        workflow = self.workflow("release")
        trigger = workflow.split("permissions:", 1)[0]
        self.assertIn("tags: ['v*']", trigger)
        self.assertIn("workflow_dispatch", trigger)
        self.assertNotIn("component:", trigger)
        self.assertIn('test "$GITHUB_REF_TYPE" = tag', workflow)
        self.assertIn('--component desktop', workflow)
        self.assertNotIn("pull_request", trigger)
        self.assertNotIn("--generate-notes", workflow)

    def test_builds_exact_tag_in_main_cache(self):
        desktop = self.workflow("release")
        self.assertIn("if: github.event_name == 'push'", desktop)
        self.assertIn('--ref main -f "tag=$RELEASE_TAG"', desktop)
        self.assertIn("if: github.event_name != 'push'", desktop)
        for name in ("release", "release-host"):
            workflow = self.workflow(name)
            self.assertIn('test "$GITHUB_EVENT_NAME" = workflow_dispatch', workflow)
            self.assertIn('test "$GITHUB_REF" = refs/heads/main', workflow)
            self.assertEqual(workflow.count("persist-credentials: false"),
                             2 * workflow.count("ref: refs/tags/${{ env.RELEASE_TAG }}"))
            self.assertEqual(workflow.count("ref: ${{ github.workflow_sha }}"),
                             workflow.count("ref: refs/tags/${{ env.RELEASE_TAG }}"))
            self.assertIn("uses: ./.workflow-tools/.github/actions/rust-cache", workflow)
            self.assertIn(".workflow-tools/scripts/ci-run.py", workflow)
            self.assertIn('--source "$GITHUB_WORKSPACE"', workflow)
            self.assertIn('--tag "$RELEASE_TAG" --component', workflow)
            self.assertNotIn('--tag "$GITHUB_REF_NAME"', workflow)
            self.assertIn("cancel-in-progress: false", workflow)
        self.assertIn("desktop-release-${{ github.event_name }}-", desktop)

    def test_gate_accepts_only_matching_dispatch_or_tag(self):
        spec = importlib.util.spec_from_file_location("release_gate", ROOT / "scripts/check-release.py")
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        with tempfile.TemporaryDirectory() as directory:
            event = Path(directory) / "event.json"
            event.write_text(json.dumps({"inputs": {"tag": "v0.1.0"}}))
            base = {"GITHUB_REF": "refs/heads/main", "GITHUB_EVENT_NAME": "workflow_dispatch",
                    "GITHUB_EVENT_PATH": str(event)}
            with patch.dict(os.environ, base, clear=True):
                gate.require_context("v0.1.0")
                with self.assertRaises(ValueError):
                    gate.require_context("v0.2.0")
            for change in ({"GITHUB_REF": "refs/heads/other"}, {"GITHUB_EVENT_NAME": "push"}):
                with patch.dict(os.environ, {**base, **change}, clear=True), self.assertRaises(ValueError):
                    gate.require_context("v0.1.0")
            with patch.dict(os.environ, {"GITHUB_REF_TYPE": "tag", "GITHUB_REF_NAME": "v0.1.0"}, clear=True):
                gate.require_context("v0.1.0")
                with self.assertRaises(ValueError):
                    gate.require_context("v0.2.0")

    def test_tooling_runs_without_candidate_helpers(self):
        spec = importlib.util.spec_from_file_location("release_gate", ROOT / "scripts/check-release.py")
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        with tempfile.TemporaryDirectory() as directory:
            candidate = Path(directory)
            tools = candidate / ".workflow-tools/scripts"
            tools.mkdir(parents=True)
            (tools / "ci-run.py").write_text((ROOT / "scripts/ci-run.py").read_text())
            self.assertFalse((candidate / "scripts/ci-run.py").exists())
            self.assertFalse((candidate / ".github/actions/rust-cache").exists())
            result = subprocess.run(
                [sys.executable, str(tools / "ci-run.py"), "--", sys.executable, "-c", "raise SystemExit(23)"],
                cwd=candidate, capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 23)
            event = candidate / "event.json"
            event.write_text(json.dumps({"inputs": {"tag": "v0.1.0"}}))
            with patch.dict(os.environ, {"GITHUB_REF": "refs/heads/main", "GITHUB_EVENT_NAME": "workflow_dispatch",
                                        "GITHUB_EVENT_PATH": str(event), "GITHUB_REPOSITORY": "fixture/product"}), \
                    patch.object(sys, "argv", ["check-release.py", "--tag", "v0.1.0", "--source", str(candidate),
                                               "--component", "desktop"]), \
                    patch.object(gate.publication, "tagged_commit", return_value="a" * 40) as commit, \
                    patch.object(gate.publication, "readiness", return_value=123) as readiness, \
                    patch.object(gate.publication, "release", return_value={}) as release:
                gate.main()
            commit.assert_called_once_with("v0.1.0", root=candidate)
            self.assertEqual(readiness.call_args.args[2:], ("a" * 40, "desktop"))
            self.assertEqual(release.call_args.args[2:], ("v0.1.0", "a" * 40))

    def test_desktop_publishes_without_host_or_mobile(self):
        workflow = self.workflow("release")
        build = workflow.split("  build:\n", 1)[1].split("  publish:\n", 1)[0]
        publish = workflow.split("  publish:\n", 1)[1]
        self.assertIn("-p sailry-desktop -p sailry-release", build)
        self.assertNotIn("-p sailry-host", build)
        self.assertIn("scripts/package-macos.sh release desktop", build)
        self.assertIn("fixtures shared", build)
        self.assertIn("fixtures desktop", build)
        self.assertIn("needs: build", publish)
        self.assertNotIn("needs: [", publish)
        self.assertIn("*.dmg", build)
        self.assertIn("*.zip.update.json", build)
        self.assertNotIn("*.tar.gz", build)
        self.assertNotIn("*.apk", workflow)
        self.assertNotIn("windows", workflow.lower())
        self.assertIn("--component desktop", publish)

    def test_publication_dispatches_exact_tag_followups(self):
        workflow = self.workflow("release")
        publish = workflow.split("  publish:\n", 1)[1]
        self.assertIn("contents: write", publish)
        self.assertIn("actions: write", publish)
        self.assertIn("'release-host.yml', 'update-feed.yml', 'changelog.yml'", publish)
        self.assertIn("'ref': 'main' if workflow == 'release-host.yml' else tag", publish)
        self.assertIn("'inputs': {'tag': tag}", publish)
        self.assertIn('--tag "$RELEASE_TAG" --published', publish)
        self.assertLess(publish.index("scripts/publish-packages.py"), publish.index("dispatches"))
        self.assertNotIn("release-mobile.yml", publish)

    def test_dispatch_failure_still_attempts_all_followups(self):
        workflow = self.workflow("release").split("Dispatch same-tag Host, feeds and changelog", 1)[1]
        script = textwrap.dedent(workflow.split("python3 - <<'PY'\n", 1)[1].split("\n          PY", 1)[0])
        results = [subprocess.CompletedProcess([], code) for code in (1, 0, 0)]
        with patch.dict(os.environ, {"GITHUB_REPOSITORY": "fixture/product", "RELEASE_TAG": "v0.1.0-alpha.1"}), \
                patch.object(subprocess, "run", side_effect=results) as run:
            with self.assertRaisesRegex(SystemExit, "release-host.yml"):
                exec(script, {})
        self.assertEqual(run.call_count, 3)
        for index, call in enumerate(run.call_args_list):
            self.assertEqual(json.loads(call.kwargs["input"]), {
                "ref": "main" if index == 0 else "v0.1.0-alpha.1", "inputs": {"tag": "v0.1.0-alpha.1"}})

    def test_host_appends_independently_on_four_targets(self):
        workflow = self.workflow("release-host")
        trigger = workflow.split("permissions:", 1)[0]
        self.assertIn("workflow_dispatch", trigger)
        self.assertNotIn("push:", trigger)
        self.assertNotIn("release:", trigger)
        self.assertIn("group: host-release-${{ github.event_name }}-${{ inputs.tag || github.ref_name }}", workflow)
        self.assertIn("--component host --published", workflow)
        mac = workflow.split("  build:\n", 1)[1].split("  host:\n", 1)[0]
        linux = workflow.split("  host:\n", 1)[1].split("  publish:\n", 1)[0]
        publish = workflow.split("  publish:\n", 1)[1]
        self.assertIn("-p sailry-host", mac)
        self.assertNotIn("sailry-desktop", mac)
        self.assertNotIn("sailry-release", mac)
        self.assertNotIn("SAILRY_UPDATE_SIGNING_KEY", mac)
        self.assertIn("scripts/package-macos.sh release host", mac)
        self.assertNotIn("*.dmg", mac)
        self.assertIn('SHA256SUMS-${{ matrix.target }}', mac)
        self.assertIn("ubuntu-24.04-arm", linux)
        self.assertIn("x86_64-unknown-linux-gnu", linux)
        self.assertIn("aarch64-unknown-linux-gnu", linux)
        self.assertIn("scripts/build-host.sh release", linux)
        self.assertIn("scripts/check-host.py", linux)
        self.assertIn("scripts/check-host-install.py", linux)
        self.assertIn("needs: [build, host]", publish)
        self.assertIn("pattern: host-*", publish)
        self.assertIn("SHA256SUMS-installer", publish)
        self.assertIn("--component host", publish)
        self.assertNotIn("release-notes.py", publish)
        self.assertNotIn("actions: write", publish)

    def test_native_cache_and_toolchains_match(self):
        desktop = self.workflow("release").split("  build:\n", 1)[1].split("  publish:\n", 1)[0]
        host = self.workflow("release-host").split("  build:\n", 1)[1].split("  host:\n", 1)[0]
        workflow = self.workflow("release")
        defaults = ast.literal_eval("[" + workflow.split("targets = [", 1)[1].split("]", 1)[0] + "]")
        fallback = json.loads(desktop.split("fromJSON(needs.version.outputs.matrix || '", 1)[1].split("') }}", 1)[0])
        self.assertEqual(defaults, [
            {"runner": "macos-15", "target": "aarch64-apple-darwin", "jobs": 1},
            {"runner": "macos-15-intel", "target": "x86_64-apple-darwin", "jobs": 2},
        ])
        self.assertEqual(fallback, {"include": defaults})
        self.assertIn("runner: macos-15", host)
        for build in (desktop, host):
            with self.subTest(build=build[:80]):
                self.assertIn("macos-15-intel", build)
                self.assertIn("Xcode_26.3.app", build)
                self.assertIn("target/sherpa-onnx-prebuilt", build)
                self.assertIn("uses: ./.workflow-tools/.github/actions/rust-cache", build)
                self.assertNotIn("key: packages-", build)
                self.assertIn("key: ${{ steps.speech-key.outputs.key }}", build)
                self.assertNotIn("github.sha", build)
                self.assertNotIn("--target", build)
                self.assertIn("version: '0.16.0'", build)
                self.assertIn("fixtures shared", build)
        self.assertIn("MACOSX_DEPLOYMENT_TARGET: '13.0'", self.workflow("release"))
        self.assertIn("MACOSX_DEPLOYMENT_TARGET: '13.0'", self.workflow("release-host"))

    def test_mobile_entry_is_manual_readiness_only(self):
        workflow = self.workflow("release-mobile")
        self.assertIn("workflow_dispatch", workflow)
        self.assertIn("--component mobile --published", workflow)
        self.assertIn("distribution remains manual", workflow)
        self.assertNotIn("upload-artifact", workflow)
        self.assertNotIn("contents: write", workflow)
        self.assertNotIn("push:", workflow)

    def test_followups_verify_publication_for_both_triggers(self):
        for name in ("changelog", "update-feed"):
            workflow = self.workflow(name)
            with self.subTest(name=name):
                self.assertIn("types: [published]", workflow)
                self.assertIn("workflow_dispatch", workflow)
                self.assertIn("ref: ${{ inputs.tag || github.event.release.tag_name }}", workflow)
                self.assertIn('scripts/check-release.py --tag "$RELEASE_TAG" --published', workflow)
                self.assertNotIn("created", workflow.split("permissions:", 1)[0])
        changelog = self.workflow("changelog")
        self.assertIn("contents/CHANGELOG.md", changelog)
        feed = self.workflow("update-feed")
        self.assertIn('--event "$RUNNER_TEMP/published-release.json"', feed)
        self.assertIn("--pattern 'Sailry-*.zip' --pattern 'Sailry-*.zip.update.json'", feed)
        self.assertNotIn("sailry-host", feed)

    def test_existing_changelog_skips_remote_write(self):
        workflow = self.workflow("changelog")
        script = textwrap.dedent(workflow.rsplit("python3 - <<'PY'\n", 1)[1].split("\n          PY", 1)[0])
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            original = (ROOT / "CHANGELOG.md").read_bytes()
            (stage / "CHANGELOG.md").write_bytes(original)
            (stage / "changelog-original.md").write_bytes(original)
            with chdir(stage), patch.dict(os.environ, {"GITHUB_REPOSITORY": "fixture/product", "RUNNER_TEMP": directory}), \
                    patch.object(subprocess, "run") as run, patch("builtins.print"):
                with self.assertRaises(SystemExit) as outcome:
                    exec(script, {})
            self.assertEqual(outcome.exception.code, 0)
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
