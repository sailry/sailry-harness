"""Product builds start with full exact-source Checks; publication waits for them."""

import copy
import importlib.util
import json
import re
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("start_release", ROOT / "scripts/start-release.py")
starter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(starter)
TAG = "v0.1.0-alpha.1"
REPOSITORY = "fixture/product"
CHECK_JOBS = (
    "Source", "Rust strict lint", "Test build", "Shared Node (1/2)", "Shared Node (2/2)",
    "Desktop", "Host", "Mobile", "Mobile Android", "Pairing service",
    "Desktop ready", "Host ready", "Mobile ready", "Checks passed",
)


def metadata(draft=False):
    return {"id": 12, "tag_name": TAG, "draft": draft, "prerelease": True,
            "published_at": None if draft else "2026-10-15T00:00:00Z",
            "immutable": False, "assets": []}


class FakeGitHub:
    def __init__(self, commit, release=None, failed=()):
        self.commit = commit
        self.release = copy.deepcopy(release)
        self.failed = failed
        self.calls = []
        self.runs = [{"id": 42, "head_sha": commit, "head_branch": TAG, "event": "workflow_dispatch",
                      "status": "completed", "conclusion": "success"}]
        self.jobs = [{"id": index, "name": name, "status": "completed", "conclusion": "success"}
                     for index, name in enumerate(CHECK_JOBS, 1)]

    def api(self, method, endpoint, payload=None, missing=False):
        self.calls.append((method, endpoint, payload))
        if endpoint.endswith("/dispatches"):
            if any(f"/{workflow}/" in endpoint for workflow in self.failed):
                raise RuntimeError("Fixture dispatch failure")
            return None
        if "/actions/workflows/ci.yml/runs?" in endpoint:
            return {"workflow_runs": copy.deepcopy(self.runs)}
        if "/actions/runs/" in endpoint:
            run_id = int(endpoint.split("/actions/runs/", 1)[1].split("/", 1)[0])
            if "/jobs?" in endpoint:
                return {"jobs": copy.deepcopy(self.jobs)}
            if "/artifacts?" in endpoint:
                return {"artifacts": []}
            return copy.deepcopy(next(run for run in self.runs if run["id"] == run_id))
        if "/commits/" in endpoint:
            return {"sha": self.commit}
        if "/releases?" in endpoint:
            return [copy.deepcopy(self.release)] if self.release else []
        if method == "GET":
            return copy.deepcopy(self.release)
        if method == "POST":
            self.release = metadata()
        elif method != "PATCH":
            raise AssertionError("Unexpected fixture API call")
        self.release.update(payload)
        self.release["published_at"] = "2026-10-15T00:00:00Z"
        return copy.deepcopy(self.release)


