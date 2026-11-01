"""Release notes use isolated Git histories, never product tags or remote writes."""

import ast
import importlib.util
import json
import os
from contextlib import chdir
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest
from unittest.mock import Mock, patch

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
        self.assertIn("workflow_dispatch", trigger)
        self.assertNotIn("push:", trigger)
        self.assertNotIn("component:", trigger)
        self.assertIn('--component desktop --wait-ready --published', workflow)
        self.assertNotIn("pull_request", trigger)
        self.assertNotIn("--generate-notes", workflow)

    def test_builds_exact_tag_in_main_cache(self):
        desktop = self.workflow("release")
        self.assertNotIn("  dispatch:\n", desktop)
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
        build = workflow.split("  build:\n", 1)[1].split("  package:\n", 1)[0]
        packaging = workflow.split("  package:\n", 1)[1].split("  publish:\n", 1)[0]
        publish = workflow.split("  publish:\n", 1)[1]
        self.assertIn("-p sailry-desktop -p sailry-release", build)
        self.assertNotIn("-p sailry-host", build)
        self.assertIn("name: Preserve native Desktop", build)
        self.assertEqual(set(re.findall(r"secrets\.([A-Z_]+)", build)), {
            "SAILRY_SENTRY_DSN", "SAILRY_TELEMETRY_KEY",
            "SENTRY_AUTH_TOKEN", "SENTRY_ORG", "SENTRY_PROJECT",
        })
        self.assertIn("needs: [version, build]", packaging)
        self.assertIn("scripts/package-desktop-adhoc.py", packaging)
        self.assertNotIn("secrets.", packaging)
        for command in ("cargo build", "cargo run"):
            self.assertNotIn(command, packaging)
        self.assertNotIn("fixtures shared", build)
        self.assertNotIn("fixtures desktop", build)
        self.assertIn("needs: package", publish)
        self.assertIn("pattern: packages-*", publish)
        self.assertNotIn("pattern: native-*", publish)
        self.assertIn("--component desktop-adhoc", publish)
        self.assertNotIn("*.apk", workflow)
        self.assertNotIn("windows", workflow.lower())
        formal = self.workflow("release-desktop-formal")
        signing = formal.split("  sign:\n", 1)[1].split("  publish:\n", 1)[0]
        self.assertIn("scripts/package-macos.sh release desktop", signing)
        self.assertIn("*.zip.update.json", signing)
        self.assertNotIn("*.tar.gz", signing)
        self.assertNotIn("cargo build", signing)
        self.assertNotIn("cargo run", signing)
        self.assertIn("needs: sign", formal.split("  publish:\n", 1)[1])
        self.assertIn("--component desktop --tag", formal)

    def test_desktop_dispatches_formal_signing_only(self):
        publish = self.workflow("release").split("  publish:\n", 1)[1]
        self.assertIn("contents: write", publish)
        self.assertIn("actions: write", publish)
        self.assertIn("release-desktop-formal.yml", publish)
        for name in ("release-host.yml", "release-mobile.yml", "changelog.yml"):
            self.assertNotIn(name, publish)
        self.assertNotIn("update-feed.yml", publish)
        for field in ("source_sha", "producer_run", "producer_revision", "artifacts", "signed_arm_from"):
            self.assertIn("'" + field + "'", publish)
        self.assertLess(publish.index("scripts/publish-packages.py"), publish.index("/dispatches"))
        self.assertNotIn("release-notes.py", publish)
        formal = self.workflow("release-desktop-formal").split("  publish:\n", 1)[1]
        self.assertIn('gh workflow run update-feed.yml --repo "$GITHUB_REPOSITORY" --ref main -f "tag=$RELEASE_TAG"', formal)
        self.assertNotIn("release-host.yml", formal)

    def test_formal_dispatch_preserves_producer_evidence(self):
        workflow = self.workflow("release").split("Dispatch exact-version formal signing\n", 1)[1]
        script = textwrap.dedent(workflow.split("python3 - <<'PY'\n", 1)[1].split("\n          PY", 1)[0])
        environment = {"GITHUB_REPOSITORY": "fixture/product", "RELEASE_TAG": "v0.1.0-alpha.1",
                       "GITHUB_RUN_ID": "7", "WORKFLOW_REVISION": "b" * 40,
                       "NATIVE_ARTIFACTS": "[]", "RESUME_INTEL_FROM": "9"}
        with patch.dict(os.environ, environment), patch.object(subprocess, "check_output", return_value="a" * 40), \
                patch.object(subprocess, "run") as run:
            exec(script, {})
        self.assertEqual(run.call_count, 1)
        self.assertEqual(run.call_args.args[0][4], "repos/fixture/product/actions/workflows/release-desktop-formal.yml/dispatches")
        self.assertTrue(run.call_args.kwargs["check"])
        self.assertEqual(json.loads(run.call_args.kwargs["input"]), {"ref": "main", "inputs": {
            "tag": "v0.1.0-alpha.1", "source_sha": "a" * 40, "producer_run": "7",
            "producer_revision": "b" * 40, "artifacts": "[]", "signed_arm_from": "9"}})

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
        desktop = self.workflow("release").split("  build:\n", 1)[1].split("  package:\n", 1)[0]
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
                self.assertIn("uses: ./.workflow-tools/.github/actions/native-cache", build)
                self.assertIn("uses: ./.workflow-tools/.github/actions/rust-cache", build)
                self.assertNotIn("key: packages-", build)
                self.assertIn("target: ${{ matrix.target }}", build)
                self.assertNotIn("github.sha", build)
                commands = re.findall(r"cargo build[^\n]+", build)
                self.assertTrue(commands)
                for command in commands:
                    self.assertNotIn("--target", command)
                self.assertIn("version: '0.16.0'", build)
                self.assertNotIn("fixtures shared", build)
        self.assertIn("MACOSX_DEPLOYMENT_TARGET: '13.0'", self.workflow("release"))
        self.assertIn("MACOSX_DEPLOYMENT_TARGET: '13.0'", self.workflow("release-host"))

    def test_mobile_packages_require_readiness_and_distribution_signing(self):
        workflow = self.workflow("release-mobile")
        self.assertIn("workflow_dispatch", workflow)
        self.assertIn('.workflow-tools/scripts/package/mobile-release.py --source "$GITHUB_WORKSPACE" admit', workflow)
        self.assertIn("needs: readiness", workflow)
        self.assertIn("IOS_PROVISIONING_PROFILE", workflow)
        self.assertIn("ANDROID_SIGNING_SHA256", workflow)
        self.assertIn("upload_testflight", workflow)
        self.assertIn("upload-artifact", workflow)
        self.assertIn("contents: write", workflow)
        self.assertNotIn("push:", workflow.split("permissions:", 1)[0])

    def test_followups_verify_publication_for_both_triggers(self):
        changelog = self.workflow("changelog")
        self.assertIn("types: [published]", changelog)
        self.assertIn("workflow_dispatch", changelog)
        self.assertIn("ref: ${{ inputs.tag || github.event.release.tag_name }}", changelog)
        self.assertIn('scripts/check-release.py --tag "$RELEASE_TAG" --published', changelog)
        self.assertIn("contents/CHANGELOG.md", changelog)
        feed = self.workflow("update-feed")
        trigger = feed.split("permissions:", 1)[0]
        self.assertIn("workflow_dispatch", trigger)
        self.assertNotIn("release:", trigger)
        self.assertIn("ref: refs/tags/${{ inputs.tag }}", feed)
        self.assertIn("ref: ${{ github.workflow_sha }}", feed)
        self.assertIn('test "$GITHUB_REF" = refs/heads/main', feed)
        self.assertIn('--source "$GITHUB_WORKSPACE"', feed)
        self.assertIn('--event "$RUNNER_TEMP/published-release.json"', feed)
        self.assertIn("steps.canonical.outputs.ready == 'true'", feed)
        self.assertLess(feed.index("--download"), feed.index("cargo build"))
        self.assertIn("Publish signed catalog to the exact release", feed)
        self.assertNotIn("--pattern", feed)
        self.assertNotIn("sailry-host", feed)
        self.assertNotIn("git push", feed)
        self.assertNotIn("contents/update-v1", feed)

    def test_website_waits_for_formal_publication(self):
        feed = self.workflow("update-feed")
        job = feed.split("  publish:\n", 1)[1]
        self.assertIn("permissions:\n      contents: write\n      actions: write", job)
        catalog, dispatch = job.split("      - name: Update changelog and website\n", 1)
        self.assertIn("name: Publish signed catalog to the exact release\n        id: catalog", catalog)
        self.assertIn("if: steps.canonical.outputs.ready == 'true' && steps.catalog.outcome == 'success'", dispatch)
        self.assertIn("GH_TOKEN: ${{ github.token }}", dispatch)
        self.assertIn('gh workflow run changelog.yml --repo "$GITHUB_REPOSITORY" --ref main', dispatch)
        self.assertIn('-f "tag=$RELEASE_TAG" -f notify_website=true', dispatch)
        changelog = self.workflow("changelog")
        trigger = changelog.split("permissions:", 1)[0]
        self.assertIn("notify_website:", trigger)
        self.assertIn("type: boolean\n        default: false", trigger)
        update, notify = changelog.split("      - name: Notify website\n", 1)
        self.assertIn("name: Update changelog\n        id: changelog", update)
        self.assertIn("if: github.event_name == 'workflow_dispatch' && inputs.notify_website && steps.changelog.outcome == 'success'", notify)
        self.assertIn("GH_TOKEN: ${{ github.token }}", notify)
        self.assertIn("WEBSITE_DISPATCH_TOKEN: ${{ secrets.WEBSITE_DISPATCH_TOKEN }}", notify)
        self.assertNotIn("always()", dispatch + notify)
        self.assertNotIn("continue-on-error", dispatch + notify)
        self.assertNotIn("notify_website", self.workflow("release"))

    def test_existing_changelog_notifies_current_main(self):
        workflow = self.workflow("changelog")
        update, notify = workflow.split("      - name: Notify website\n", 1)
        script = textwrap.dedent(update.rsplit("python3 - <<'PY'\n", 1)[1].split("\n          PY", 1)[0])
        notification = textwrap.dedent(notify.split("python3 - <<'PY'\n", 1)[1].split("\n          PY", 1)[0])
        commit = "a" * 40
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            original = (ROOT / "CHANGELOG.md").read_bytes()
            (stage / "CHANGELOG.md").write_bytes(original)
            (stage / "changelog-original.md").write_bytes(original)
            environment = {"GITHUB_REPOSITORY": "fixture/product", "RUNNER_TEMP": directory,
                           "GITHUB_SHA": "b" * 40, "GH_TOKEN": "repository-token",
                           "WEBSITE_DISPATCH_TOKEN": "website-token"}
            with chdir(stage), patch.dict(os.environ, environment, clear=True), \
                    patch.object(subprocess, "run") as run, \
                    patch.object(subprocess, "check_output", return_value=commit + "\n") as read, \
                    patch("builtins.print"):
                with self.assertRaises(SystemExit) as outcome:
                    exec(script, {})
                self.assertEqual(outcome.exception.code, 0)
                run.assert_not_called()
                exec(notification, {})
            read.assert_called_once_with(['gh', 'api', 'repos/fixture/product/git/ref/heads/main',
                                          '--jq', '.object.sha'], text=True)
            run.assert_called_once_with(
                ['gh', 'api', '--method', 'POST', 'repos/sailry/sailry-website/dispatches',
                 '-f', 'event_type=harness-docs-updated', '-f', f'client_payload[sha]={commit}'],
                env={**environment, "GH_TOKEN": "website-token"}, check=True,
            )

    def test_website_notification_propagates_failures(self):
        workflow = self.workflow("changelog").split("      - name: Notify website\n", 1)[1]
        script = textwrap.dedent(workflow.split("python3 - <<'PY'\n", 1)[1].split("\n          PY", 1)[0])
        for reason in ("token", "source", "sha", "dispatch"):
            environment = {"GITHUB_REPOSITORY": "fixture/product", "GH_TOKEN": "repository-token",
                           "WEBSITE_DISPATCH_TOKEN": "" if reason == "token" else "website-token"}
            error = SystemExit if reason in {"token", "sha"} else subprocess.CalledProcessError
            with self.subTest(reason=reason), patch.dict(os.environ, environment, clear=True), \
                    patch.object(subprocess, "check_output", return_value="null" if reason == "sha" else "a" * 40) as read, \
                    patch.object(subprocess, "run") as run:
                if reason == "source":
                    read.side_effect = subprocess.CalledProcessError(1, ["gh", "api"])
                if reason == "dispatch":
                    run.side_effect = subprocess.CalledProcessError(1, ["gh", "api"])
                with self.assertRaises(error) as outcome:
                    exec(script, {})
                if isinstance(outcome.exception, SystemExit):
                    self.assertTrue(outcome.exception.code)
                self.assertEqual(read.call_count, 0 if reason == "token" else 1)
                self.assertEqual(run.call_count, 1 if reason == "dispatch" else 0)


