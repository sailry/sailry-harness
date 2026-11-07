"""Linux artifacts remain tied to their successful Host producer and product source."""

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("host_recovery", ROOT / "scripts/recover-host.py")
recovery = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(recovery)
COMMIT = "a" * 40
REPOSITORY = "fixture/product"


class Recovery(unittest.TestCase):
    def recover(self, source=COMMIT, checksum=True, job="success", branch="main"):
        run = {"id": 7, "head_branch": branch, "event": "workflow_dispatch",
               "path": ".github/workflows/release-host.yml",
               "repository": {"full_name": REPOSITORY}, "head_repository": {"full_name": REPOSITORY}}
        jobs = {"jobs": [{"name": f"Linux Host {target}", "conclusion": job} for target in recovery.TARGETS]}
        client = Mock()
        client.api.side_effect = [run, jobs]

        def download(command, **options):
            self.assertEqual(command[:7], ["gh", "run", "download", "7", "--repo", REPOSITORY, "--name"])
            target = command[7].removeprefix("host-")
            directory = Path(command[9])
            path = directory / f"sailry-host-{target}.tar.gz"
            value = {"version": 1, "source_revision": source, "source_dirty": False,
                     "profile": "release", "target": target, "application_version": "0.1.0"}
            content = json.dumps(value).encode()
            with tarfile.open(path, "w:gz") as archive:
                item = tarfile.TarInfo("./build.json")
                item.size = len(content)
                archive.addfile(item, io.BytesIO(content))
            digest = hashlib.sha256(path.read_bytes()).hexdigest() if checksum else "0" * 64
            (directory / f"SHA256SUMS-{target}").write_text(f"{digest}  ./{path.name}\n")
            return subprocess.CompletedProcess(command, 0)

        with tempfile.TemporaryDirectory() as temporary, \
                patch.dict(os.environ, {"GITHUB_REPOSITORY": REPOSITORY}), \
                patch.object(recovery.publication, "tagged_commit", return_value=COMMIT), \
                patch.object(recovery.publication, "release"), \
                patch.object(recovery.publication, "GitHub", return_value=client), \
                patch.object(recovery.subprocess, "run", side_effect=download) as invoke:
            directory = Path(temporary) / "packages"
            recovery.recover(ROOT, "v0.1.0", "7", directory)
            self.assertEqual(invoke.call_count, 2)
            self.assertEqual(len(list(directory.iterdir())), 4)

    def test_reuses_successful_linux_packages(self):
        self.recover()

    def test_rejects_wrong_source_and_checksum(self):
        for options in ({"source": "b" * 40}, {"checksum": False}):
            with self.subTest(options=options), self.assertRaises(ValueError):
                self.recover(**options)

    def test_rejects_other_branch_and_failed_builds(self):
        for options in ({"branch": "feature"}, {"job": "failure"}, {"job": "cancelled"}):
            with self.subTest(options=options), self.assertRaises(ValueError):
                self.recover(**options)

    def test_workflow_skips_linux_and_recovers_before_publication(self):
        workflow = (ROOT / ".github/workflows/release-host.yml").read_text()
        linux = workflow.split("\n  host:\n", 1)[1].split("\n  publish:\n", 1)[0]
        publish = workflow.split("\n  publish:\n", 1)[1]
        self.assertIn("if: inputs.linux_run == ''", linux)
        self.assertIn("needs.sign.result == 'success'", publish)
        self.assertIn("needs.host.result == 'skipped' && inputs.linux_run != ''", publish)
        self.assertIn("actions: read", publish)
        self.assertLess(publish.index("scripts/recover-host.py"), publish.index("scripts/publish-packages.py"))


if __name__ == "__main__":
    unittest.main()
