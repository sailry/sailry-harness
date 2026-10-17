"""Native Desktop artifacts survive failed signing without trusting other builds."""

import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("desktop_build", ROOT / "scripts/desktop-build.py")
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)
TARGET = "x86_64-apple-darwin"
PRODUCT = "a" * 40
CONTROLLER = "b" * 40
EXPECTED = {"version": 1, "source_revision": PRODUCT, "workflow_revision": CONTROLLER,
            "tag": "v0.1.0-alpha.1", "target": TARGET, "profile": "release",
            "command": build.COMMAND, "run_id": 7, "rustc": "rustc pinned", "sdk": "26.3",
            "deployment_target": "13.0"}


def payload(changes=None, extra=None, content=None):
    data = {name: b"\xcf\xfa\xed\xfe\x07\x00\x00\x01" + name.encode() for name in build.FILES}
    manifest = {**EXPECTED, "run_attempt": 1,
                "files": {name: {"sha256": hashlib.sha256(value).hexdigest(), "size": len(value)}
                          for name, value in data.items()}, **(changes or {})}
    data["provenance.json"] = json.dumps(manifest).encode()
    if content:
        data.update(content)
    with io.BytesIO() as output:
        with tarfile.open(fileobj=output, mode="w:gz") as archive:
            for name, value in data.items():
                item = tarfile.TarInfo(name)
                item.size = len(value)
                archive.addfile(item, io.BytesIO(value))
            if extra:
                archive.addfile(extra)
        return output.getvalue()


class GitHub:
    def __init__(self, packed):
        self.run = {"id": 7, "head_sha": CONTROLLER, "head_branch": "main", "event": "workflow_dispatch",
                    "path": ".github/workflows/release.yml", "repository": {"full_name": "example/product"},
                    "head_repository": {"full_name": "example/product"}}
        self.job = {"name": f"Build Desktop {TARGET}", "head_sha": CONTROLLER, "run_id": 7,
                    "conclusion": "success", "completed_at": "2026-10-17T01:03:00Z", "steps": [
                        {"name": "Build native Desktop", "conclusion": "success", "completed_at": "2026-10-17T01:00:00Z"},
                        {"name": "Preserve native Desktop", "conclusion": "success"}]}
        self.artifact = {"id": 9, "name": f"native-{TARGET}", "expired": False,
                         "size_in_bytes": len(packed), "digest": "sha256:" + hashlib.sha256(packed).hexdigest(),
                         "created_at": "2026-10-17T01:02:00Z",
                         "workflow_run": {"id": 7, "head_sha": CONTROLLER, "head_branch": "main"}}

    def api(self, method, endpoint):
        if "/artifacts?" in endpoint:
            return {"total_count": 1, "artifacts": [self.artifact]}
        if "/attempts/" in endpoint:
            if "/attempts/1/jobs?" not in endpoint:
                raise AssertionError("Signing retries must use the original successful producer")
            return {"jobs": [self.job]}
        return self.run


