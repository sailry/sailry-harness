"""Public ad-hoc downloads remain separate from signed update assets."""

import contextlib
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("desktop_adhoc", ROOT / "scripts/package-desktop-adhoc.py")
adhoc = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(adhoc)


class Packaging(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="sailry-adhoc-test-")
        self.addCleanup(temporary.cleanup)
        self.source = Path(temporary.name).resolve() / "product checkout"
        self.directory = self.source / "dist/macos-release-fixture"
        self.directory.mkdir(parents=True)
        (self.directory / "Sailry.app").mkdir()

    def assets(self, target):
        for extension in ("dmg", "zip"):
            (self.directory / f"Sailry-{target}.{extension}").write_bytes(extension.encode())
        (self.directory / "SHA256SUMS").write_text("original package checksums\n")

    def run_package(self, target, stdout=None):
        result = subprocess.CompletedProcess([], 0, stdout=(stdout if stdout is not None
                    else f"Packaging diagnostics\nLocal package: {self.directory}\n"))
        with patch.object(adhoc.subprocess, "run", return_value=result) as run:
            with contextlib.redirect_stdout(io.StringIO()):
                output = adhoc.package(self.source, target)
        return output, run.call_args

    def test_separates_each_target(self):
        for target in sorted(adhoc.TARGETS):
            with self.subTest(target=target):
                self.assets(target)
                output, call = self.run_package(target)
                expected = {f"Sailry-{target}-adhoc.{extension}" for extension in ("dmg", "zip")}
                checksum = f"SHA256SUMS-desktop-{target}-adhoc"
                self.assertEqual({path.name for path in output.iterdir()}, expected | {checksum})
                rows = (output / checksum).read_text().splitlines()
                self.assertEqual(len(rows), 2)
                for row in rows:
                    digest, name = row.split("  ")
                    self.assertIn(name, expected)
                    self.assertEqual(digest, hashlib.sha256((output / name).read_bytes()).hexdigest())
                self.assertEqual(call.args[0], ["bash", str(self.source / "scripts/package-macos.sh"),
                                                "release", "desktop"])
                self.assertEqual(call.kwargs["cwd"], self.source)
                self.assertTrue(call.kwargs["check"])
                self.assertTrue((self.directory / "Sailry.app").is_dir())

    def test_removes_signing_secrets(self):
        secrets = {name: "secret" for name in ("APPLE_API_PRIVATE_KEY", "APPLE_CERTIFICATE_P12",
                   "SAILRY_UPDATE_SIGNING_KEY", "NOTARYTOOL_PROFILE", "TAURI_SIGNING_PRIVATE_KEY",
                   "KEYCHAIN_PASSWORD", "MACOS_SIGNING_IDENTITY")}
        with patch.dict(os.environ, secrets | {"SAILRY_SIGNING": "developer-id", "SAILRY_NOTARIZE": "1"}):
            values = adhoc.environment()
        self.assertTrue(set(secrets).isdisjoint(values))
        self.assertEqual(values["SAILRY_SIGNING"], "ad-hoc")
        self.assertEqual(values["SAILRY_NOTARIZE"], "0")
        self.assertEqual(values["PATH"], os.environ["PATH"])

    def test_runs_product_packager(self):
        target = "aarch64-apple-darwin"
        self.assets(target)
        scripts = self.source / "scripts"
        scripts.mkdir()
        (scripts / "package-macos.sh").write_text(
            '#!/bin/bash\nset -eu\n'
            'test "$1" = release\ntest "$2" = desktop\n'
            'test "$SAILRY_SIGNING" = ad-hoc\ntest "$SAILRY_NOTARIZE" = 0\n'
            'test -z "${APPLE_API_PRIVATE_KEY+x}"\n'
            'test -z "${SAILRY_UPDATE_SIGNING_KEY+x}"\n'
            f"printf 'Local package: %s\\n' {shlex.quote(str(self.directory))}\n")
        with patch.dict(os.environ, {"APPLE_API_PRIVATE_KEY": "private", "SAILRY_UPDATE_SIGNING_KEY": "private"}):
            with contextlib.redirect_stdout(io.StringIO()):
                output = adhoc.package(self.source, target)
        self.assertEqual(len(list(output.iterdir())), 3)

    def test_rejects_extra_or_linked_assets(self):
        target = "aarch64-apple-darwin"
        self.assets(target)
        unexpected = self.directory / f"Sailry-{target}.zip.update.json"
        unexpected.write_text("signed updates must remain separate")
        with self.assertRaises(ValueError):
            self.run_package(target)
        unexpected.unlink()
        archive = self.directory / f"Sailry-{target}.zip"
        archive.unlink()
        archive.symlink_to(self.directory / "SHA256SUMS")
        with self.assertRaises(ValueError):
            self.run_package(target)

    def test_rejects_ambiguous_or_external_output(self):
        target = "aarch64-apple-darwin"
        self.assets(target)
        for stdout in ("no output", f"Local package: {self.directory}\nLocal package: {self.directory}\n",
                       f"Local package: {self.source}\n", "Local package: dist/relative\n"):
            with self.subTest(stdout=stdout), self.assertRaises(ValueError):
                self.run_package(target, stdout)

    def test_rejects_wrong_target_before_packaging(self):
        with patch.object(adhoc.subprocess, "run") as run:
            with self.assertRaises(ValueError):
                adhoc.package(self.source, "x86_64-unknown-linux-gnu")
            run.assert_not_called()

    def test_propagates_packaging_failure(self):
        with patch.object(adhoc.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "bash")):
            with self.assertRaises(subprocess.CalledProcessError):
                adhoc.package(self.source, "aarch64-apple-darwin")


if __name__ == "__main__":
    unittest.main()