class Publication(unittest.TestCase):
    def setUp(self):
        self.selection = self.enterContext(patch.object(starter.publication.ci_reuse, "source_selection"))
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.tooling = self.root / "tooling"
        self.source = self.root / "product"
        self.tooling.mkdir()
        self.git("init", "--quiet", "--initial-branch=main", "--template=")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "user.name", "Fixture")
        for app in ("desktop", "host"):
            path = self.tooling / f"apps/{app}/Cargo.toml"
            path.parent.mkdir(parents=True)
            path.write_text(f'[package]\nname = "fixture-{app}"\nversion = "{TAG[1:]}"\n')
        mobile = self.tooling / "apps/mobile/pubspec.yaml"
        mobile.parent.mkdir(parents=True)
        mobile.write_text(f"name: fixture\nversion: {TAG[1:]}+1\n")
        (self.tooling / "CHANGELOG.md").write_text("# Changelog\n")
        self.commit = self.record("Initial product")
        self.git("tag", TAG)
        (self.tooling / "tooling.txt").write_text("New release tooling\n")
        self.workflow = self.record("Tooling follow-up")
        self.git("worktree", "add", "--quiet", "--detach", str(self.source), self.commit)
        self.event = self.root / "event.json"
        self.event.write_text(json.dumps({"inputs": {"tag": TAG}}))
        self.environment = {
            "GITHUB_REPOSITORY": REPOSITORY, "GITHUB_SERVER_URL": "https://example.invalid",
            "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": "refs/heads/main",
            "GITHUB_EVENT_PATH": str(self.event), "GITHUB_SHA": self.workflow,
            "WORKFLOW_REVISION": self.workflow,
        }

    def git(self, *arguments, root=None):
        return subprocess.check_output(["git", "-C", str(root or self.tooling), *arguments],
                                       stderr=subprocess.PIPE, text=True).strip()

    def record(self, message):
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", message)
        return self.git("rev-parse", "HEAD")

    def start(self, client, environment=None):
        with patch("builtins.print"):
            return starter.start(TAG, self.source, self.tooling, environment or self.environment, client)

    def dispatches(self, client):
        return [(path.rsplit("/", 2)[1], payload)
                for method, path, payload in client.calls if path.endswith("/dispatches")]

    def test_builds_start_before_checked_publication(self):
        client = FakeGitHub(self.commit)
        with patch.object(starter.publication, "wait_ready") as wait, \
                patch.object(starter.publication, "readiness") as ready:
            self.start(client)
        wait.assert_not_called()
        ready.assert_not_called()
        self.selection.assert_called_once_with(REPOSITORY, client.jobs[0])
        creation = next(call for call in client.calls if call[0] == "POST" and call[1].endswith("/releases"))
        self.assertEqual(creation[2]["target_commitish"], self.commit)
        self.assertFalse(creation[2]["draft"])
        self.assertFalse(creation[2]["prerelease"])
        self.assertEqual(creation[2]["make_latest"], "false")
        self.assertEqual(creation[2]["name"], "Sailry Harness 0.1.0-alpha.1")
        self.assertIn("Initial product", creation[2]["body"])
        self.assertNotIn("Tooling follow-up", creation[2]["body"])
        dispatches = self.dispatches(client)
        self.assertEqual(dispatches, [
            ("release.yml", {"ref": "main", "inputs": {"tag": TAG}}),
            ("release-host.yml", {"ref": "main", "inputs": {"tag": TAG}}),
            ("release-mobile.yml", {"ref": "main", "inputs": {
                "tag": TAG, "platform": "both", "upload_testflight": "true"}}),
            ("changelog.yml", {"ref": "main", "inputs": {"tag": TAG}}),
        ])
        indexes = [index for index, call in enumerate(client.calls) if call[1].endswith("/dispatches")]
        checked = next(index for index, call in enumerate(client.calls) if "/jobs?" in call[1])
        # Desktop and Host build alongside Checks; Mobile and the changelog publish, so they follow.
        self.assertLess(checked, indexes[0])
        self.assertLess(indexes[1], client.calls.index(creation))
        self.assertLess(client.calls.index(creation), indexes[2])
        self.assertTrue(any(call[0] == "GET" and "/releases/tags/" in call[1]
                            for call in client.calls[client.calls.index(creation) + 1:indexes[2]]))

    def test_builds_run_while_checks_are_pending(self):
        client = FakeGitHub(self.commit)
        client.runs[0].update(status="in_progress", conclusion=None)
        def finish(*_, **__):
            self.assertEqual([name for name, _ in self.dispatches(client)], ["release.yml", "release-host.yml"])
            self.assertIsNone(client.release)
            client.runs[0].update(status="completed", conclusion="success")
            return 42
        with patch.object(starter.publication, "wait_ready", side_effect=finish) as wait:
            self.start(client)
        wait.assert_called_once_with(client, REPOSITORY, self.commit, "checks", tag=TAG, ignored_runs=set())
        self.assertFalse(client.release["draft"])
        self.assertEqual(len(self.dispatches(client)), 4)

    def test_draft_publication_preserves_existing_assets(self):
        release = metadata(draft=True)
        release.update(name="Candidate", body="Old notes")
        release["assets"] = [{"id": 5, "name": "existing.zip", "digest": "sha256:" + "a" * 64}]
        client = FakeGitHub(self.commit, release)
        self.start(client)
        self.assertEqual(client.release["assets"], release["assets"])
        self.assertFalse(client.release["draft"])
        self.assertIn("Initial product", client.release["body"])
        self.assertEqual([call[0] for call in client.calls if "/actions/" not in call[1] and call[0] != "GET"], ["PATCH"])

    def test_published_retry_preserves_release(self):
        release = metadata()
        release.update(name="Published version", body="Published notes")
        release["assets"] = [{"id": 5, "name": "existing.zip", "digest": "sha256:" + "a" * 64}]
        client = FakeGitHub(self.commit, release)
        self.start(client)
        self.start(client)
        self.assertEqual(client.release, release)
        self.assertTrue(all(method == "GET" or path.endswith("/dispatches")
                            for method, path, payload in client.calls))
        self.assertEqual(len(self.dispatches(client)), 8)

    def test_failed_dispatch_keeps_other_products_independent(self):
        client = FakeGitHub(self.commit, failed=("release-host.yml",))
        with self.assertRaisesRegex(RuntimeError, "published.*release-host.yml"):
            self.start(client)
        self.assertFalse(client.release["draft"])
        self.assertEqual(len(self.dispatches(client)), 4)

    def test_untrusted_context_stops_before_publication(self):
        for change in ({"GITHUB_REF": f"refs/tags/{TAG}"}, {"GITHUB_EVENT_NAME": "push"},
                       {"GITHUB_SHA": "a" * 40}, {"WORKFLOW_REVISION": "b" * 40}):
            client = FakeGitHub(self.commit)
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.start(client, {**self.environment, **change})
            self.assertFalse(client.calls)
        self.event.write_text(json.dumps({"inputs": {"tag": "v0.1.0-alpha.2"}}))
        client = FakeGitHub(self.commit)
        with self.assertRaisesRegex(ValueError, "dispatch input"):
            self.start(client)
        self.assertFalse(client.calls)

    def test_moved_tag_stops_before_publication(self):
        client = FakeGitHub("a" * 40)
        with self.assertRaisesRegex(ValueError, "remote release tag differs"):
            self.start(client)
        self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_requires_tagged_source_and_trusted_ancestry(self):
        client = FakeGitHub(self.commit)
        self.git("checkout", "--quiet", self.workflow, root=self.source)
        with self.assertRaisesRegex(ValueError, "exact commit"):
            self.start(client)
        self.git("checkout", "--quiet", self.commit, root=self.source)
        self.git("checkout", "--quiet", "--orphan", "unrelated")
        other = self.record("Unrelated history")
        with self.assertRaisesRegex(ValueError, "ancestor"):
            self.start(client, {**self.environment, "GITHUB_SHA": other, "WORKFLOW_REVISION": other})
        self.assertFalse(client.calls)

    def test_requires_matching_product_versions(self):
        (self.source / "apps/mobile/pubspec.yaml").write_text("name: fixture\nversion: 0.2.0+2\n")
        client = FakeGitHub(self.commit)
        with self.assertRaisesRegex(ValueError, "versions must match"):
            self.start(client)
        self.assertFalse(client.calls)

    def test_immutable_release_never_dispatches_packages(self):
        client = FakeGitHub(self.commit, {**metadata(), "immutable": True})
        with self.assertRaisesRegex(ValueError, "mutable"):
            self.start(client)
        self.assertFalse(self.dispatches(client))
        self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_failed_checks_never_publish_or_follow_up(self):
        for error in (ValueError("Checks failed"), TimeoutError("Checks timed out")):
            client = FakeGitHub(self.commit)
            with self.subTest(error=type(error).__name__), \
                    patch.object(starter, "require_checks", side_effect=error), self.assertRaises(type(error)):
                self.start(client)
            self.assertIsNone(client.release)
            # Builds already started; their publish jobs stop at the same failed gate.
            self.assertEqual([name for name, _ in self.dispatches(client)], ["release.yml", "release-host.yml"])
            self.assertTrue(all(method == "GET" or path.endswith("/dispatches") for method, path, _ in client.calls))

    def test_unqueued_checks_start_no_builds(self):
        client = FakeGitHub(self.commit)
        with patch.object(starter, "queue_checks", side_effect=RuntimeError("Checks dispatch failed")), \
                self.assertRaises(RuntimeError):
            self.start(client)
        self.assertIsNone(client.release)
        self.assertFalse(self.dispatches(client))

    def test_rechecks_tag_after_waiting(self):
        client = FakeGitHub(self.commit)
        def move(*_):
            client.commit = "a" * 40
            return 42
        with patch.object(starter, "require_checks", side_effect=move), \
                self.assertRaisesRegex(ValueError, "remote release tag differs"):
            self.start(client)
        self.assertIsNone(client.release)
        self.assertTrue(all(method == "GET" or path.endswith("/dispatches") for method, path, _ in client.calls))

    def test_latest_uses_server_side_version_selection(self):
        self.assertEqual(starter.latest_flag("v1.0.0"), "legacy")
        self.assertEqual(starter.latest_flag("v3.0.0"), "legacy")
        self.assertEqual(starter.latest_flag(TAG), "false")


