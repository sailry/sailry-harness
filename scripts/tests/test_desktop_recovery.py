"""Signed ARM reuse accepts only the original exact-tag producer and unchanged bytes."""

import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import textwrap
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("recovery", ROOT / "scripts/recover-desktop.py")
recovery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recovery)
TAG = "v0.1.0-alpha.2"
COMMIT = "a" * 40
REPOSITORY = "fixture/product"


class GitHub:
    def __init__(self):
        self.run = {"id": 7, "head_sha": COMMIT, "head_branch": "main", "event": "workflow_dispatch",
                    "path": ".github/workflows/release.yml", "run_attempt": 1, "status": "completed",
                    "conclusion": "failure", "repository": {"id": 3, "full_name": REPOSITORY},
                    "head_repository": {"full_name": REPOSITORY}}
        self.jobs = [{"run_id": 7, "head_sha": COMMIT, "name": f"Desktop {recovery.TARGET}", "status": "completed", "conclusion": "success",
                      "completed_at": "2026-10-17T01:06:00Z", "steps": [
                          {"name": "Build native Desktop", "conclusion": "success"},
                          {"name": "Sign, notarize and package", "conclusion": "success", "completed_at": "2026-10-17T01:05:00Z"},
                          {"name": "Remove temporary signing credentials", "conclusion": "success"}]}]
        self.artifacts = {"total_count": 1, "artifacts": [{
            "id": 9, "name": f"packages-{recovery.TARGET}", "expired": False, "size_in_bytes": 10,
            "digest": "sha256:" + "b" * 64, "created_at": "2026-10-17T01:05:30Z",
            "workflow_run": {"id": 7, "head_sha": COMMIT, "head_branch": "main", "repository_id": 3, "head_repository_id": 3}}]}
        self.log = (f"[command]/usr/bin/git checkout --progress --force refs/tags/{TAG}\n"
                    f"[command]/usr/bin/git log -1 --format=%H\n{COMMIT}\n")

    def api(self, method, endpoint):
        if "/jobs?" in endpoint:
            return {"jobs": copy.deepcopy(self.jobs)}
        if "/artifacts?" in endpoint:
            return copy.deepcopy(self.artifacts)
        return copy.deepcopy(self.run)

    def build_log(self, repository, run_id, name):
        return self.log


