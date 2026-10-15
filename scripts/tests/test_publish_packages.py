"""Layered publishers preserve released tags and existing assets without network writes."""

import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("publisher", ROOT / "scripts/publish-packages.py")
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)
publication = publisher.publication
TAG = "v0.1.0-alpha.1"
COMMIT = "a" * 40
REPOSITORY = "fixture/product"


def metadata(draft=False):
    return {"id": 12, "tag_name": TAG, "draft": draft, "prerelease": True,
            "published_at": None if draft else "2026-10-15T00:00:00Z",
            "immutable": False, "assets": []}


class FakeGitHub:
    def __init__(self, release=None):
        self.release = release
        self.calls = []
        self.uploads = []

    def api(self, method, endpoint, payload=None, missing=False):
        self.calls.append((method, endpoint, payload))
        if "/commits/" in endpoint:
            return {"sha": COMMIT}
        if method == "GET":
            return copy.deepcopy(self.release)
        if method == "POST":
            self.release = metadata(draft=True)
            return copy.deepcopy(self.release)
        if method == "PATCH":
            self.release.update(payload)
            self.release["published_at"] = "2026-10-15T00:00:00Z"
            return copy.deepcopy(self.release)
        raise AssertionError("Unexpected fixture API call")

    def upload(self, tag, assets):
        self.uploads.extend(path.name for path in assets)
        self.release["assets"].extend(asset(path) for path in assets)

    def digest(self, repository, existing):
        return existing["fixture_digest"]


def asset(path):
    return {"id": 5, "name": path.name, "state": "uploaded", "size": path.stat().st_size,
            "digest": publisher.digest(path)}


