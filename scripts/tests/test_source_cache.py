"""Source archive cleanup preserves other graphs, scopes and active consumers."""

import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.parse import parse_qs, urlsplit


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
spec = importlib.util.spec_from_file_location("prune_source_cache", ROOT / "scripts/prune-source-cache.py")
cleanup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cleanup)


class Archives(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.lockfile = Path(self.directory.name) / "Cargo.lock"
        self.lockfile.write_text(
            'version = 4\n[[package]]\nname = "application"\nversion = "0.1.0"\n'
            '[[package]]\nname = "dependency"\nversion = "1.0.0"\n'
            'source = "registry+https://example.invalid/index"\nchecksum = "abc"\n',
        )
        self.key = cleanup.ci.source_cache_key(self.lockfile)
        self.legacy = cleanup.legacy_key(self.lockfile)
        self.entries = [self.archive(1, self.key), self.archive(2, self.legacy)]
        self.runs = {status: [] for status in cleanup.ACTIVE_STATES}
        self.deleted = []

    def archive(self, identity, key, **changes):
        return {"id": identity, "key": key, "ref": cleanup.MAIN_REF,
                "version": "layout-v1", "size_in_bytes": 1024, **changes}

    def api(self, repository, endpoint, method="GET"):
        self.assertEqual(repository, "example/project")
        if method == "DELETE":
            self.deleted.append(int(endpoint.rsplit("/", 1)[1]))
            return None
        url = urlsplit(endpoint)
        query = parse_qs(url.query)
        page = int(query["page"][0])
        self.assertEqual(query["per_page"], ["100"])
        if url.path == "actions/caches":
            self.assertEqual(query["ref"], [cleanup.MAIN_REF])
            values = [entry for entry in self.entries if entry["key"].startswith(query["key"][0])]
            field = "actions_caches"
        else:
            self.assertEqual(url.path, "actions/runs")
            values = self.runs[query["status"][0]]
            field = "workflow_runs"
        return {field: values[(page - 1) * 100:page * 100]}

    def prune(self, lockfiles=None):
        with patch.object(cleanup, "api", side_effect=self.api):
            return cleanup.prune("example/project", 10, self.key, lockfiles or [self.lockfile])

    def test_removes_only_replaced_layout_in_main(self):
        self.entries.extend([
            self.archive(3, self.legacy, ref="refs/heads/feature"),
            self.archive(4, self.legacy, ref="refs/tags/v1"),
            self.archive(5, self.legacy, version="different-layout"),
            self.archive(6, self.legacy + "-other"),
            self.archive(7, "sccache-object"),
            self.archive(8, "cargo-sources-v1-" + "f" * 64),
        ])
        self.runs["in_progress"] = [{"id": 10, "status": "in_progress"}]
        self.assertEqual(self.prune(), [2])
        self.assertEqual(self.deleted, [2])

    def test_requires_saved_replacement(self):
        for changes in (
            {"ref": "refs/heads/feature"}, {"size_in_bytes": 0}, {"version": ""},
        ):
            with self.subTest(changes=changes):
                self.entries = [self.archive(1, self.key, **changes), self.archive(2, self.legacy)]
                with self.assertRaises(ValueError):
                    self.prune()
                self.assertEqual(self.deleted, [])

    def test_defers_for_other_active_runs(self):
        for state in cleanup.ACTIVE_STATES:
            with self.subTest(state=state):
                self.runs = {status: [] for status in cleanup.ACTIVE_STATES}
                self.runs[state] = [{"id": 11, "status": state}]
                self.assertEqual(self.prune(), [])
                self.assertEqual(self.deleted, [])

    def test_rejects_other_dependency_graphs(self):
        alternate = self.lockfile.with_name("other.lock")
        alternate.write_text(self.lockfile.read_text().replace('checksum = "abc"', 'checksum = "def"'))
        with self.assertRaisesRegex(ValueError, "different external dependencies"):
            self.prune([self.lockfile, alternate])
        self.assertEqual(self.deleted, [])

    def test_accepts_workspace_only_version_changes(self):
        alternate = self.lockfile.with_name("previous.lock")
        alternate.write_text(self.lockfile.read_text().replace('version = "0.1.0"', 'version = "0.2.0"'))
        old_key = cleanup.legacy_key(alternate)
        self.assertNotEqual(old_key, self.legacy)
        self.entries.append(self.archive(3, old_key))
        self.assertCountEqual(self.prune([self.lockfile, alternate]), [2, 3])

    def test_paginates_exact_legacy_matches(self):
        self.entries = [self.archive(index + 2, self.legacy + "-other") for index in range(100)]
        self.entries.extend([self.archive(1, self.key), self.archive(102, self.legacy)])
        self.assertEqual(self.prune(), [102])

    def test_propagates_api_failure_before_deletion(self):
        with patch.object(cleanup, "api", side_effect=RuntimeError("Unavailable")):
            with self.assertRaisesRegex(RuntimeError, "Unavailable"):
                cleanup.prune("example/project", 10, self.key, [self.lockfile])
        self.assertEqual(self.deleted, [])

class Invocation(unittest.TestCase):
    def test_skips_pull_requests_and_tags(self):
        for reference, event in (("refs/pull/1/merge", "pull_request"),
                                 ("refs/tags/v1", "push"),
                                 (cleanup.MAIN_REF, "workflow_run")):
            with self.subTest(reference=reference, event=event):
                result = subprocess.run(
                    [sys.executable, str(ROOT / "scripts/prune-source-cache.py"), "--saved-key", "unused"],
                    env={**os.environ, "GITHUB_REF": reference, "GITHUB_EVENT_NAME": event},
                    capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_reports_missing_token(self):
        environment = {**os.environ, "GITHUB_REF": cleanup.MAIN_REF, "GITHUB_EVENT_NAME": "push"}
        environment.pop("GH_TOKEN", None)
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/prune-source-cache.py"), "--saved-key", "unused"],
            env=environment, capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("::warning::Source cache cleanup failed", result.stdout)
        self.assertIn("requires an Actions write token", result.stdout)


if __name__ == "__main__":
    unittest.main()
