"""Native runtime packaging fixtures do not load libraries or contact release servers."""

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("onnx_runtime", ROOT / "scripts/package/onnx-runtime.py")
runtime = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runtime)
TARGET = "aarch64-apple-darwin"


def archive(entries):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:bz2") as package:
        for name, content, kind in entries:
            member = tarfile.TarInfo(name)
            member.type = kind
            member.size = len(content) if kind == tarfile.REGTYPE else 0
            if kind == tarfile.SYMTYPE:
                member.linkname = "../../outside"
            package.addfile(member, io.BytesIO(content))
    return output.getvalue()


class Assets(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.output = self.root / "output"
        self.cache = self.root / "cache"
        self.files = []
        self.entries = []
        for name, role, content in (("libonnxruntime.dylib", "runtime", b"runtime fixture"),
                                    ("libsherpa-onnx-c-api.dylib", "speech", b"speech fixture"),
                                    ("sherpa-onnx-c-api.lib", "link", b"link fixture")):
            member = "fixed/lib/" + name
            self.files.append({"name": name, "member": member, "size": len(content),
                               "sha256": hashlib.sha256(content).hexdigest(), "role": role})
            self.entries.append((member, content, tarfile.REGTYPE))
        self.data = archive(self.entries)
        self.lock = {"version": 1, "runtime_version": "1.28.2", "sherpa_version": "1.13.8",
                     "source": "https://release.invalid/", "targets": {TARGET: {
                         "archive": "fixed.tar.bz2", "size": len(self.data),
                         "sha256": hashlib.sha256(self.data).hexdigest(), "files": self.files}}}
        self.lock_path = self.root / "lock.json"
        self.lock_path.write_text(json.dumps(self.lock))
        self.cache.mkdir()
        (self.cache / "fixed.tar.bz2").write_bytes(self.data)
        self.addCleanup(patch.stopall)
        patch.object(runtime, "LOCK", self.lock_path).start()

    def test_host_excludes_speech_and_import_libraries(self):
        with patch.object(runtime, "urlopen") as request:
            runtime.prepare(TARGET, self.output, self.cache)
        request.assert_not_called()
        self.assertEqual({path.name for path in self.output.iterdir()},
                         {"libonnxruntime.dylib", runtime.MANIFEST})
        metadata = json.loads((self.output / runtime.MANIFEST).read_text())
        self.assertIsNone(metadata["sherpa_version"])

    def test_cargo_populates_tests_and_examples(self):
        runtime.prepare(TARGET, self.output, self.cache, speech=True, cargo=True, link=True)
        for directory in (self.output, self.output / "deps", self.output / "examples"):
            for file, (_, content, _) in zip(self.files, self.entries):
                self.assertEqual((directory / file["name"]).read_bytes(), content)
            metadata = json.loads((directory / runtime.MANIFEST).read_text())
            self.assertEqual({file["name"] for file in metadata["files"]},
                             {"libonnxruntime.dylib", "libsherpa-onnx-c-api.dylib"})

    def test_bundle_keeps_metadata_outside_code(self):
        output = self.root / "Sailry.app/Contents/MacOS"
        output.mkdir(parents=True)
        (output / runtime.MANIFEST).write_text("generated metadata from the earlier package")
        runtime.prepare(TARGET, output, self.cache, speech=True)
        self.assertEqual({path.name for path in output.iterdir()},
                         {"libonnxruntime.dylib", "libsherpa-onnx-c-api.dylib"})
        metadata = json.loads((output.parent / "Resources" / runtime.MANIFEST).read_text())
        self.assertEqual(metadata["target"], TARGET)
        self.assertEqual(metadata["sherpa_version"], "1.13.8")
        self.assertEqual({file["name"] for file in metadata["files"]},
                         {"libonnxruntime.dylib", "libsherpa-onnx-c-api.dylib"})

    def test_bundle_cleans_old_metadata_after_signing(self):
        output = self.root / "Sailry.app/Contents/MacOS"
        runtime.prepare(TARGET, output, self.cache, speech=True)
        old = output / runtime.MANIFEST
        old.write_text("generated metadata from the earlier package")
        library = output / self.files[0]["name"]
        library.write_bytes(library.read_bytes() + b"signature fixture")
        runtime.finalize(TARGET, output, speech=True)
        self.assertFalse(old.exists())
        metadata = json.loads((output.parent / "Resources" / runtime.MANIFEST).read_text())
        recorded = metadata["files"][0]
        self.assertEqual(recorded["sha256"], runtime.digest(library))
        self.assertEqual(recorded["size"], library.stat().st_size)
        self.assertEqual(recorded["source_sha256"], self.files[0]["sha256"])
        self.assertNotEqual(recorded["sha256"], recorded["source_sha256"])

    def test_reuses_only_verified_source_files(self):
        source = self.root / "source"
        source.mkdir()
        (source / self.files[0]["name"]).write_bytes(self.entries[0][1])
        with patch.object(runtime, "urlopen") as request:
            runtime.prepare(TARGET, self.output, self.root / "missing", source=source)
        request.assert_not_called()
        self.assertEqual((self.output / self.files[0]["name"]).read_bytes(), self.entries[0][1])

    def test_rejects_modified_source_without_network_fallback(self):
        source = self.root / "source"
        source.mkdir()
        (source / self.files[0]["name"]).write_bytes(b"different runtime")
        with patch.object(runtime, "urlopen") as request:
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                runtime.prepare(TARGET, self.output, self.cache, source=source)
        request.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_checks_archive_before_parsing_or_writing(self):
        (self.cache / "fixed.tar.bz2").write_bytes(b"invalid archive")
        with patch.object(runtime.tarfile, "open") as parse:
            with self.assertRaisesRegex(ValueError, "archive checksum mismatch"):
                runtime.prepare(TARGET, self.output, self.cache)
        parse.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_checks_download_before_caching(self):
        (self.cache / "fixed.tar.bz2").unlink()
        with patch.object(runtime, "urlopen", return_value=io.BytesIO(b"incorrect download")):
            with self.assertRaisesRegex(ValueError, "archive checksum mismatch"):
                runtime.prepare(TARGET, self.output, self.cache)
        self.assertFalse((self.cache / "fixed.tar.bz2").exists())
        self.assertFalse(self.output.exists())

    def test_refreshes_manifest_after_native_signing(self):
        runtime.prepare(TARGET, self.output, self.cache, speech=True)
        library = self.output / self.files[0]["name"]
        library.write_bytes(library.read_bytes() + b"signature fixture")
        runtime.finalize(TARGET, self.output, speech=True)
        metadata = json.loads((self.output / runtime.MANIFEST).read_text())
        recorded = metadata["files"][0]
        self.assertEqual(recorded["source_sha256"], self.files[0]["sha256"])
        self.assertEqual(recorded["sha256"], runtime.digest(library))
        self.assertNotEqual(recorded["sha256"], recorded["source_sha256"])
        self.assertEqual(metadata["source"]["sha256"], self.lock["targets"][TARGET]["sha256"])

    def test_rejects_symlink_destination(self):
        self.output.mkdir()
        outside = self.root / "outside"
        outside.write_bytes(b"preserve")
        (self.output / self.files[0]["name"]).symlink_to(outside)
        with self.assertRaisesRegex(ValueError, "destination is a symlink"):
            runtime.prepare(TARGET, self.output, self.cache)
        self.assertEqual(outside.read_bytes(), b"preserve")

    def test_closes_temporary_file_before_replacement(self):
        create = runtime.tempfile.NamedTemporaryFile
        pending = []
        def temporary(**kwargs):
            file = create(**kwargs)
            pending.append(file)
            return file
        replace = runtime.os.replace
        def move(source, destination):
            self.assertTrue(pending[-1].closed)
            replace(source, destination)
        with patch.object(runtime.tempfile, "NamedTemporaryFile", side_effect=temporary), \
                patch.object(runtime.os, "replace", side_effect=move):
            runtime.atomic(self.output / "fixture", b"fixture")
        self.assertEqual((self.output / "fixture").read_bytes(), b"fixture")
        if os.name != "nt":
            self.assertEqual((self.output / "fixture").stat().st_mode & 0o777, 0o644)

    def test_rejects_unsupported_target(self):
        with self.assertRaisesRegex(ValueError, "Unsupported native runtime target"):
            runtime.prepare("unknown-target", self.output, self.cache)

    def test_extracts_whitelist_without_executing_archive_entries(self):
        entries = self.entries + [("../../outside", b"untrusted executable", tarfile.REGTYPE)]
        files = runtime.extract(archive(entries), self.files[:1])
        self.assertEqual(files, {self.files[0]["name"]: self.entries[0][1]})
        self.assertFalse((self.root / "outside").exists())

    def test_rejects_duplicate_link_and_changed_entries(self):
        variations = [self.entries + [self.entries[0]],
                      [(self.entries[0][0], b"", tarfile.SYMTYPE), *self.entries[1:]],
                      [(self.entries[0][0], b"different runtime", tarfile.REGTYPE), *self.entries[1:]]]
        for entries in variations:
            with self.subTest(entries=entries):
                with self.assertRaises(ValueError):
                    runtime.extract(archive(entries), self.files[:1])

    def test_rejects_output_traversal_names(self):
        for name in ("../library", "nested/library", "nested\\library"):
            with self.subTest(name=name):
                file = {**self.files[0], "name": name}
                with self.assertRaisesRegex(ValueError, "output name is invalid"):
                    runtime.extract(self.data, [file])


class Lock(unittest.TestCase):
    def test_pins_six_native_targets_and_full_checksums(self):
        lock = json.loads((ROOT / "scripts/package/onnx-runtime.lock.json").read_text())
        self.assertEqual(lock["runtime_version"], "1.28.2")
        self.assertEqual(lock["sherpa_version"], "1.13.8")
        self.assertEqual(set(lock["targets"]), {
            "aarch64-apple-darwin", "x86_64-apple-darwin", "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu", "aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc"})
        for asset in lock["targets"].values():
            self.assertRegex(asset["sha256"], "^[0-9a-f]{64}$")
            self.assertGreater(asset["size"], 0)
            self.assertTrue(any(file["role"] == "runtime" for file in asset["files"]))
            self.assertTrue(any(file["role"] == "speech" for file in asset["files"]))
            for file in asset["files"]:
                self.assertRegex(file["sha256"], "^[0-9a-f]{64}$")
                self.assertGreater(file["size"], 0)


if __name__ == "__main__":
    unittest.main()
