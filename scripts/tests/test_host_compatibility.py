"""Linux compatibility checks apply to the executable and every packaged library."""
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