class PackagePublication(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.notes = self.root / "notes.md"
        self.notes.write_text("Release notes")
        self.desktop = self.assets("desktop")
        self.host = self.assets("host")

    def assets(self, component):
        output = self.root / component
        output.mkdir()
        for name in publisher.filenames(component, TAG[1:]):
            (output / name).write_bytes(("fixture " + name).encode())
        return sorted(output.iterdir())

    def publish(self, component, client, assets=None):
        publisher.publish(component, TAG, self.notes, assets or getattr(self, component),
                          REPOSITORY, COMMIT, client)

    def public_desktop(self):
        release = metadata()
        release["assets"] = [asset(path) for path in self.desktop]
        return release

    def test_desktop_creates_draft_then_publishes(self):
        client = FakeGitHub()
        self.publish("desktop", client)
        creation = next(call for call in client.calls if call[0] == "POST")
        self.assertEqual(creation[2]["target_commitish"], COMMIT)
        self.assertTrue(creation[2]["draft"])
        self.assertTrue(creation[2]["prerelease"])
        self.assertEqual(len(client.uploads), 8)
        self.assertEqual(client.calls[-1][2], {"draft": False, "make_latest": "false"})

    def test_desktop_retry_preserves_published_assets(self):
        client = FakeGitHub(self.public_desktop())
        self.publish("desktop", client)
        self.assertFalse(client.uploads)
        self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_host_appends_only_missing_assets(self):
        original = self.public_desktop()
        original["assets"].append(asset(self.host[0]))
        client = FakeGitHub(original)
        self.publish("host", client)
        self.assertEqual(set(client.uploads), {path.name for path in self.host[1:]})
        self.assertTrue(all(call[0] == "GET" for call in client.calls))
        self.assertEqual(client.release["assets"][:8], original["assets"][:8])
        self.publish("host", client)
        self.assertEqual(len(client.uploads), 9)

    def test_draft_retry_skips_matching_assets(self):
        release = metadata(draft=True)
        release["assets"] = [asset(self.desktop[0])]
        client = FakeGitHub(release)
        self.publish("desktop", client)
        self.assertEqual(len(client.uploads), 7)

    def test_collision_fails_before_any_upload(self):
        for field, value in (("size", 0), ("digest", "sha256:" + "0" * 64), ("state", "open")):
            with self.subTest(field=field):
                release = self.public_desktop()
                existing = asset(self.host[-1])
                existing[field] = value
                release["assets"].append(existing)
                client = FakeGitHub(release)
                with self.assertRaisesRegex(ValueError, "collision"):
                    self.publish("host", client)
                self.assertFalse(client.uploads)
                self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_missing_digest_verifies_existing_bytes(self):
        release = self.public_desktop()
        release["assets"][0]["fixture_digest"] = release["assets"][0].pop("digest")
        self.publish("desktop", FakeGitHub(release))

    def test_host_requires_public_mutable_desktop(self):
        for release in (None, metadata(draft=True), metadata(),
                        {**self.public_desktop(), "immutable": True}):
            with self.subTest(release=release):
                client = FakeGitHub(release)
                with self.assertRaises(ValueError):
                    self.publish("host", client)
                self.assertFalse(client.uploads)
                self.assertTrue(all(call[0] == "GET" for call in client.calls))

    def test_public_desktop_cannot_acquire_missing_assets(self):
        client = FakeGitHub(metadata())
        with self.assertRaisesRegex(ValueError, "missing Desktop"):
            self.publish("desktop", client)
        self.assertFalse(client.uploads)

    def test_component_allowlists_reject_mixed_or_partial_assets(self):
        for component, files in (("host", self.desktop), ("desktop", self.host),
                                 ("host", self.host[:-1]), ("desktop", self.desktop + self.host)):
            with self.subTest(component=component, count=len(files)):
                with self.assertRaises(ValueError):
                    self.publish(component, FakeGitHub(), files)

    def test_moved_tag_and_wrong_channel_fail(self):
        client = FakeGitHub(self.public_desktop())
        with self.assertRaisesRegex(ValueError, "differs"):
            publisher.publish("host", TAG, None, self.host, REPOSITORY, "b" * 40, client)
        client.release["prerelease"] = False
        with self.assertRaisesRegex(ValueError, "channel"):
            self.publish("host", client)
        self.assertFalse(client.uploads)


class Readiness(unittest.TestCase):
    def run_fixture(self, gate="success", event="push", branch="main", sha=COMMIT, overall="failure"):
        client = Mock()
        client.api.side_effect = [
            {"workflow_runs": [{"id": 42, "head_sha": sha, "head_branch": branch,
                                "event": event, "conclusion": overall}]},
            {"jobs": [{"name": name, "status": "completed", "conclusion": "success"}
                      for name in ("Source", "Shared Node", "Desktop")] +
                     [{"name": "Desktop ready", "status": "completed", "conclusion": gate}]},
        ]
        return client

    def test_product_success_admits_despite_other_product_failure(self):
        client = self.run_fixture()
        self.assertEqual(publication.readiness(client, REPOSITORY, COMMIT, "desktop"), 42)
        self.assertIn("head_sha=" + COMMIT, client.api.call_args_list[0].args[1])
        self.assertIn("filter=latest", client.api.call_args_list[1].args[1])

    def test_rejects_skipped_failed_and_missing_gates(self):
        for conclusion in ("skipped", "failure", "cancelled", None):
            with self.subTest(conclusion=conclusion), self.assertRaises(ValueError):
                publication.readiness(self.run_fixture(gate=conclusion), REPOSITORY, COMMIT, "desktop")
        with self.assertRaises(ValueError):
            publication.readiness(self.run_fixture(), REPOSITORY, COMMIT, "host")

    def test_rejects_partial_pr_and_wrong_sha(self):
        for options in ({"event": "pull_request"}, {"branch": "feature"}, {"sha": "b" * 40}):
            client = self.run_fixture(**options)
            with self.subTest(options=options), self.assertRaises(ValueError):
                publication.readiness(client, REPOSITORY, COMMIT, "desktop")
            self.assertEqual(client.api.call_count, 1)

    def test_full_manual_main_can_admit(self):
        self.assertEqual(publication.readiness(self.run_fixture(event="workflow_dispatch"),
                                              REPOSITORY, COMMIT, "desktop"), 42)

    def test_requires_actual_prerequisites_with_success_gate(self):
        for name in ("Source", "Shared Node", "Desktop"):
            for conclusion in ("failure", "skipped"):
                client = self.run_fixture()
                runs, jobs = list(client.api.side_effect)
                for job in jobs["jobs"]:
                    if job["name"] == name:
                        job["conclusion"] = conclusion
                client.api.side_effect = [runs, jobs]
                with self.subTest(name=name, conclusion=conclusion), self.assertRaises(ValueError):
                    publication.readiness(client, REPOSITORY, COMMIT, "desktop")

    def test_tag_must_match_checkout_and_product(self):
        with patch.object(publication.versions, "product", return_value=(TAG[1:], 1)), \
                patch.object(publication.subprocess, "check_output", side_effect=[COMMIT, "b" * 40]):
            with self.assertRaisesRegex(ValueError, "exact commit"):
                publication.tagged_commit(TAG)
        with patch.object(publication.versions, "product", return_value=(TAG[1:], 1)):
            with self.assertRaisesRegex(ValueError, "application version"):
                publication.tagged_commit("v0.1.0-alpha.2")


if __name__ == "__main__":
    unittest.main()
