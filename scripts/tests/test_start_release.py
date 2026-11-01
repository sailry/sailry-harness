"""Version publication precedes independent exact-tag builds without network writes."""

import copy
import importlib.util
import json
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

    def api(self, method, endpoint, payload=None, missing=False):
        self.calls.append((method, endpoint, payload))
        if endpoint.endswith("/dispatches"):
            if any(f"/{workflow}/" in endpoint for workflow in self.failed):
                raise RuntimeError("Fixture dispatch failure")
            return None
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

    def test_publishes_before_independent_product_dispatches(self):
        client = FakeGitHub(self.commit)
        with patch.object(starter.publication, "wait_ready") as wait, \
                patch.object(starter.publication, "readiness") as ready:
            self.start(client)
        wait.assert_not_called()
        ready.assert_not_called()
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
        first = next(index for index, call in enumerate(client.calls) if call[1].endswith("/dispatches"))
        self.assertLess(client.calls.index(creation), first)
        self.assertTrue(any(call[0] == "GET" and "/releases/tags/" in call[1]
                            for call in client.calls[client.calls.index(creation) + 1:first]))

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

    def test_latest_uses_server_side_version_selection(self):
        self.assertEqual(starter.latest_flag("v1.0.0"), "legacy")
        self.assertEqual(starter.latest_flag("v3.0.0"), "legacy")
        self.assertEqual(starter.latest_flag(TAG), "false")


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
        self.assertNotIn("wait-ready", entry)
        self.assertNotIn("cargo build", entry)
        self.assertNotIn("secrets.", entry)
        for name in ("release", "release-host", "release-mobile"):
            trigger = (ROOT / f".github/workflows/{name}.yml").read_text().split("permissions:", 1)[0]
            with self.subTest(workflow=name):
                self.assertIn("workflow_dispatch", trigger)
                self.assertNotIn("push:", trigger)
                self.assertNotIn("release:", trigger)


if __name__ == "__main__":
    unittest.main()
