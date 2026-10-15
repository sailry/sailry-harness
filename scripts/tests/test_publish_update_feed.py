"""Published update feeds reject drafts and use one non-forced ref update."""

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "publish_update_feed", ROOT / "scripts/publish-update-feed.py"
)
publisher = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = publisher
spec.loader.exec_module(publisher)


def event(version="1.0.0", draft=False, prerelease=False, action="published"):
    return {
        "action": action,
        "repository": {"full_name": "sailry/sailry-harness"},
        "release": {
            "draft": draft,
            "prerelease": prerelease,
            "tag_name": f"v{version}",
        },
    }


class ReleaseOwnership(unittest.TestCase):
    def test_accepts_matching_stable_and_preview(self):
        stable = publisher.publication(event(), "sailry/sailry-harness")
        preview = publisher.publication(
            event("1.1.0-alpha.1", prerelease=True), "sailry/sailry-harness"
        )
        self.assertFalse(stable.preview)
        self.assertTrue(preview.preview)

    def test_rejects_draft_wrong_event_channel_or_repository(self):
        fixtures = [
            event(draft=True),
            event(action="edited"),
            event("1.0.0-alpha.1", prerelease=False),
            {**event(), "repository": {"full_name": "other/project"}},
        ]
        for fixture in fixtures:
            with self.subTest(fixture=fixture), self.assertRaises(ValueError):
                publisher.publication(fixture, "sailry/sailry-harness")


class Client:
    def __init__(self, fail_ref=False):
        self.calls = []
        self.fail_ref = fail_ref

    def api(self, method, endpoint, payload=None):
        self.calls.append((method, endpoint, payload))
        if method == "PATCH":
            if self.fail_ref:
                raise RuntimeError("compare-and-swap failed")
            return {"object": {"sha": payload["sha"]}}
        if endpoint.endswith("/git/blobs"):
            return {"sha": f"blob-{len(self.calls)}"}
        if endpoint.endswith("/git/trees"):
            return {"sha": "tree-next"}
        if endpoint.endswith("/git/commits"):
            return {"sha": "commit-next"}
        raise AssertionError((method, endpoint, payload))


class ReadClient:
    def api(self, method, endpoint, payload=None):
        self.last = (method, endpoint, payload)
        if endpoint == "repos/sailry/sailry-harness":
            return {"default_branch": "main"}
        if endpoint.endswith("/git/ref/heads/main"):
            return {"object": {"sha": "commit-before"}}
        if endpoint.endswith("/git/commits/commit-before"):
            return {"tree": {"sha": "tree-before"}}
        if "/contents/" in endpoint:
            encoded = publisher.base64.b64encode(b"signed feed\n").decode("ascii")
            return {"content": f"{encoded[:4]}\n{encoded[4:]}"}
        raise AssertionError((method, endpoint, payload))


class FeedRead(unittest.TestCase):
    def test_decodes_wrapped_contents(self):
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            branch, head, tree, feeds = publisher.read_feeds(
                ReadClient(),
                "sailry/sailry-harness",
                ["update-v1.json"],
                stage,
            )
            self.assertEqual((branch, head, tree), ("main", "commit-before", "tree-before"))
            self.assertEqual(feeds["update-v1.json"].read_bytes(), b"signed feed\n")


class ChildEnvironment(unittest.TestCase):
    def test_scopes_signing_key_to_producer(self):
        completed = publisher.subprocess.CompletedProcess([], 0, stdout="{}")
        with patch.dict(
            publisher.os.environ,
            {publisher.SIGNING_KEY: "inherited-fixture"},
        ), patch.object(publisher.subprocess, "run", return_value=completed) as run:
            publisher.GitHub().api("GET", "repos/sailry/sailry-harness")
            github = run.call_args.kwargs["env"]
            self.assertNotIn(publisher.SIGNING_KEY, github)

            publisher.merge(
                Path("release-tool"),
                Path("keys"),
                Path("assets"),
                "sailry/sailry-harness",
                publisher.Publication("v1.0.0", "1.0.0", False),
                "stable",
                Path("current"),
                Path("output"),
                "producer-fixture",
            )
            producer = run.call_args.kwargs["env"]
            self.assertEqual(producer[publisher.SIGNING_KEY], "producer-fixture")


class CompareAndSwap(unittest.TestCase):
    def test_commits_feeds_with_cas(self):
        client = Client()
        changed = publisher.commit_feeds(
            client,
            "sailry/sailry-harness",
            "main",
            "commit-before",
            "tree-before",
            {"update-v1.json": b"stable", "update-v1-preview.json": b"experience"},
            "v1.0.0",
        )
        self.assertTrue(changed)
        commit = next(call for call in client.calls if call[1].endswith("/git/commits"))
        self.assertEqual(commit[2]["parents"], ["commit-before"])
        patch = client.calls[-1]
        self.assertEqual(patch[:2], (
            "PATCH", "repos/sailry/sailry-harness/git/refs/heads/main"
        ))
        self.assertEqual(patch[2], {"sha": "commit-next", "force": False})

    def test_surfaces_ref_race_without_retry(self):
        client = Client(fail_ref=True)
        with self.assertRaisesRegex(RuntimeError, "compare-and-swap"):
            publisher.commit_feeds(
                client,
                "sailry/sailry-harness",
                "main",
                "commit-before",
                "tree-before",
                {"update-v1.json": b"stable"},
                "v1.0.0",
            )
        self.assertEqual(sum(call[0] == "PATCH" for call in client.calls), 1)

    def test_skips_unchanged_feeds(self):
        client = Client()
        self.assertFalse(publisher.commit_feeds(
            client, "sailry/sailry-harness", "main", "head", "tree", {}, "v1.0.0"
        ))
        self.assertEqual(client.calls, [])


if __name__ == "__main__":
    unittest.main()