class Inputs(unittest.TestCase):
    def test_stages_binaries_with_separate_source_and_controller(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "native.tar.gz"
            directory = root / "target/release"
            directory.mkdir(parents=True)
            for name in build.EXECUTABLES:
                path = directory / name
                path.write_bytes(b"\xcf\xfa\xed\xfe\x07\x00\x00\x01" + name.encode())
                path.chmod(0o755)
            with patch.object(build, "context", return_value=copy.deepcopy(EXPECTED)), \
                    patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "1"}), \
                    patch.object(build.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)):
                build.stage(root, EXPECTED["tag"], TARGET, output)
            extracted = root / "extracted"
            extracted.mkdir()
            manifest = build.unpack(output, extracted, EXPECTED)
            self.assertEqual(manifest["run_attempt"], 1)
            self.assertNotEqual(manifest["source_revision"], manifest["workflow_revision"])
            self.assertEqual(set(manifest["files"]), build.EXECUTABLES)

    def test_restores_original_modes_and_architecture(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "inputs.tar.gz"
            archive.write_bytes(payload())
            output = root / "stage"
            output.mkdir()
            manifest = build.unpack(archive, output, EXPECTED)
            self.assertEqual(manifest["source_revision"], PRODUCT)
            for name in build.EXECUTABLES:
                build.publication.native_binary(output / name, TARGET)
                self.assertEqual(stat.S_IMODE((output / name).stat().st_mode), 0o755)

    def test_rejects_changed_source_target_and_payload(self):
        values = [payload({key: value}) for key, value in (
            ("source_revision", CONTROLLER), ("workflow_revision", PRODUCT), ("target", "aarch64-apple-darwin"),
            ("profile", "debug"), ("sdk", "other"), ("deployment_target", "14.0"), ("command", "cargo check"))]
        values.append(payload(content={"sailry-desktop": b"changed"}))
        for value in values:
            with self.subTest(value=value[:8]), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                archive = root / "inputs.tar.gz"
                archive.write_bytes(value)
                output = root / "stage"
                output.mkdir()
                with self.assertRaises(ValueError):
                    build.unpack(archive, output, EXPECTED)

    def test_rejects_extra_links_and_traversal(self):
        for name, kind in (("../escape", tarfile.REGTYPE), ("link", tarfile.SYMTYPE),
                           ("sailry-desktop", tarfile.LNKTYPE)):
            item = tarfile.TarInfo(name)
            item.type = kind
            item.linkname = "sailry-release"
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                archive = root / "inputs.tar.gz"
                archive.write_bytes(payload(extra=item))
                with self.assertRaises(ValueError):
                    build.unpack(archive, root, EXPECTED)
                self.assertFalse((root / "sailry-desktop").exists())


class Producer(unittest.TestCase):
    def test_accepts_failed_signing_with_original_producer(self):
        with patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "2"}):
            self.assertEqual(build.producer(GitHub(b""), "example/product", EXPECTED, 1)["conclusion"], "success")

    def test_rejects_other_workflow_or_failed_producer(self):
        clients = []
        for key, value in (("head_sha", PRODUCT), ("head_branch", "feature"), ("event", "pull_request"),
                           ("path", ".github/workflows/ci.yml")):
            client = GitHub(b"")
            client.run[key] = value
            clients.append(client)
        for change in (lambda job: job.update(conclusion="failure"),
                       lambda job: job["steps"][0].update(conclusion="failure"),
                       lambda job: job["steps"][1].update(conclusion="failure")):
            client = GitHub(b"")
            change(client.job)
            clients.append(client)
        with patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "2"}):
            for client in clients:
                with self.subTest(client=client), self.assertRaises(ValueError):
                    build.producer(client, "example/product", EXPECTED, 1)
            with self.assertRaises(ValueError):
                build.producer(GitHub(b""), "example/product", EXPECTED, 3)


class Restoration(unittest.TestCase):
    def restore(self, change=None):
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as archive:
            archive.writestr(f"native-{TARGET}.tar.gz", payload())
        packed = output.getvalue()
        client = GitHub(packed)
        if change:
            change(client)
        def download(command, **options):
            self.assertEqual(command, ["gh", "api", "repos/example/product/actions/artifacts/9/zip"])
            options["stdout"].write(packed)
            return subprocess.CompletedProcess(command, 0)
        with tempfile.TemporaryDirectory() as temporary, \
                patch.dict(os.environ, {"GITHUB_REPOSITORY": "example/product", "GITHUB_RUN_ATTEMPT": "2"}), \
                patch.object(build, "context", return_value=copy.deepcopy(EXPECTED)), \
                patch.object(build.publication, "GitHub", return_value=client), \
                patch.object(build.publication, "remote_commit", return_value=PRODUCT), \
                patch.object(build.subprocess, "run", side_effect=download):
            root = Path(temporary)
            build.restore(root, EXPECTED["tag"], TARGET)
            self.assertEqual({path.name for path in (root / "target/release").iterdir()}, build.FILES)

    def test_reuses_native_inputs_without_cargo(self):
        self.restore()

    def test_requires_immutable_current_run_artifact(self):
        for key, value in (("expired", True), ("digest", "sha256:" + "f" * 64),
                           ("created_at", "2026-10-17T00:59:00Z")):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.restore(lambda client: client.artifact.update({key: value}))


class Workflow(unittest.TestCase):
    def test_preserves_builds_before_secrets_and_signing(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        native = workflow.split("\n  build:\n")[1].split("\n  sign:\n")[0]
        signing = workflow.split("\n  sign:\n")[1].split("\n  publish:\n")[0]
        self.assertIn(build.COMMAND, native)
        self.assertNotIn("secrets.", native)
        self.assertLess(native.index("name: Build native Desktop"), native.index("name: Preserve native Desktop"))
        self.assertIn("retention-days: 14", native)
        self.assertIn("needs: [version, build]", signing)
        self.assertIn("desktop-build.py restore", signing)
        self.assertNotIn("cargo build", signing)
        self.assertNotIn("cargo run", signing)
        self.assertIn("name: Sign, notarize and package", signing)
        self.assertIn("needs: sign", workflow.split("\n  publish:\n")[1])
        self.assertIn("pattern: packages-*", workflow)


if __name__ == "__main__":
    unittest.main()