class GateWaiting(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("waiting_gate", ROOT / "scripts/package/publication.py")
        self.gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.gate)
        self.client = Mock()
        self.pending = {"id": 7, "head_sha": "a" * 40, "head_branch": "main", "event": "push", "status": "queued"}

    def jobs(self, component):
        return [{"name": name, "status": "completed", "conclusion": "success"}
                for name in (*self.gate.PREREQUISITES[component], f"{component.title()} ready")]

    def checks(self, jobs, **changes):
        run = {**self.pending, **changes}
        def api(method, path):
            if "/actions/workflows/ci.yml/runs?" in path:
                return {"workflow_runs": [run]}
            if "/actions/runs/7/jobs?" in path:
                return {"jobs": jobs}
            if path.endswith("/actions/runs/7"):
                return run
            raise AssertionError(f"Unexpected readiness request: {method} {path}")
        self.client.api.side_effect = api

    def test_waits_for_queued_exact_source(self):
        self.checks([], status="in_progress")
        with patch.object(self.gate, "ready_run", side_effect=[None, 7]) as ready, \
                patch.object(self.gate.time, "sleep") as sleep:
            self.assertEqual(self.gate.wait_ready(self.client, "fixture/product", "a" * 40, "desktop"), 7)
        self.assertEqual(sleep.call_count, 1)
        self.assertEqual(sleep.call_args.args, (60,))
        self.assertEqual(ready.call_args.args[2:], ("a" * 40, "desktop"))

    def test_admits_before_unrelated_checks_succeed(self):
        for component in self.gate.PREREQUISITES:
            for status, conclusion in (("in_progress", None), ("completed", "failure")):
                jobs = self.jobs(component) + [{"name": "Other product", "status": status, "conclusion": conclusion}]
                self.checks(jobs, status=status, conclusion=conclusion)
                with self.subTest(component=component, status=status), \
                        patch.object(self.gate, "checked_source") as source, \
                        patch.object(self.gate.time, "sleep") as sleep:
                    self.assertEqual(self.gate.wait_ready(self.client, "fixture/product", "a" * 40, component), 7)
                source.assert_not_called()
                sleep.assert_not_called()

    def test_rejects_terminal_failure_or_cancellation(self):
        for conclusion in ("failure", "cancelled", "success"):
            self.checks([], status="completed", conclusion=conclusion)
            with self.subTest(conclusion=conclusion), \
                    patch.object(self.gate.time, "sleep") as sleep, self.assertRaisesRegex(ValueError, "ended without"):
                self.gate.wait_ready(self.client, "fixture/product", "a" * 40, "desktop")
            sleep.assert_not_called()

    def test_rejects_failed_product_jobs(self):
        for component in self.gate.PREREQUISITES:
            for name in (*self.gate.PREREQUISITES[component], f"{component.title()} ready"):
                for conclusion in ("failure", "skipped"):
                    jobs = self.jobs(component)
                    next(job for job in jobs if job["name"] == name)["conclusion"] = conclusion
                    self.checks(jobs, status="in_progress")
                    with self.subTest(component=component, name=name, conclusion=conclusion), \
                            patch.object(self.gate.time, "sleep") as sleep, self.assertRaisesRegex(ValueError, "ended without"):
                        self.gate.wait_ready(self.client, "fixture/product", "a" * 40, component)
                    sleep.assert_not_called()

    def test_rejects_missing_product_jobs(self):
        for component in self.gate.PREREQUISITES:
            for name in (*self.gate.PREREQUISITES[component], f"{component.title()} ready"):
                jobs = [job for job in self.jobs(component) if job["name"] != name]
                self.checks(jobs, status="completed", conclusion="success")
                with self.subTest(component=component, name=name), \
                        patch.object(self.gate.time, "sleep") as sleep, self.assertRaisesRegex(ValueError, "ended without"):
                    self.gate.wait_ready(self.client, "fixture/product", "a" * 40, component)
                sleep.assert_not_called()

    def test_rejects_mismatched_ready_run(self):
        for change in ({"id": 8}, {"head_sha": "b" * 40}, {"head_branch": "topic"}, {"event": "pull_request"}):
            self.client.api.return_value = {**self.pending, **change}
            with self.subTest(change=change), patch.object(self.gate, "ready_run", return_value=7), \
                    patch.object(self.gate.time, "sleep") as sleep, self.assertRaisesRegex(ValueError, "does not match"):
                self.gate.wait_ready(self.client, "fixture/product", "a" * 40, "desktop")
            sleep.assert_not_called()

    def test_ignores_unrelated_source_and_times_out(self):
        self.client.api.return_value = {"workflow_runs": [{**self.pending, "head_sha": "b" * 40, "status": "completed"}]}
        with patch.object(self.gate, "ready_run", return_value=None), \
                patch.object(self.gate.time, "monotonic", side_effect=[0, 18000]), \
                patch.object(self.gate.time, "sleep") as sleep, self.assertRaises(TimeoutError):
            self.gate.wait_ready(self.client, "fixture/product", "a" * 40, "desktop")
        sleep.assert_not_called()

    def test_products_wait_before_compilation(self):
        for workflow, component in (("release", "desktop"), ("release-host", "host")):
            source = (ROOT / f".github/workflows/{workflow}.yml").read_text()
            gate = source.split("  version:\n", 1)[1].split("  build:\n", 1)[0]
            with self.subTest(component=component):
                self.assertIn("timeout-minutes: 310", gate)
                self.assertIn(f"--component {component}", gate)
                self.assertIn("--wait-ready", gate)
                self.assertIn("--published", gate)
                self.assertNotIn("cargo build", gate)

    def test_published_products_can_wait_for_checks(self):
        spec = importlib.util.spec_from_file_location("waiting_release", ROOT / "scripts/check-release.py")
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        for component in ("desktop", "host", "mobile"):
            with self.subTest(component=component), \
                    patch.object(sys, "argv", ["check-release.py", "--tag", "v0.1.0", "--published", "--wait-ready", "--component", component]), \
                    patch.dict(os.environ, {"GITHUB_REPOSITORY": "fixture/product"}), \
                    patch.object(gate, "require_context"), patch.object(gate.publication, "GitHub"), \
                    patch.object(gate.publication, "tagged_commit", return_value="a" * 40), \
                    patch.object(gate.publication, "wait_ready") as wait, \
                    patch.object(gate.publication, "readiness", return_value=7), \
                    patch.object(gate.publication, "release", return_value={}) as release:
                gate.main()
            self.assertEqual(wait.call_args.args[2:], ("a" * 40, component))
            self.assertTrue(release.call_args.kwargs["published"])

    def test_missing_publication_fails_before_waiting(self):
        spec = importlib.util.spec_from_file_location("unpublished_release", ROOT / "scripts/check-release.py")
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        with patch.object(sys, "argv", ["check-release.py", "--tag", "v0.1.0", "--published", "--wait-ready", "--component", "host"]), \
                patch.dict(os.environ, {"GITHUB_REPOSITORY": "fixture/product"}), \
                patch.object(gate, "require_context"), patch.object(gate.publication, "GitHub"), \
                patch.object(gate.publication, "tagged_commit", return_value="a" * 40), \
                patch.object(gate.publication, "wait_ready") as wait, \
                patch.object(gate.publication, "release", side_effect=ValueError("Published release required")), \
                self.assertRaisesRegex(ValueError, "Published release required"):
            gate.main()
        wait.assert_not_called()


if __name__ == "__main__":
    unittest.main()
