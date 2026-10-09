"""macOS packaging fixtures never use developer certificates or Apple services."""

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from test_python_runtime import archive as python_archive

ROOT = Path(__file__).resolve().parents[2]


def load(name, filename=None):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts/package" / (filename or f"{name}-macos.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


signing = load("sign")
notary = load("notarize")
dmg = load("dmg", "dmg.py")


class CodeSigning(unittest.TestCase):
    def test_finds_macho_without_symlinks_or_linux(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "nested").mkdir()
            (root / "program").write_bytes(b"\xcf\xfa\xed\xfe" + b"fixture")
            (root / "nested/lib.so").write_bytes(b"\xca\xfe\xba\xbe" + b"fixture")
            (root / "linux").write_bytes(b"\x7fELF" + b"fixture")
            (root / "link").symlink_to("program")
            self.assertEqual(signing.mach_objects(root), [root / "nested/lib.so", root / "program"])

    def test_signs_separate_host_and_nested_native_code(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / "Sailry.app"
            resources = app / "Contents/Resources"
            main = app / "Contents/MacOS/sailry-desktop"
            dependency = app / "Contents/Frameworks/fixture.dylib"
            native = app / "Contents/MacOS/libonnxruntime.dylib"
            speech = app / "Contents/MacOS/libsherpa-onnx-c-api.dylib"
            host = root / "host"
            host.mkdir()
            for path in (main, dependency, native, speech, host / "sailry-host", host / "libonnxruntime.dylib"):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"\xcf\xfa\xed\xfe" + b"fixture")
            interpreter = b"\xcf\xfa\xed\xfeinterpreter fixture"
            extension = b"\xcf\xfa\xed\xfeextension fixture"
            data = python_archive(
                [("python/lib/python3.12/fixture.so", extension, None)], interpreter=interpreter)
            cache = root / "cache"
            cache.mkdir()
            (cache / "python.tar.gz").write_bytes(data)
            lock = root / "python.lock.json"
            digest = hashlib.sha256(data).hexdigest()
            lock.write_text(json.dumps({"version": 1, "python_version": "3.12.13", "targets": {
                "aarch64-apple-darwin": {"url": "https://release.invalid/python.tar.gz",
                                          "archive": "python.tar.gz", "size": len(data),
                                          "sha256": digest}}}))
            outputs = [app / "Contents/MacOS", host]
            python_objects = [signing.python_runtime.runtime_output(output) / "python" / name for output in outputs
                              for name in ["bin/python3.12", "lib/python3.12/fixture.so"]]
            def command(*args):
                path = args[-1]
                if path.is_file() and (path.suffix == ".dylib" or path in python_objects):
                    path.write_bytes(path.read_bytes() + b"signed fixture")
                if path == app:
                    recorded = json.loads((resources / "onnx-runtime.json").read_text())
                    self.assertEqual(recorded["files"][0]["sha256"], signing.runtime.digest(native))
                    receipt = json.loads((app / "Contents/MacOS/python/python-runtime.json").read_text())
                    child = next(file for file in receipt["files"] if file["path"] == "bin/python3.12")
                    self.assertEqual(child["sha256"], signing.python_runtime.digest(python_objects[0]))
                return ""
            with patch.object(signing.python_runtime, "LOCK", lock), \
                    patch.object(signing.python_runtime, "urlopen") as request, \
                    patch.object(signing, "run", side_effect=command) as run, \
                    patch.object(signing, "verify") as verify:
                for output in outputs:
                    signing.python_runtime.prepare("aarch64-apple-darwin", output, cache)
                signing.sign(app, host, "aarch64-apple-darwin", "Fixture identity", "FIXTURE", root / "keychain")
            request.assert_not_called()
            calls = [call.args for call in run.call_args_list]
            app_call = next(call for call in calls if call[-1] == app)
            self.assertEqual(calls[-1][-1], host / "sailry-host")
            self.assertTrue(all("--deep" not in call for call in calls))
            self.assertTrue(all("--timestamp" in call and "runtime" in call for call in calls))
            self.assertTrue(all("--keychain" in call for call in calls))
            self.assertIn("--entitlements", app_call)
            self.assertEqual(app_call[app_call.index("--entitlements") + 1],
                             ROOT / "apps/desktop/macos/entitlements.plist")
            for path in python_objects:
                call = next(call for call in calls if call[-1] == path)
                verify.assert_any_call(path, "FIXTURE")
                if path.name == "python3.12":
                    self.assertEqual(call[call.index("--entitlements") + 1],
                                     str(ROOT / "scripts/package/python.entitlements.plist"))
                else:
                    self.assertNotIn("--entitlements", call)
            self.assertTrue(all("--entitlements" not in call for call in calls
                                if call[-1] != app and call[-1] not in python_objects))
            for output in outputs:
                python = output / "python"
                receipt = json.loads((python / "python-runtime.json").read_text())
                self.assertEqual(receipt["python_version"], "3.12.13")
                self.assertEqual(receipt["target"], "aarch64-apple-darwin")
                self.assertEqual(receipt["source"]["sha256"], digest)
                self.assertFalse(python.stat().st_mode & 0o200)
                self.assertTrue((python / "lib/python3.12/EXTERNALLY-MANAGED").is_file())
                self.assertEqual((python / "bin/python3").resolve(), (python / "bin/python3.12").resolve())
                for name, content in [("bin/python3.12", interpreter),
                                      ("lib/python3.12/fixture.so", extension)]:
                    expected = content + b"signed fixture"
                    file = next(file for file in receipt["files"] if file["path"] == name)
                    self.assertEqual((python / name).read_bytes(), expected)
                    self.assertEqual(file["sha256"], signing.python_runtime.digest(python / name))
                    self.assertEqual(file["size"], len(expected))
                    self.assertFalse((python / name).stat().st_mode & 0o200)
            self.assertFalse((resources / "hosts").exists())
            self.assertFalse((resources / "office-runtime").exists())
            self.assertFalse((host / "office-runtime").exists())
            verify.assert_any_call(dependency, "FIXTURE")
            verify.assert_any_call(native, "FIXTURE")
            verify.assert_any_call(speech, "FIXTURE")
            verify.assert_any_call(host / "libonnxruntime.dylib", "FIXTURE")
            verify.assert_any_call(app, "FIXTURE", deep=True)
            verify.assert_any_call(host / "sailry-host", "FIXTURE")
            recorded = json.loads((host / "onnx-runtime.json").read_text())
            self.assertEqual(recorded["files"][0]["sha256"], signing.runtime.digest(host / "libonnxruntime.dylib"))
            self.assertIsNone(recorded["sherpa_version"])

    def test_rejects_wrong_team_and_incomplete_metadata(self):
        for metadata in ("TeamIdentifier=OTHER\nflags=0x10000(runtime)\nTimestamp=now\n",
                         "TeamIdentifier=FIXTURE\nTimestamp=now\n",
                         "TeamIdentifier=FIXTURE\nflags=0x10000(runtime)\n"):
            with self.subTest(metadata=metadata), patch.object(signing, "run", return_value=metadata):
                with self.assertRaises(RuntimeError):
                    signing.verify(Path("fixture"), "FIXTURE")

    def test_accepts_verified_distribution_metadata(self):
        metadata = "TeamIdentifier=FIXTURE\nflags=0x10000(runtime)\nTimestamp=now\n"
        with patch.object(signing, "run", return_value=metadata) as run:
            signing.verify(Path("fixture"), "FIXTURE", deep=True)
        self.assertEqual(run.call_args_list[0].args, ("codesign", "--verify", "--strict", "--deep", Path("fixture")))