class Producer(unittest.TestCase):
    def test_accepts_original_signed_source(self):
        client = GitHub()
        artifact = recovery.source(client, REPOSITORY, TAG, COMMIT, "7")
        self.assertEqual(artifact["id"], 9)

    def test_rejects_changed_provenance(self):
        fixtures = []
        for key, value in (("head_sha", "c" * 40), ("head_branch", "branch"), ("run_attempt", 2),
                           ("status", "in_progress"), ("path", ".github/workflows/other.yml")):
            client = GitHub()
            client.run[key] = value
            fixtures.append(client)
        client = GitHub()
        client.jobs.append(copy.deepcopy(client.jobs[0]))
        fixtures.append(client)
        client = GitHub()
        client.jobs[0]["steps"][1]["conclusion"] = "failure"
        fixtures.append(client)
        for key, value in (("expired", True), ("digest", "sha256:bad"), ("size_in_bytes", 0),
                           ("created_at", "2026-10-17T01:04:59Z")):
            client = GitHub()
            client.artifacts["artifacts"][0][key] = value
            fixtures.append(client)
        client = GitHub()
        client.artifacts["artifacts"][0]["workflow_run"]["head_sha"] = "c" * 40
        fixtures.append(client)
        client = GitHub()
        client.artifacts["artifacts"].append(copy.deepcopy(client.artifacts["artifacts"][0]))
        fixtures.append(client)
        for index, client in enumerate(fixtures):
            with self.subTest(index=index), self.assertRaises(ValueError):
                recovery.source(client, REPOSITORY, TAG, COMMIT, "7")

    def test_rejects_changed_checkout_and_recursive_producer(self):
        for change in (lambda text: text.replace(TAG, "v0.1.0-alpha.1"),
                       lambda text: text.replace(COMMIT, "c" * 40) + f"[command]git log -1 --format=%H\n{COMMIT}\n"):
            client = GitHub()
            client.log = change(client.log)
            with self.assertRaises(ValueError):
                recovery.source(client, REPOSITORY, TAG, COMMIT, "7")
        client = GitHub()
        client.run["head_sha"] = "d" * 40
        with self.assertRaises(ValueError):
            recovery.source(client, REPOSITORY, TAG, COMMIT, "7")

    def test_requires_integer_input(self):
        for value in ("0", "-1", "7; echo value", " 7", "7\n"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                recovery.source(GitHub(), REPOSITORY, TAG, COMMIT, value)


def contents():
    data = {name: b"fixture" for name in recovery.filenames()}
    checksum = f"SHA256SUMS-desktop-{recovery.TARGET}"
    data[checksum] = "".join(f"{hashlib.sha256(value).hexdigest()}  ./{name}\n"
                             for name, value in sorted(data.items()) if name != checksum).encode()
    return data


def archive(data, symlink=False):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as zipped:
        for index, (name, value) in enumerate(data.items()):
            entry = zipfile.ZipInfo(name)
            entry.create_system = 3
            entry.external_attr = ((stat.S_IFLNK if symlink and index == 0 else stat.S_IFREG) | 0o644) << 16
            zipped.writestr(entry, value)
    return output.getvalue()


class SignedFiles(unittest.TestCase):
    def test_preserves_four_assets_and_checksums(self):
        data = contents()
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            recovery.unpack(io.BytesIO(archive(data)), output)
            self.assertEqual({path.name: path.read_bytes() for path in output.iterdir()}, data)

    def test_rejects_unsafe_or_changed_assets(self):
        fixtures = []
        data = contents()
        name = next(name for name in data if name.endswith(".dmg"))
        data["../" + name] = data.pop(name)
        fixtures.append(archive(data))
        fixtures.append(archive(contents(), symlink=True))
        data = contents()
        data[name] = b"changed"
        fixtures.append(archive(data))
        data = contents()
        data["unexpected"] = b"extra"
        fixtures.append(archive(data))
        for encoded in fixtures:
            with tempfile.TemporaryDirectory() as temporary, self.assertRaises(ValueError):
                recovery.unpack(io.BytesIO(encoded), Path(temporary))

    def restore(self, changed_digest=False, changed_metadata=False, signature_error=False):
        data = contents()
        encoded = archive(data)
        artifact = {"id": 9, "digest": "sha256:" + hashlib.sha256(encoded).hexdigest()}
        if changed_digest:
            artifact["digest"] = "sha256:" + "0" * 64
        def command(arguments, **options):
            if arguments[0] == "gh":
                options["stdout"].write(encoded)
                return subprocess.CompletedProcess(arguments, 0)
            if signature_error:
                raise subprocess.CalledProcessError(1, arguments)
            output = Path(arguments[arguments.index("--output") + 1])
            output.write_bytes(b"changed" if changed_metadata else b"fixture")
            return subprocess.CompletedProcess(arguments, 0)
        with tempfile.TemporaryDirectory() as temporary, patch.object(recovery.subprocess, "run", side_effect=command) as run:
            output = Path(temporary) / "recovered-arm"
            recovery.restore(artifact, REPOSITORY, TAG, output, Path("release-tool"), ROOT)
            self.assertEqual({path.name: path.read_bytes() for path in output.iterdir()}, data)
            described = run.call_args.args[0]
            self.assertEqual(described[1], "describe")
            self.assertEqual(described[described.index("--target") + 1], recovery.TARGET)
            self.assertEqual(described[described.index("--version") + 1], TAG[1:])

    def test_verifies_archive_with_compiled_owner(self):
        self.restore()

    def test_rejects_digest_metadata_or_signature_failure(self):
        for options in ({"changed_digest": True}, {"changed_metadata": True}, {"signature_error": True}):
            with self.subTest(options=options), self.assertRaises((ValueError, subprocess.CalledProcessError)):
                self.restore(**options)


class Workflow(unittest.TestCase):
    def selection(self, run_id, fail=False):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        block = workflow.split("      - name: Select native producers\n", 1)[1].split("\n  build:", 1)[0]
        code = block.split("          python3 - <<'PYTHON'\n", 1)[1].split("          PYTHON", 1)[0]
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "output"
            environment = {"RESUME_INTEL_FROM": run_id, "GITHUB_WORKSPACE": "/product", "RELEASE_TAG": TAG,
                           "GITHUB_OUTPUT": str(output)}
            with patch.dict(os.environ, environment), patch.object(subprocess, "run", side_effect=RuntimeError("invalid source") if fail else None) as run:
                if fail:
                    with self.assertRaises(RuntimeError):
                        exec(compile(textwrap.dedent(code), "selector", "exec"), {})
                    self.assertFalse(output.exists())
                    return
                exec(compile(textwrap.dedent(code), "selector", "exec"), {})
            matrix = json.loads(output.read_text().removeprefix("matrix="))
            self.assertEqual(run.call_count, bool(run_id))
            return matrix["include"]

    def test_preserves_default_and_selects_only_intel(self):
        self.assertEqual([target["target"] for target in self.selection("")],
                         [recovery.TARGET, "x86_64-apple-darwin"])
        self.assertEqual([target["target"] for target in self.selection("7")], ["x86_64-apple-darwin"])
        self.selection("7", fail=True)

    def test_overlays_only_packaging_after_compilation(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertLess(workflow.index("name: Build native Desktop"), workflow.index("name: Recover signed ARM packages"))
        self.assertLess(workflow.index("name: Recover signed ARM packages"), workflow.index("name: Assemble ad-hoc Desktop"))
        self.assertNotIn("name: Sign, notarize and package", workflow)
        formal = (ROOT / ".github/workflows/release-desktop-formal.yml").read_text()
        self.assertLess(formal.index("name: Recover signed ARM packages"), formal.index("name: Use workflow notarization helper"))
        self.assertLess(formal.index("name: Use workflow notarization helper"), formal.index("name: Sign, notarize and package"))
        self.assertIn("shutil.copyfile('.workflow-tools/scripts/package/notarize-macos.py', 'scripts/package/notarize-macos.py')", formal)
        for source in (workflow, formal):
            self.assertIn('path: ${{ runner.temp }}/recovered-arm/*', source)
            self.assertIn('pattern: packages-*', source)


if __name__ == "__main__":
    unittest.main()
