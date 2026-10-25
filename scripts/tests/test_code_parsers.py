"""Parser fixtures use isolated synthetic files and never download or compile grammars."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path, PureWindowsPath
import shutil
import tarfile
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/package/code-parsers.py"
spec = importlib.util.spec_from_file_location("code_parsers", SCRIPT)
parsers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parsers)
ci_spec = importlib.util.spec_from_file_location("ci", ROOT / "scripts/ci.py")
ci = importlib.util.module_from_spec(ci_spec)
ci_spec.loader.exec_module(ci)
TARGET = "x86_64-unknown-linux-gnu"


def fixture(root, output=None):
    script = root / "scripts/package/code-parsers.py"
    script.parent.mkdir(parents=True)
    shutil.copyfile(SCRIPT, script)
    content = b"isolated query archive fixture"
    asset = {"archive": "query sources.tar.zst", "size": len(content),
             "sha256": hashlib.sha256(content).hexdigest()}
    lock = {"version": 1, "language_pack": "1.18.0", "tree_sitter": "0.26.13",
            "source": "https://fixtures.invalid/", "queries": asset,
            "targets": {TARGET: {"sha256": "a" * 64}}}
    path = script.with_name("code-parsers.lock.json")
    path.write_text(json.dumps(lock))
    cache = root / ".runtime/code-index/resources"
    cache.mkdir(parents=True)
    (cache / asset["archive"]).write_bytes(content)
    if output is not None:
        output.mkdir(parents=True)
        files = []
        for index in range(371):
            name = f"libtree_sitter_fixture_{index}.so"
            (output / name).write_bytes(b"synthetic grammar fixture")
            files.append({"name": name, "size": (output / name).stat().st_size,
                          "sha256": parsers.digest(output / name)})
        manifest = {"version": 1, "target": TARGET, "language_pack": lock["language_pack"],
                    "tree_sitter": lock["tree_sitter"], "source_sha256": asset["sha256"],
                    "archive_sha256": lock["targets"][TARGET]["sha256"],
                    "languages": [f"fixture_{index}" for index in range(371)], "files": files}
        (output / parsers.MANIFEST).write_text(json.dumps(manifest))
    return lock, path, cache


class Sources(unittest.TestCase):
    def test_uses_native_absolute_paths_and_verified_sidecar(self):
        with tempfile.TemporaryDirectory(prefix="parser fixture ") as temporary:
            root = Path(temporary)
            lock, path, cache = fixture(root)
            with patch.object(parsers, "LOCK", path), patch.object(parsers, "urlopen") as request:
                url = parsers.prepare(None, None, cache, source_only=True)
            archive = cache / lock["queries"]["archive"]
            self.assertEqual(url, "file://" + str(archive))
            self.assertNotIn("%20", url)
            self.assertEqual(archive.with_name(archive.name + ".sha256").read_text(),
                             lock["queries"]["sha256"] + "  " + archive.name + "\n")
            request.assert_not_called()

    def test_preserves_windows_drive_paths(self):
        path = PureWindowsPath("C:/source cache/query sources.tar.zst")
        self.assertEqual(parsers.bundle_url(path), "file://" + str(path))
        self.assertEqual(parsers.bundle_url(path).removeprefix("file://"), str(path))

    def test_rejects_corrupt_cached_sources(self):
        with tempfile.TemporaryDirectory() as temporary:
            lock, path, cache = fixture(Path(temporary))
            archive = cache / lock["queries"]["archive"]
            archive.write_bytes(b"corrupted query archive fixture")
            with patch.object(parsers, "LOCK", path), patch.object(parsers, "urlopen") as request:
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    parsers.prepare(None, None, cache, source_only=True)
            request.assert_not_called()


class Resources(unittest.TestCase):
    def test_reuses_verified_staging_without_archive_download(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "parsers"
            lock, path, cache = fixture(root, output)
            with patch.object(parsers, "LOCK", path), patch.object(parsers, "urlopen") as request:
                parsers.prepare(TARGET, output, cache)
                manifest = parsers.staged(TARGET, output, lock)
            self.assertEqual(len(manifest["files"]), 371)
            request.assert_not_called()

    def test_finalizes_hashes_after_native_signing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "parsers"
            lock, path, _ = fixture(root, output)
            library = output / "libtree_sitter_fixture_0.so"
            library.write_bytes(library.read_bytes() + b"synthetic signature fixture")
            with patch.object(parsers, "LOCK", path):
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    parsers.staged(TARGET, output, lock)
                parsers.finalize(TARGET, output)
                manifest = parsers.staged(TARGET, output, lock)
            record = next(item for item in manifest["files"] if item["name"] == library.name)
            self.assertEqual(record["sha256"], parsers.digest(library))
            self.assertEqual(manifest["archive_sha256"], lock["targets"][TARGET]["sha256"])

    def test_requires_the_complete_pinned_inventory(self):
        files = {f"tree_sitter_fixture_{index}.dll": b"" for index in range(371)}
        self.assertEqual(parsers.validate(files, "dll"), files)
        files.pop("tree_sitter_fixture_0.dll")
        with self.assertRaisesRegex(ValueError, "371 pinned grammars"):
            parsers.validate(files, "dll")

    def test_rejects_nested_archive_entries(self):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w") as archive:
            item = tarfile.TarInfo("nested/tree_sitter_fixture.so")
            item.size = 1
            archive.addfile(item, io.BytesIO(b"x"))
        buffer.seek(0)
        stream = tarfile.open(fileobj=buffer, mode="r:")
        with patch.object(parsers.tarfile, "open", return_value=stream):
            with self.assertRaisesRegex(ValueError, "invalid entry"):
                parsers.libraries(Path("synthetic.tar.zst"), "so")


class BuildEnvironment(unittest.TestCase):
    def test_clears_query_and_static_grammar_overrides(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock, _, cache = fixture(root)
            original = {"TSLP_OFFLINE": "1", "TSLP_LANGUAGES": "all", "RUSTFLAGS": "fixture flags"}
            with patch("urllib.request.urlopen", side_effect=AssertionError("fixture download")):
                environment = ci.parser_environment(root, original, libraries=False)
            self.assertNotIn("TSLP_OFFLINE", environment)
            self.assertNotIn("TSLP_LANGUAGES", environment)
            self.assertEqual(environment["RUSTFLAGS"], original["RUSTFLAGS"])
            self.assertEqual(environment["TSLP_SOURCE_BUNDLE_URL"],
                             "file://" + str(cache / lock["queries"]["archive"]))
            self.assertEqual(original["TSLP_OFFLINE"], "1")

    def test_honors_shared_target_directory_and_target_triple(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "build cache" / TARGET / "debug/code-parsers"
            fixture(root, output)
            with patch("urllib.request.urlopen", side_effect=AssertionError("fixture download")):
                environment = ci.parser_environment(root, {"CARGO_TARGET_DIR": "build cache",
                                                           "CARGO_BUILD_TARGET": TARGET}, libraries=True)
            self.assertEqual(environment["CARGO_TARGET_DIR"], "build cache")
            self.assertTrue((output / parsers.MANIFEST).is_file())

    def test_prepares_libraries_only_for_node_execution(self):
        for name, libraries in [("backend", True), ("desktop", True), ("host", True),
                                ("lint", False), ("desktop-production", False), ("host-production", False)]:
            with self.subTest(name=name), patch.object(ci, "parser_environment", return_value={}) as prepare, \
                    patch.object(ci.subprocess, "run") as command:
                command.return_value.returncode = 1
                selection = ci.outputs({"shared"}) if name == "lint" else None
                self.assertEqual(ci.run_check(name, selection=selection), 1)
                prepare.assert_called_once_with(ROOT, None, libraries=libraries)


if __name__ == "__main__":
    unittest.main()