class Notarization(unittest.TestCase):
    def test_staples_and_verifies_distribution_disk_image(self):
        image = Path("Sailry.dmg")
        with patch.object(notary, "run") as run, patch.object(notary, "submit") as submit:
            notary.notarize_disk_image(image, ["--key", "fixture.p8"])
        submit.assert_called_once_with(image, ["--key", "fixture.p8"])
        self.assertEqual([call.args[1:3] for call in run.call_args_list], [
            ("stapler", "staple"), ("stapler", "validate"), ("--verify", "--strict"),
            ("--assess", "--type"), ("verify", image),
        ])
        self.assertIn("context:primary-signature", run.call_args_list[-2].args)

    def test_disk_image_failure_does_not_staple(self):
        with patch.object(notary, "run") as run, patch.object(notary, "submit", side_effect=RuntimeError("Service unavailable")):
            with self.assertRaises(RuntimeError):
                notary.notarize_disk_image(Path("Sailry.dmg"), [])
        run.assert_not_called()
    def test_requires_acceptance(self):
        for status in ("Invalid", "In Progress", None):
            with self.subTest(status=status), patch.object(notary, "run", return_value=json.dumps({"status": status})):
                with self.assertRaisesRegex(RuntimeError, "did not succeed"):
                    notary.submit(Path("fixture.zip"), [])

    def test_reports_rejected_submission_issues(self):
        responses = [json.dumps({"status": "Invalid", "id": "fixture-id"}),
                     json.dumps({"issues": [{"path": "Sailry.app", "message": "Unsigned code"}]})]
        with patch.object(notary, "run", side_effect=responses) as run, patch("builtins.print") as output:
            with self.assertRaises(RuntimeError):
                notary.submit(Path("fixture.zip"), ["--key", "fixture.p8"])
        self.assertEqual(run.call_args_list[1].args[1:4], ("notarytool", "log", "fixture-id"))
        output.assert_called_once_with("Notarization issue: Sailry.app: Unsigned code")

    def test_staples_and_assesses_only_after_both_submissions(self):
        with patch.object(notary, "run") as run, patch.object(notary, "submit") as submit:
            notary.notarize(Path("Sailry.app"), Path("host"), ["--key", "fixture.p8"])
        self.assertEqual(submit.call_count, 2)
        self.assertEqual([call.args[1:3] for call in run.call_args_list], [
            ("-c", "-k"), ("-c", "-k"), ("stapler", "staple"),
            ("stapler", "validate"), ("--verify", "--deep"), ("--assess", "--type"),
        ])

    def test_does_not_staple_after_submission_failure(self):
        with patch.object(notary, "run") as run, \
                patch.object(notary, "submit", side_effect=RuntimeError("Service unavailable")):
            with self.assertRaises(RuntimeError):
                notary.notarize(Path("Sailry.app"), Path("host"), [])
        self.assertEqual(run.call_count, 1)


