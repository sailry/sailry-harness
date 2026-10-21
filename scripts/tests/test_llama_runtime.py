"""Official runtime archives are verified before ordinary files are published."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("llama_runtime", ROOT / "scripts/package/llama-runtime.py")
runtime = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runtime)
TARGET = "aarch64-apple-darwin"


def fixture(directory, cache, target=TARGET, windows=False):
    name = "llama.dll" if windows else "libllama.0.dylib"
    content = b"\xcf\xfa\xed\xfedecision runtime fixture"
    stream = io.BytesIO()
    if windows:
        with zipfile.ZipFile(stream, "w") as archive:
            archive.writestr(name, content)
    else:
        with tarfile.open(fileobj=stream, mode="w:gz") as archive:
            member = tarfile.TarInfo(name)
            member.size = len(content)
            archive.addfile(member, io.BytesIO(content))
    data = stream.getvalue()
    archive = "runtime.zip" if windows else "runtime.tar.gz"
    file = {"name": name, "member": name, "size": len(content),
            "sha256": hashlib.sha256(content).hexdigest()}
    lock = {"version": 1, "build": "b11756", "runtime_version": "0.7.0",
            "source": "https://release.invalid/", "targets": {target: {
                "archive": archive, "size": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                "entry": name, "available": True, "minimum_macos": "13.3", "files": [file]}}}
    cache.mkdir(parents=True, exist_ok=True)
    (cache / archive).write_bytes(data)
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / "llama-runtime.lock.json"
    path.write_text(json.dumps(lock))
    return path, lock, content


class Assets(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.cache = self.root / "cache"
        self.output = self.root / "output"
        self.lock, self.definition, self.content = fixture(self.root, self.cache)
        self.addCleanup(patch.stopall)
        patch.object(runtime, "LOCK", self.lock).start()

    def test_populates_cargo_and_manifest(self):
        with patch.object(runtime.NATIVE, "urlopen") as request:
            runtime.prepare(TARGET, self.output, self.cache, cargo=True)
        request.assert_not_called()
        for directory in (self.output, self.output / "deps", self.output / "examples"):
            self.assertEqual((directory / "libllama.0.dylib").read_bytes(), self.content)
            manifest = json.loads((directory / runtime.MANIFEST).read_text())
            self.assertEqual(set(manifest), {"version", "target", "build", "files"})
            self.assertEqual(manifest["version"], 1)
            self.assertEqual(manifest["target"], TARGET)
            self.assertEqual(manifest["build"], self.definition["build"])
            self.assertEqual(manifest["files"][0]["sha256"], hashlib.sha256(self.content).hexdigest())

    def test_rejects_corrupt_archive_before_publication(self):
        (self.cache / "runtime.tar.gz").write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError, "archive checksum"):
            runtime.prepare(TARGET, self.output, self.cache)
        self.assertFalse(self.output.exists())

    def test_rejects_symlink_destination(self):
        self.output.mkdir()
        outside = self.root / "outside"
        outside.write_bytes(b"preserved")
        (self.output / "libllama.0.dylib").symlink_to(outside)
        with self.assertRaisesRegex(ValueError, "symlink"):
            runtime.prepare(TARGET, self.output, self.cache)
        self.assertEqual(outside.read_bytes(), b"preserved")

    def test_rejects_archive_symlink(self):
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w:gz") as archive:
            member = tarfile.TarInfo("libllama.0.dylib")
            member.type = tarfile.SYMTYPE
            member.linkname = "outside"
            archive.addfile(member)
        asset = self.definition["targets"][TARGET]
        with self.assertRaisesRegex(ValueError, "entry is invalid"):
            runtime.extract(stream.getvalue(), asset, asset["files"])

    def test_supports_windows_archive(self):
        target = "x86_64-pc-windows-msvc"
        fixture(self.root, self.cache, target, windows=True)
        runtime.prepare(target, self.output, self.cache)
        self.assertEqual((self.output / "llama.dll").read_bytes(), self.content)

    def test_keeps_platform_minimum_in_lock(self):
        target = "aarch64-unknown-linux-gnu"
        fixture(self.root, self.cache, target)
        definition = json.loads(self.lock.read_text())
        definition["targets"][target]["minimum_glibc"] = "2.38"
        self.lock.write_text(json.dumps(definition))
        runtime.prepare(target, self.output, self.cache)
        manifest = json.loads((self.output / runtime.MANIFEST).read_text())
        self.assertNotIn("minimum_glibc", manifest)
        self.assertEqual(json.loads(self.lock.read_text())["targets"][target]["minimum_glibc"], "2.38")
        self.assertEqual((self.output / "libllama.0.dylib").read_bytes(), self.content)

    def test_records_unavailable_without_download(self):
        asset = self.definition["targets"][TARGET]
        asset.update(available=False, files=[], reason="Unsupported baseline")
        self.lock.write_text(json.dumps(self.definition))
        with patch.object(runtime.NATIVE, "urlopen") as request:
            runtime.prepare(TARGET, self.output, self.cache)
        request.assert_not_called()
        manifest = json.loads((self.output / runtime.MANIFEST).read_text())
        self.assertEqual(set(manifest), {"version", "target", "build", "files"})
        self.assertEqual(manifest["files"], [])

    def test_finalizes_signed_bundle(self):
        output = self.root / "Sailry.app/Contents/MacOS"
        runtime.prepare(TARGET, output, self.cache)
        file = output / "libllama.0.dylib"
        file.write_bytes(self.content + b"signature")
        runtime.finalize(TARGET, output)
        self.assertFalse((output / runtime.MANIFEST).exists())
        manifest = json.loads((output.parent / "Resources" / runtime.MANIFEST).read_text())
        self.assertEqual(manifest["files"][0]["sha256"], runtime.NATIVE.digest(file))
        self.assertNotEqual(manifest["files"][0]["sha256"], self.definition["targets"][TARGET]["files"][0]["sha256"])


if __name__ == "__main__":
    unittest.main()