class FullChecks(unittest.TestCase):
    def setUp(self):
        self.commit = "a" * 40
        self.client = FakeGitHub(self.commit)
        self.logs = self.enterContext(patch.object(
            starter.publication.ci_reuse, "download", return_value=(
                b"Check mode: full\nSelected checks: android, desktop, host, mobile, mobile_ffi, services, shared\n")))
        self.enterContext(patch("builtins.print"))

    def ready(self):
        return starter.publication.ready_run(self.client, REPOSITORY, self.commit, "checks", tag=TAG)

    def require(self):
        queued = starter.queue_checks(self.client, REPOSITORY, TAG, self.commit)
        return starter.require_checks(self.client, REPOSITORY, TAG, self.commit, queued)

    def writes(self):
        return [call for call in self.client.calls if call[0] != "GET"]

    def test_reuses_only_exact_successful_main_or_tag(self):
        for branch in ("main", TAG):
            with self.subTest(branch=branch):
                self.client.runs[0]["head_branch"] = branch
                self.assertEqual(self.require(), 42)
                self.assertFalse(self.writes())
        self.assertEqual(self.logs.call_count, 2)

    def test_rejects_unrelated_or_unfinished_runs(self):
        original = self.client.runs[0]
        for changes in ({"head_sha": "b" * 40}, {"head_branch": "topic"}, {"head_branch": "v0.2.0"},
                        {"event": "pull_request"}, {"event": "push"}, {"status": "in_progress"},
                        {"conclusion": "failure"}, {"conclusion": "cancelled"}, {"conclusion": "skipped"}):
            with self.subTest(changes=changes):
                self.client.runs = [{**original, **changes}]
                self.assertIsNone(self.ready())
        self.logs.assert_not_called()

    def test_requires_every_job_and_fresh_gates(self):
        original = self.client.jobs
        for job in original:
            for state in ("missing", "failure", "cancelled", "skipped", "in_progress"):
                with self.subTest(job=job["name"], state=state):
                    self.client.jobs = copy.deepcopy(original)
                    changed = next(entry for entry in self.client.jobs if entry["id"] == job["id"])
                    if state == "missing":
                        self.client.jobs.remove(changed)
                    elif state == "in_progress":
                        changed.update(status=state, conclusion=None)
                    else:
                        changed["conclusion"] = state
                    self.assertIsNone(self.ready())
        self.logs.assert_not_called()

    def test_requires_full_source_selection(self):
        for content in (b"Check mode: light\nSelected checks: desktop\n",
                        b"Check mode: full\nSelected checks: desktop\n", b""):
            self.logs.return_value = content
            with self.subTest(content=content), self.assertRaisesRegex(ValueError, "every product input"):
                self.ready()

    def test_retains_verified_job_reuse(self):
        self.client.runs[0]["head_branch"] = "main"
        names = {"Rust strict lint", "Test build", "Shared Node (1/2)", "Shared Node (2/2)"}
        for job in self.client.jobs:
            if job["name"] in names:
                job["conclusion"] = "skipped"
        with patch.object(starter.publication.ci_reuse, "verified_jobs", return_value=names) as reuse:
            self.assertEqual(self.ready(), 42)
            reuse.assert_called_once_with(self.client, REPOSITORY, self.commit, 42)
            reuse.return_value = names - {"Test build"}
            self.assertIsNone(self.ready())
            reuse.side_effect = ValueError("Invalid reuse evidence")
            with self.assertRaisesRegex(ValueError, "Invalid reuse evidence"):
                self.ready()

    def test_dispatches_once_despite_old_failed_runs(self):
        successful = {**self.client.runs[0], "id": 43}
        self.client.runs[0]["conclusion"] = "failure"
        def complete(_):
            self.client.runs.append(successful)
        with patch.object(starter.publication.time, "sleep", side_effect=complete) as sleep:
            self.assertEqual(self.require(), 43)
        sleep.assert_called_once_with(60)
        self.assertEqual(self.writes(), [
            ("POST", f"repos/{REPOSITORY}/actions/workflows/ci.yml/dispatches", {"ref": TAG}),
        ])

    def test_waits_for_existing_run_without_dispatch(self):
        self.client.runs[0].update(status="in_progress", conclusion=None)
        jobs, self.client.jobs = self.client.jobs, []
        def complete(_):
            self.client.runs[0].update(status="completed", conclusion="success")
            self.client.jobs = jobs
        with patch.object(starter.publication.time, "sleep", side_effect=complete) as sleep:
            self.assertEqual(self.require(), 42)
        sleep.assert_called_once_with(60)
        self.assertFalse(self.writes())

    def test_new_failure_never_dispatches_again(self):
        failed = {**self.client.runs[0], "conclusion": "failure"}
        self.client.runs = []
        def complete(_):
            self.client.runs = [failed]
        with patch.object(starter.publication.time, "sleep", side_effect=complete) as sleep, \
                self.assertRaisesRegex(ValueError, "ended without"):
            self.require()
        sleep.assert_called_once_with(60)
        self.assertEqual(len(self.writes()), 1)

    def test_missing_run_times_out_without_recovery(self):
        self.client.runs = []
        with patch.object(starter.publication.time, "monotonic", side_effect=[0, 18000]), \
                patch.object(starter.publication.time, "sleep") as sleep, \
                patch.object(starter.publication, "recovered_run") as recovered, \
                self.assertRaisesRegex(TimeoutError, "five hours"):
            self.require()
        sleep.assert_not_called()
        recovered.assert_not_called()
        self.assertEqual(len(self.writes()), 1)

    def test_dispatch_failure_stops_without_retry(self):
        self.client.runs = []
        self.client.failed = ("ci.yml",)
        with patch.object(starter.publication, "wait_ready") as wait, self.assertRaises(RuntimeError):
            self.require()
        wait.assert_not_called()
        self.assertEqual(len(self.writes()), 1)

    def test_run_status_is_rechecked_after_success(self):
        with patch.object(starter.publication, "ready_run", return_value=42), \
                self.assertRaisesRegex(ValueError, "does not match"):
            self.client.runs[0]["conclusion"] = "cancelled"
            starter.publication.wait_ready(self.client, REPOSITORY, self.commit, "checks", tag=TAG)