class DiskImage(unittest.TestCase):
    def test_contains_the_app_and_applications_link(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / "Sailry.app"
            app.mkdir()
            (app / "fixture").write_bytes(b"fixture")
            output = root / "Sailry.dmg"
            def command(*arguments):
                if arguments[1] == "create":
                    stage = Path(arguments[arguments.index("-srcfolder") + 1])
                    self.assertEqual((stage / "Applications").readlink(), Path("/Applications"))
                    self.assertEqual((stage / "Sailry.app/fixture").read_bytes(), b"fixture")
                    output.write_bytes(b"disk image fixture")
            with patch.object(dmg, "run", side_effect=command) as run:
                dmg.create(app, output)
            self.assertEqual(run.call_args_list[-1].args, ("hdiutil", "verify", output))
            self.assertEqual((app / "fixture").read_bytes(), b"fixture")
            with self.assertRaises(ValueError):
                dmg.create(app, output)

    def test_distribution_image_is_signed_before_verification(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / "Sailry.app"
            app.mkdir()
            with patch.object(dmg, "run") as run, patch.dict(dmg.os.environ, {"APPLE_SIGNING_IDENTITY":"Fixture identity", "APPLE_SIGNING_KEYCHAIN":"Fixture keychain"}):
                dmg.create(app, root / "Sailry.dmg", "developer-id")
            calls = [call.args for call in run.call_args_list]
            self.assertEqual([call[0] for call in calls], ["hdiutil", "codesign", "codesign", "hdiutil"])
            self.assertIn("--timestamp", calls[1])
            self.assertIn("ai.sailry.disk-image", calls[1])
            self.assertIn("--keychain", calls[1])


if __name__ == "__main__":
    unittest.main()
