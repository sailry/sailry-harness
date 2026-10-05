"""Pinned Linux link inputs are checked before extraction."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("sysroot", ROOT / "scripts/package/linux-sysroot.py")
sysroot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sysroot)


class Extraction(unittest.TestCase):
    def test_rejects_checksum_before_running_tools(self):
        with patch.object(sysroot.subprocess, "check_output") as command:
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                sysroot.extract(b"fixture", "0" * 64, "ar", Path("unused"))
        command.assert_not_called()

    def test_extracts_data_and_rejects_escaping_paths(self):
        for name in ("usr/lib/fixture.so", "../escape"):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                buffer = io.BytesIO()
                with tarfile.open(fileobj=buffer, mode="w:xz") as archive:
                    entry = tarfile.TarInfo(name)
                    entry.size = 7
                    archive.addfile(entry, io.BytesIO(b"fixture"))
                package = b"fixture package"
                digest = hashlib.sha256(package).hexdigest()
                with patch.object(sysroot.subprocess, "check_output", side_effect=["data.tar.xz\n", buffer.getvalue()]):
                    if name.startswith("../"):
                        with self.assertRaises(tarfile.OutsideDestinationError):
                            sysroot.extract(package, digest, "ar", Path(directory))
                    else:
                        sysroot.extract(package, digest, "ar", Path(directory))
                        self.assertEqual((Path(directory) / name).read_bytes(), b"fixture")

    def test_locks_both_architectures_and_preserves_unrecognized_output(self):
        lock = json.loads(sysroot.LOCK.read_text())
        self.assertEqual(set(lock["packages"]), set(sysroot.TARGETS.values()))
        for packages in lock["packages"].values():
            self.assertTrue(packages)
            for package in packages:
                self.assertEqual(len(package["sha256"]), 64)
                self.assertTrue(package["filename"].endswith(".deb"))
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "existing"
            output.mkdir()
            fixture = output / "keep"
            fixture.write_bytes(b"fixture")
            with patch.object(sysroot, "urlopen") as network:
                with self.assertRaisesRegex(ValueError, "unrecognized sysroot"):
                    sysroot.prepare("aarch64-unknown-linux-gnu", output, "ar")
            network.assert_not_called()
            self.assertEqual(fixture.read_bytes(), b"fixture")


if __name__ == "__main__":
    unittest.main()