class Workflows(unittest.TestCase):
    def test_version_entry_owns_tag_trigger(self):
        entry = (ROOT / ".github/workflows/release-version.yml").read_text()
        self.assertIn("tags: ['v*']", entry)
        self.assertIn("if: github.event_name == 'push'", entry)
        self.assertIn('gh workflow run release-version.yml --repo "$GITHUB_REPOSITORY" --ref main', entry)
        self.assertIn("if: github.event_name == 'workflow_dispatch'", entry)
        self.assertIn("ref: refs/tags/${{ env.RELEASE_TAG }}", entry)
        self.assertIn("ref: ${{ github.workflow_sha }}", entry)
        self.assertIn('scripts/start-release.py --source "$GITHUB_WORKSPACE" --tag "$RELEASE_TAG"', entry)
        self.assertIn("timeout-minutes: 310", entry)
        self.assertIn("Start product builds and require full Checks before publication", entry)
        for name, component in (("release", "desktop"), ("release-host", "host")):
            workflow = (ROOT / f".github/workflows/{name}.yml").read_text()
            version = workflow.split("\n  version:\n", 1)[1].split("\n  build:\n", 1)[0]
            publish = workflow.split("\n  publish:\n", 1)[1]
            with self.subTest(workflow=name):
                # Builds start at admission; only publication waits for the exact-source gate.
                self.assertNotIn("--wait-ready", version)
                self.assertNotIn("--published", version)
                self.assertIn("timeout-minutes: 15", version)
                self.assertIn(f"--component {component} --wait-ready --wait-published", publish)
                self.assertIn("timeout-minutes: 320", publish)
                self.assertLess(publish.index("--wait-published"), publish.index("publish-packages.py"))
        self.assertNotIn("cargo build", entry)
        self.assertNotIn("secrets.", entry)
        for name in ("release", "release-host", "release-mobile"):
            trigger = (ROOT / f".github/workflows/{name}.yml").read_text().split("permissions:", 1)[0]
            with self.subTest(workflow=name):
                self.assertIn("workflow_dispatch", trigger)
                self.assertNotIn("push:", trigger)
                self.assertNotIn("release:", trigger)

    def test_preserves_only_release_and_website_automation(self):
        for path in (ROOT / ".github/workflows").glob("*.yml"):
            trigger = path.read_text().split("\non:\n", 1)[1].split("\npermissions:", 1)[0]
            events = set(re.findall(r"^  ([a-z_]+):", trigger, re.MULTILINE))
            automatic = events - {"workflow_dispatch"}
            expected = {"release-version": {"push"}, "website": {"push"}, "changelog": {"release"}}.get(path.stem, set())
            with self.subTest(workflow=path.stem):
                self.assertEqual(automatic, expected)
        website = (ROOT / ".github/workflows/website.yml").read_text()
        self.assertIn("branches: [main]", website)
        self.assertIn("paths:", website)


if __name__ == "__main__":
    unittest.main()
