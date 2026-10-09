"""Standalone Python packaging is tested without network access or execution."""
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
SPEC = importlib.util.spec_from_file_location("python_runtime", ROOT / "scripts/package/python-runtime.py")
runtime = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runtime)
TARGET = "aarch64-apple-darwin"


def archive(extra=(), interpreter=b"interpreter"):
    buffer = io.BytesIO()
    entries = [("python/bin/python3.12", interpreter, None),
               ("python/lib/python3.12/site.py", b"stdlib", None),
               ("python/bin/python3", b"", "python3.12"), *extra]
    with tarfile.open(fileobj=buffer, mode="w:gz") as package:
        for name, content, link in entries:
            member = tarfile.TarInfo(name)
            member.mode = 0o755 if "/bin/" in name else 0o644
            member.size = len(content)
            if link is not None:
                member.type = tarfile.SYMTYPE
                member.linkname = link
                member.size = 0
            package.addfile(member, io.BytesIO(content))
    return buffer.getvalue()


class Packaging(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.data = archive()
        self.cache = self.root / "cache"
        self.cache.mkdir()
        (self.cache / "python.tar.gz").write_bytes(self.data)
        self.lock = self.root / "lock.json"
        self.lock.write_text(json.dumps({"version": 1, "python_version": "3.12.13", "targets": {
            TARGET: {"url": "https://release.invalid/python.tar.gz", "archive": "python.tar.gz",
                     "size": len(self.data), "sha256": hashlib.sha256(self.data).hexdigest()}}}))
        self.addCleanup(patch.stopall)
        patch.object(runtime, "LOCK", self.lock).start()

    def test_publishes_complete_offline_runtime(self):
        output = self.root / "output"
        with patch.object(runtime, "urlopen") as request:
            runtime.prepare(TARGET, output, self.cache)
            runtime.prepare(TARGET, output, self.cache)
        request.assert_not_called()
        installed = output / "python"
        self.assertEqual((installed / "bin/python3").resolve(), (installed / "bin/python3.12").resolve())
        self.assertTrue((installed / "lib/python3.12/EXTERNALLY-MANAGED").is_file())
        receipt = json.loads((installed / runtime.MANIFEST).read_text())
        self.assertEqual(receipt["python_version"], "3.12.13")
        self.assertTrue(any(item.get("link") == "python3.12" for item in receipt["files"]))

    def test_rejects_modified_cached_archive(self):
        (self.cache / "python.tar.gz").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            runtime.prepare(TARGET, self.root / "output", self.cache)
        self.assertFalse((self.root / "output/python").exists())

    def test_restores_permissions_after_signing(self):
        output = self.root / "output"
        runtime.prepare(TARGET, output, self.cache)
        executable = output / "python/bin/python3.12"
        before = runtime.digest(executable)
        runtime.unlock(output)
        self.assertTrue(executable.stat().st_mode & 0o200)
        executable.write_bytes(b"signed interpreter")
        runtime.finalize(TARGET, output)
        self.assertFalse(executable.stat().st_mode & 0o200)
        self.assertFalse((output / "python").stat().st_mode & 0o200)
        receipt = json.loads((output / "python" / runtime.MANIFEST).read_text())
        record = next(item for item in receipt["files"] if item["path"] == "bin/python3.12")
        self.assertNotEqual(record["sha256"], before)
        self.assertEqual(record["sha256"], runtime.digest(executable))
        self.assertIn("--entitlements", runtime.signing_options(executable, output))
        self.assertEqual(runtime.signing_options(output / "python/lib/extension.so", output), [])

    def test_rejects_escaping_links_and_paths(self):
        for entry in [("python/bin/escape", b"", "../../outside"),
                      ("python/../outside", b"unsafe", None)]:
            with self.subTest(entry=entry), tempfile.TemporaryDirectory() as destination:
                with self.assertRaises(ValueError):
                    runtime.extract(archive([entry]), Path(destination))

    def test_preserves_unmanaged_destination(self):
        output = self.root / "output"
        (output / "python").mkdir(parents=True)
        content = output / "python/notes.txt"
        content.write_text("keep")
        with self.assertRaisesRegex(ValueError, "not a managed package"):
            runtime.prepare(TARGET, output, self.cache)
        self.assertEqual(content.read_text(), "keep")

    def test_fixed_platform_assets(self):
        lock = json.loads((ROOT / "scripts/package/python-runtime.lock.json").read_text())
        self.assertEqual(len(lock["targets"]), 8)
        for asset in lock["targets"].values():
            self.assertEqual(len(asset["sha256"]), 64)
            self.assertTrue(asset["url"].startswith("https://github.com/astral-sh/python-build-standalone/releases/download/20260807/"))
            self.assertIn("3.12.13", asset["archive"])


if __name__ == "__main__":
    unittest.main()
