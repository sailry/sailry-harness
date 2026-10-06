"""Independent platform publishers only stage drafts and never mutate public assets."""

import importlib.util
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("publisher", ROOT / "scripts/publish-packages.py")
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


def response(code=0, draft=True, tag="v0.1.0-alpha.1"):
    return subprocess.CompletedProcess([], code, json.dumps({"isDraft":draft,"tagName":tag}), "Fixture failure" if code else "")


class DraftAssets(unittest.TestCase):
    def publish(self):
        publisher.draft("v0.1.0-alpha.1", Path("notes.md"), [Path("Host.tar.gz")])

    def test_creates_a_private_preview_then_uploads_assets(self):
        with patch.object(publisher, "invoke", side_effect=[response(1), response(), response(), response()]) as invoke:
            self.publish()
        calls = [call.args for call in invoke.call_args_list]
        self.assertIn("--draft", calls[1])
        self.assertIn("--verify-tag", calls[1])
        self.assertIn("--prerelease", calls[1])
        self.assertIn("--latest=false", calls[1])
        self.assertEqual(calls[-1], ("release", "upload", "v0.1.0-alpha.1", "Host.tar.gz"))
        self.assertNotIn("--clobber", calls[-1])

    def test_reuses_a_draft_created_by_another_platform(self):
        with patch.object(publisher, "invoke", side_effect=[response(1), response(1), response(), response()]) as invoke:
            self.publish()
        self.assertEqual(invoke.call_args_list[-1].args[1], "upload")

    def test_existing_draft_does_not_require_other_platforms(self):
        with patch.object(publisher, "invoke", side_effect=[response(), response()]) as invoke:
            self.publish()
        self.assertEqual(invoke.call_count, 2)

    def test_refuses_published_or_wrong_version_releases(self):
        for metadata in (response(draft=False), response(tag="v0.1.0-alpha.2")):
            with self.subTest(metadata=metadata.stdout), patch.object(publisher, "invoke", return_value=metadata) as invoke:
                with self.assertRaises(ValueError):
                    self.publish()
                self.assertEqual(invoke.call_count, 1)

    def test_reports_failed_creation_or_duplicate_assets(self):
        for responses in ([response(1), response(1), response(1)], [response(), response(1)]):
            with self.subTest(responses=responses), patch.object(publisher, "invoke", side_effect=responses):
                with self.assertRaises(RuntimeError):
                    self.publish()


if __name__ == "__main__":
    unittest.main()
