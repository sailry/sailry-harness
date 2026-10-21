"""Linux compatibility checks apply to the executable and every packaged library."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "check_host", Path(__file__).resolve().parents[1] / "check-host.py")
HOST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HOST)


class Compatibility(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.binary = self.directory / "sailry-host"
        self.binary.write_bytes(b"\x7fELFfixture")
        self.library = self.directory / "libllama.so"
        self.library.write_bytes(b"\x7fELFlibrary")
        (self.directory / "llama-runtime.json").write_text(
            json.dumps({"files": [{"name": self.library.name}]}))

    def check(self, version):
        with patch.object(HOST.shutil, "which", return_value="readelf"), patch.object(
                HOST.subprocess, "run") as run:
            run.return_value.stdout = f"GLIBC_2.2.5 GLIBC_{version} GLIBCXX_3.4.30"
            HOST.check_linux(self.binary)
            self.assertEqual(run.call_count, 3)

    def test_accepts_baseline(self):
        self.check("2.34")

    def test_rejects_newer_library(self):
        with self.assertRaisesRegex(ValueError, "newer than 2.34"):
            self.check("2.35")

    def arm_runtime(self):
        target = "aarch64-unknown-linux-gnu"
        digest = hashlib.sha256(self.library.read_bytes()).hexdigest()
        lock = self.directory / "lock.json"
        lock.write_text(json.dumps({"targets": {target: {"minimum_glibc": "2.38",
            "files": [{"name": self.library.name, "sha256": digest}]}}}))
        (self.directory / "llama-runtime.json").write_text(json.dumps({
            "target": target, "files": [{"name": self.library.name}]}))
        self.addCleanup(patch.stopall)
        patch.object(HOST, "LLAMA_LOCK", lock).start()

    def outputs(self, host="2.34", decision="2.38"):
        return [SimpleNamespace(stdout=f"GLIBC_{host}"),
                SimpleNamespace(stdout=f"GLIBC_{decision}"), SimpleNamespace(stdout="")]

    def test_accepts_locked_arm_runtime(self):
        self.arm_runtime()
        with patch.object(HOST.shutil, "which", return_value="readelf"), patch.object(
                HOST.subprocess, "run", side_effect=self.outputs()):
            HOST.check_linux(self.binary)

    def test_keeps_host_baseline_with_arm_runtime(self):
        self.arm_runtime()
        with patch.object(HOST.shutil, "which", return_value="readelf"), patch.object(
                HOST.subprocess, "run", side_effect=self.outputs(host="2.38")):
            with self.assertRaisesRegex(ValueError, "newer than 2.34: sailry-host"):
                HOST.check_linux(self.binary)

    def test_keeps_ort_baseline_with_arm_runtime(self):
        self.arm_runtime()
        library = self.directory / "libonnxruntime.so"
        library.write_bytes(b"ORT fixture")
        (self.directory / "onnx-runtime.json").write_text(json.dumps({
            "files": [{"name": library.name}]}))
        with patch.object(HOST.shutil, "which", return_value="readelf"), patch.object(
                HOST.subprocess, "run", side_effect=self.outputs() + [SimpleNamespace(stdout="GLIBC_2.38")]):
            with self.assertRaisesRegex(ValueError, "newer than 2.34: libonnxruntime"):
                HOST.check_linux(self.binary)

    def test_rejects_newer_arm_runtime(self):
        self.arm_runtime()
        with patch.object(HOST.shutil, "which", return_value="readelf"), patch.object(
                HOST.subprocess, "run", side_effect=self.outputs(decision="2.39")):
            with self.assertRaisesRegex(ValueError, "newer than 2.38"):
                HOST.check_linux(self.binary)

    def test_rejects_unverified_arm_library(self):
        self.arm_runtime()
        self.library.write_bytes(b"modified library")
        with self.assertRaisesRegex(ValueError, "does not match its lock"):
            HOST.check_linux(self.binary)

    def test_rejects_unrecorded_library(self):
        (self.directory / "libextra.so.1").write_bytes(b"library")
        with self.assertRaisesRegex(ValueError, "no manifest entry"):
            HOST.check_linux(self.binary)

    def test_rejects_unbundled_dependency(self):
        with patch.object(HOST.shutil, "which", return_value="readelf"), patch.object(
                HOST.subprocess, "run", side_effect=[
                    SimpleNamespace(stdout="GLIBC_2.34"), SimpleNamespace(stdout="GLIBC_2.34"),
                    SimpleNamespace(stdout="Shared library: [libcuda.so.1]"),
                ]):
            with self.assertRaisesRegex(ValueError, "unbundled library"):
                HOST.check_linux(self.binary)

    def test_rejects_symlink(self):
        self.library.unlink()
        self.library.symlink_to(self.binary)
        with self.assertRaisesRegex(ValueError, "ordinary file"):
            HOST.check_linux(self.binary)

    def test_skips_other_formats(self):
        self.binary.write_bytes(b"Mach-O fixture")
        with patch.object(HOST.subprocess, "run") as run:
            HOST.check_linux(self.binary)
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
