"""APK fixtures validate packaging checks, not Android device acceptance."""

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify_android", ROOT / "scripts/package/verify-android.py")
checking = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checking)
VERSION = "0.1.0-alpha.1"
FINGERPRINT = "ab" * 32
BADGING = f"package: name='com.sailry.sailry_mobile' versionCode='1' versionName='{VERSION}'\nminSdkVersion:'24'\ntargetSdkVersion:'36'\n"


class Metadata(unittest.TestCase):
    def test_requires_exact_identity_and_release_versions(self):
        checking.metadata(BADGING, VERSION, 1, "com.sailry.sailry_mobile")
        for text in (BADGING.replace(VERSION, "0.1.0"), BADGING.replace("versionCode='1'", "versionCode='2'"),
                     BADGING.replace("com.sailry.sailry_mobile", "com.sailry.sailry_mobile.acceptance"),
                     BADGING.replace("minSdkVersion:'24'", "minSdkVersion:'26'"),
                     BADGING.replace("targetSdkVersion:'36'", "targetSdkVersion:'35'"),
                     BADGING + "application-debuggable\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                checking.metadata(text, VERSION, 1, "com.sailry.sailry_mobile")

    def test_rejects_missing_and_wrong_signers(self):
        text = f"Signer #1 certificate SHA-256 digest: {FINGERPRINT}\n"
        checking.certificate(text, FINGERPRINT.upper())
        checking.certificate(text, None)
        for output in ("", text.replace(FINGERPRINT, "cd" * 32), text + text.replace("#1", "#2")):
            with self.subTest(output=output), self.assertRaises(ValueError):
                checking.certificate(output, FINGERPRINT)


class NativeLibraries(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.apk = Path(directory.name) / "fixture.apk"

    def write(self, libraries, extra=None):
        with zipfile.ZipFile(self.apk, "w") as archive:
            for name, contents in libraries.items():
                archive.writestr(f"lib/arm64-v8a/{name}", contents)
            if extra is not None:
                archive.writestr(extra, b"fixture")

    def test_requires_every_nonempty_arm64_library(self):
        libraries = {name: b"fixture" for name in checking.LIBRARIES}
        self.write(libraries)
        checking.native(self.apk)
        for name in checking.LIBRARIES:
            self.write({**libraries, name: b""})
            with self.subTest(name=name), self.assertRaises(ValueError):
                checking.native(self.apk)

    def test_rejects_extra_architectures(self):
        self.write({name: b"fixture" for name in checking.LIBRARIES}, "lib/x86_64/libflutter.so")
        with self.assertRaises(ValueError):
            checking.native(self.apk)

    def test_verifies_signature_for_declared_platforms(self):
        self.write({name: b"fixture" for name in checking.LIBRARIES})
        with patch.object(checking, "run", side_effect=[BADGING, f"Signer #1 certificate SHA-256 digest: {FINGERPRINT}\n"]) as run:
            checking.verify(self.apk, VERSION, 1, "com.sailry.sailry_mobile", Path("tools"), FINGERPRINT)
        self.assertEqual(run.call_args_list[1].args, (Path("tools/apksigner"), "verify", "--print-certs", self.apk))


class DependencyBoundary(unittest.TestCase):
    def test_reuses_build_without_skipping_dependency_check(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cargo = root / "cargo"
            cargo.write_text("#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TASK_CALLS\"\nprintf '%s\\n' \"$TASK_DEPENDENCIES\"\n")
            cargo.chmod(0o755)
            calls = root / "calls"
            for dependency, success in (("sailry-client v0.1.0", True),
                                        ("sailry-node-runtime v0.1.0", False)):
                with self.subTest(dependency=dependency):
                    calls.write_text("")
                    result = subprocess.run(
                        ["bash", str(ROOT / "scripts/check-mobile-boundary.sh"),
                         "aarch64-apple-ios", "sailry-mobile-bridge", "--dependencies-only"],
                        env={**os.environ, "PATH": str(root) + os.pathsep + os.environ["PATH"],
                             "TASK_CALLS": str(calls), "TASK_DEPENDENCIES": dependency},
                        capture_output=True, text=True,
                    )
                    self.assertEqual(result.returncode == 0, success, result.stderr)
                    invoked = calls.read_text().splitlines()
                    self.assertEqual(len(invoked), 1)
                    self.assertTrue(invoked[0].startswith("tree --locked "))


if __name__ == "__main__":
    unittest.main()
