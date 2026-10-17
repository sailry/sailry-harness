"""macOS packaging fixtures never use developer certificates or Apple services."""

from contextlib import nullcontext
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
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
    def test_selects_one_package_without_its_counterpart(self):
        for desktop in (True, False):
            with self.subTest(desktop=desktop), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                app = root / "Sailry.app" if desktop else None
                host = None if desktop else root / "host"
                output = app / "Contents/MacOS" if desktop else host
                executable = output / ("sailry-desktop" if desktop else "sailry-host")
                output.mkdir(parents=True)
                executable.write_bytes(b"\xcf\xfa\xed\xfe" + b"fixture")
                with patch.object(signing.python_runtime, "unlock") as unlock, \
                        patch.object(signing.python_runtime, "signing_options", return_value=[]), \
                        patch.object(signing.python_runtime, "finalize") as python_finalize, \
                        patch.object(signing.runtime, "finalize") as finalize, \
                        patch.object(signing, "run") as run, patch.object(signing, "verify") as verify:
                    signing.sign(app, host, "aarch64-apple-darwin", "Fixture identity", "FIXTURE")
                unlock.assert_called_once_with(output)
                finalize.assert_called_once_with("aarch64-apple-darwin", output, speech=desktop)
                python_finalize.assert_called_once_with("aarch64-apple-darwin", output)
                verify.assert_any_call(executable, "FIXTURE")
                self.assertEqual(run.call_count, 2 if desktop else 1)
                if desktop:
                    verify.assert_any_call(app, "FIXTURE", deep=True)
                    self.assertIn("--entitlements", run.call_args_list[-1].args)
                    self.assertFalse((root / "host").exists())
                else:
                    self.assertNotIn("--entitlements", run.call_args_list[-1].args)
                    self.assertFalse((root / "Sailry.app").exists())

    def test_registers_keychain_and_restores_search_list(self):
        keychain = Path("/temporary/signing.keychain-db")
        identity = "A" * 40
        replies = ['"/login.keychain-db"\n"/system.keychain"\n', "", f'1) {identity} "Fixture"', ""]
        with patch.object(signing, "run", side_effect=replies) as run:
            with signing.keychain_scope(keychain, identity):
                self.assertEqual(run.call_count, 3)
        self.assertEqual([call.args for call in run.call_args_list], [
            ("security", "list-keychains", "-d", "user"),
            ("security", "list-keychains", "-d", "user", "-s", "/login.keychain-db", "/system.keychain", keychain),
            ("security", "find-identity", "-v", "-p", "codesigning", keychain),
            ("security", "list-keychains", "-d", "user", "-s", "/login.keychain-db", "/system.keychain"),
        ])

    def test_missing_identity_restores_before_failure(self):
        with patch.object(signing, "run", side_effect=['"/login"', "", "0 valid identities found", ""]) as run:
            with self.assertRaisesRegex(RuntimeError, "identity is unavailable"):
                with signing.keychain_scope(Path("/temporary"), "A" * 40):
                    self.fail("An unavailable identity reached signing")
        self.assertEqual(run.call_args.args, ("security", "list-keychains", "-d", "user", "-s", "/login"))

    def test_restoration_preserves_signing_failure(self):
        replies = ['"/login"', "", '1) ' + "A" * 40 + ' "Fixture"', RuntimeError("restore failed")]
        with patch.object(signing, "run", side_effect=replies), patch.object(signing.sys, "stderr"):
            with self.assertRaisesRegex(RuntimeError, "signing failed"):
                with signing.keychain_scope(Path("/temporary"), "A" * 40):
                    raise RuntimeError("signing failed")

    def test_restoration_failure_remains_an_error(self):
        replies = ['"/login"', "", '1) ' + "A" * 40 + ' "Fixture"', RuntimeError("restore failed")]
        with patch.object(signing, "run", side_effect=replies):
            with self.assertRaisesRegex(RuntimeError, "restore failed"):
                with signing.keychain_scope(Path("/temporary"), "A" * 40):
                    pass

    def test_existing_keychain_keeps_other_search_entries(self):
        keychain = Path("/temporary")
        with patch.object(signing, "run", side_effect=['"/login" "/temporary"', '1) ' + "A" * 40 + ' "Fixture"', ""]) as run:
            with signing.keychain_scope(keychain, "a" * 40):
                pass
        self.assertEqual(run.call_count, 3)
        self.assertEqual(run.call_args.args, ("security", "list-keychains", "-d", "user", "-s", "/login", "/temporary"))


    def test_requires_a_package(self):
        with self.assertRaisesRegex(RuntimeError, "Select"):
            signing.sign(None, None, "aarch64-apple-darwin", "Fixture identity", "FIXTURE")

    def test_finds_macho_without_symlinks_or_linux(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "nested").mkdir()
            (root / "program").write_bytes(b"\xcf\xfa\xed\xfe" + b"fixture")
            (root / "nested/lib.so").write_bytes(b"\xca\xfe\xba\xbe" + b"fixture")
            (root / "linux").write_bytes(b"\x7fELF" + b"fixture")
            (root / "link").symlink_to("program")
            self.assertEqual(signing.mach_objects(root), [root / "nested/lib.so", root / "program"])

    def test_accepts_system_and_bundle_dependencies(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "native"
            paths = ["/usr/lib/libSystem.B.dylib", "/System/Library/Frameworks/AppKit.framework/AppKit",
                     "@rpath/libonnxruntime.dylib", "@loader_path/libpython3.12.dylib",
                     "@executable_path/libonnxruntime.dylib"]
            commands = "cmd LC_ID_DYLIB\n cmdsize 40\n name libfixture.dylib (offset 24)\n"
            commands += "".join(f"cmd LC_LOAD_DYLIB\n cmdsize 80\n name {value} (offset 24)\n"
                                for value in paths)
            commands += "cmd LC_RPATH\n cmdsize 32\n path @loader_path (offset 12)\n"
            with patch.object(signing, "mach_objects", return_value=[path]), \
                    patch.object(signing, "run", return_value=commands) as run:
                signing.verify_dependencies(root)
            self.assertEqual([call.args for call in run.call_args_list],
                             [("otool", "-l", path)])

    def test_rejects_external_nested_native_dependencies(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "python/lib/native.so"
            for value in ("/opt/homebrew/opt/xz/lib/liblzma.5.dylib",
                          "/usr/local/lib/liblzma.5.dylib", "/Users/builder/lib/native.dylib"):
                with self.subTest(value=value), \
                        patch.object(signing, "mach_objects", return_value=[path]), \
                        patch.object(signing, "run", return_value=
                                     f"cmd LC_LOAD_DYLIB\n cmdsize 80\n name {value} (offset 24)\n"):
                    with self.assertRaisesRegex(RuntimeError, "External native dependency in python/lib/native.so"):
                        signing.verify_dependencies(root)

    def test_rejects_external_runtime_search_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "native"
            commands = "cmd LC_RPATH\n cmdsize 64\n path /opt/homebrew/lib (offset 12)\n"
            with patch.object(signing, "mach_objects", return_value=[path]), \
                    patch.object(signing, "run", return_value=commands):
                with self.assertRaisesRegex(RuntimeError, "External native dependency in native"):
                    signing.verify_dependencies(root)

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
            with patch.object(signing, "keychain_scope", return_value=nullcontext()), \
                    patch.object(signing.python_runtime, "LOCK", lock), \
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
    def test_submits_only_the_selected_package(self):
        for desktop in (True, False):
            with self.subTest(desktop=desktop), patch.object(notary, "run") as run, \
                    patch.object(notary, "submit") as submit:
                app = Path("Sailry.app") if desktop else None
                host = None if desktop else Path("host")
                notary.notarize(app, host, ["--key", "fixture.p8"])
                self.assertEqual(submit.call_count, 1)
                selected = app if desktop else host
                self.assertEqual(run.call_args_list[0].args[-2], selected)
                self.assertEqual(submit.call_args.args[0].name, f"{selected.name}.zip")
                self.assertEqual(run.call_count, 5 if desktop else 1)
                if desktop:
                    self.assertEqual(run.call_args_list[-1].args[-1], app)

    def test_requires_a_package(self):
        with self.assertRaisesRegex(ValueError, "Select"):
            notary.notarize(None, None, [])

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



class TicketDownload(unittest.TestCase):
    def test_retries_only_interrupted_ticket_downloads(self):
        failure = RuntimeError('NSURLErrorDomain Code=-1005 "The network connection was lost."')
        for action in ("staple", "validate"):
            with self.subTest(action=action), patch.object(notary, "run", side_effect=[failure, None]) as run, \
                    patch.object(notary.time, "sleep") as sleep:
                notary.run_stapler(action, Path("Sailry.dmg"))
            self.assertEqual(run.call_count, 2)
            run.assert_called_with("xcrun", "stapler", action, Path("Sailry.dmg"))
            sleep.assert_called_once_with(2)

    def test_stops_after_bounded_attempts(self):
        failure = RuntimeError('NSURLErrorDomain Code=-1005 "The network connection was lost."')
        for action in ("staple", "validate"):
            with self.subTest(action=action), patch.object(notary, "run", side_effect=failure) as run, \
                    patch.object(notary.time, "sleep") as sleep:
                with self.assertRaises(RuntimeError) as raised:
                    notary.run_stapler(action, Path("Sailry.dmg"))
            self.assertIs(raised.exception, failure)
            self.assertEqual(run.call_count, 6)
            self.assertEqual([call.args for call in sleep.call_args_list], [(2,), (5,), (15,), (30,), (60,)])

    def test_recovers_after_longer_interruption(self):
        failure = RuntimeError('NSURLErrorDomain Code=-1005 "The network connection was lost."')
        for action in ("staple", "validate"):
            with self.subTest(action=action), patch.object(notary, "run", side_effect=[failure] * 5 + [None]) as run, \
                    patch.object(notary.time, "sleep") as sleep:
                notary.run_stapler(action, Path("Sailry.dmg"))
            self.assertEqual(run.call_count, 6)
            self.assertEqual([call.args for call in sleep.call_args_list], [(2,), (5,), (15,), (30,), (60,)])

    def test_preserves_other_failures(self):
        for action in ("staple", "validate"):
            for message in ("Invalid signature", "Ticket not found", "NSURLErrorDomain Code=-1001",
                            "Other domain -1005", "NSURLErrorDomain Code=-1005 invalid ticket",
                            "NSURLErrorDomain Code=-10050 The network connection was lost"):
                with self.subTest(action=action, message=message), \
                        patch.object(notary, "run", side_effect=RuntimeError(message)) as run, \
                        patch.object(notary.time, "sleep") as sleep:
                    with self.assertRaisesRegex(RuntimeError, message):
                        notary.run_stapler(action, Path("Sailry.dmg"))
                run.assert_called_once()
                sleep.assert_not_called()

    def test_verifies_after_recovered_ticket_download(self):
        failure = RuntimeError('NSURLErrorDomain Code=-1005 "The network connection was lost."')
        image = Path("Sailry.dmg")
        with patch.object(notary, "submit") as submit, \
                patch.object(notary, "run", side_effect=[failure, None, failure, None, None, None, None]) as run, \
                patch.object(notary.time, "sleep"):
            notary.notarize_disk_image(image, [])
        submit.assert_called_once_with(image, [])
        self.assertEqual([call.args for call in run.call_args_list], [
            ("xcrun", "stapler", "staple", image), ("xcrun", "stapler", "staple", image),
            ("xcrun", "stapler", "validate", image), ("xcrun", "stapler", "validate", image),
            ("codesign", "--verify", "--strict", image),
            ("spctl", "--assess", "--type", "open", "--context", "context:primary-signature", image),
            ("hdiutil", "verify", image)])

    def test_exhaustion_stops_the_validation_chain(self):
        failure = RuntimeError('NSURLErrorDomain Code=-1005 "The network connection was lost."')
        for action in ("staple", "validate"):
            responses = [failure] * 6 if action == "staple" else [None] + [failure] * 6
            with self.subTest(action=action), patch.object(notary, "submit"), \
                    patch.object(notary, "run", side_effect=responses) as run, \
                    patch.object(notary.time, "sleep"):
                with self.assertRaises(RuntimeError) as raised:
                    notary.notarize_disk_image(Path("Sailry.dmg"), [])
            self.assertIs(raised.exception, failure)
            self.assertEqual(run.call_count, len(responses))
            self.assertTrue(all(call.args[:2] == ("xcrun", "stapler") for call in run.call_args_list))
            self.assertEqual(run.call_args.args[2], action)


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
            with patch.object(dmg, "run") as run, \
                    patch.object(dmg.signing_tools, "keychain_scope", return_value=nullcontext()) as scope, \
                    patch.dict(dmg.os.environ, {"APPLE_SIGNING_IDENTITY":"Fixture identity", "APPLE_SIGNING_KEYCHAIN":"Fixture keychain"}):
                dmg.create(app, root / "Sailry.dmg", "developer-id")
            scope.assert_called_once_with("Fixture keychain", "Fixture identity")
            calls = [call.args for call in run.call_args_list]
            self.assertEqual([call[0] for call in calls], ["hdiutil", "codesign", "codesign", "hdiutil"])
            self.assertIn("--timestamp", calls[1])
            self.assertIn("ai.sailry.disk-image", calls[1])
            self.assertIn("--keychain", calls[1])

    def test_image_signing_restores_keychain_after_failure(self):
        for fails in (False, True):
            with self.subTest(fails=fails), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                app = root / "Sailry.app"
                app.mkdir()
                events = []
                def security(*arguments):
                    events.append(arguments)
                    if arguments == ("security", "list-keychains", "-d", "user"):
                        return '"/login" "/system"'
                    if arguments[1] == "find-identity":
                        return '1) ' + "A" * 40 + ' "Fixture identity"'
                    return ""
                def command(*arguments):
                    events.append(arguments)
                    if fails and arguments[0:2] == ("codesign", "--sign"):
                        raise RuntimeError("Image signing failed")
                with patch.object(dmg.signing_tools, "run", side_effect=security), \
                        patch.object(dmg, "run", side_effect=command), \
                        patch.dict(dmg.os.environ, {"APPLE_SIGNING_IDENTITY":"A" * 40, "APPLE_SIGNING_KEYCHAIN":"/temporary"}):
                    if fails:
                        with self.assertRaisesRegex(RuntimeError, "Image signing failed"):
                            dmg.create(app, root / "Sailry.dmg", "developer-id")
                    else:
                        dmg.create(app, root / "Sailry.dmg", "developer-id")
                names = [event[0:2] for event in events]
                self.assertEqual(names[:5], [("hdiutil", "create"), ("security", "list-keychains"),
                                            ("security", "list-keychains"), ("security", "find-identity"),
                                            ("codesign", "--sign")])
                self.assertEqual(events[2], ("security", "list-keychains", "-d", "user", "-s", "/login", "/system", "/temporary"))
                restore = ("security", "list-keychains", "-d", "user", "-s", "/login", "/system")
                if fails:
                    self.assertEqual(events[-1], restore)
                else:
                    self.assertEqual(events[-2], restore)
                    self.assertEqual(names[-3:], [("codesign", "--verify"), ("security", "list-keychains"), ("hdiutil", "verify")])


class Components(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = (ROOT / "scripts/package-macos.sh").read_text()

    def test_defaults_to_both_and_selects_independently(self):
        selection = self.source.split('task_component="${2:-all}"', 1)[1]
        selection = 'task_component="${2:-all}"' + selection.split('test "$(uname -s)"', 1)[0]
        script = f'set -euo pipefail\n{selection}\nprintf "%s %s" "$task_desktop" "$task_host"\n'
        for component, expected in ((None, "1 1"), ("all", "1 1"), ("desktop", "1 0"), ("host", "0 1")):
            with self.subTest(component=component):
                arguments = ["bash", "-c", script, "fixture", "release"]
                if component:
                    arguments.append(component)
                result = subprocess.run(arguments, text=True, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected)
        invalid = subprocess.run(["bash", "-c", script, "fixture", "release", "invalid"], capture_output=True)
        self.assertNotEqual(invalid.returncode, 0)

    def test_archive_names_keep_only_the_target(self):
        assignments = [line.strip() for line in self.source.splitlines()
                       if line.strip().startswith(("task_dmg=", "task_zip_name="))]
        script = '\n'.join(assignments) + '\nprintf "%s\\n%s" "$task_dmg" "$task_zip_name"'
        environment = os.environ | {"task_output": "/packages", "task_target": "aarch64-apple-darwin",
                                    "task_version": "0.1.0-alpha.1"}
        result = subprocess.run(["bash", "-c", script], env=environment, text=True,
                                capture_output=True, check=True)
        self.assertEqual(result.stdout.splitlines(), ["/packages/Sailry-aarch64-apple-darwin.dmg",
                                                      "Sailry-aarch64-apple-darwin.zip"])

    def test_requires_only_the_selected_executable(self):
        checks = self.source.split('test -n "$task_version"\n', 1)[1].split('mkdir -p "$task_root/dist"', 1)[0]
        for desktop in (True, False):
            with self.subTest(desktop=desktop), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                executable = root / ("sailry-desktop" if desktop else "sailry-host")
                executable.write_bytes(b"fixture")
                executable.chmod(0o755)
                environment = os.environ | {"task_build": str(root), "task_desktop": str(int(desktop)),
                                            "task_host": str(int(not desktop))}
                result = subprocess.run(["bash", "-c", f"set -eu\n{checks}"], env=environment)
                self.assertEqual(result.returncode, 0)
                environment.update({"task_desktop": "1", "task_host": "1"})
                self.assertNotEqual(subprocess.run(["bash", "-c", f"set -eu\n{checks}"], env=environment).returncode, 0)


class UpdateArchive(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = (ROOT / "scripts/package-macos.sh").read_text()

    def test_requires_notarized_distribution(self):
        setup = self.source.split('case "$task_signing" in', 1)[1].split("esac", 1)[0]
        self.assertIn('test "${SAILRY_NOTARIZE:-0}" = 1', setup)
        self.assertIn('test -n "$task_update_signing_key"', setup)
        self.assertIn('test -x "${SAILRY_RELEASE_TOOL:', setup)

    def test_preflight_fails_closed(self):
        capture = self.source.split(
            "# Keep the update publisher seed out of every unrelated packaging child.", 1
        )[1].split("# Package existing builds", 1)[0]
        setup = self.source.split('task_signing="${SAILRY_SIGNING:-ad-hoc}"', 1)[1]
        setup = 'task_signing="${SAILRY_SIGNING:-ad-hoc}"' + setup.split("esac", 1)[0] + "esac"
        script = f"set -euo pipefail\n{capture}\ntask_root=\"$FIXTURE_ROOT\"\ntask_desktop=1\n{setup}\n"
        environment = os.environ.copy()
        for name in (
            "APPLE_SIGNING_IDENTITY",
            "APPLE_TEAM_ID",
            "SAILRY_NOTARIZE",
            "SAILRY_RELEASE_TOOL",
            "SAILRY_SIGNING",
            "SAILRY_UPDATE_SIGNING_KEY",
        ):
            environment.pop(name, None)
        environment.update({
            "APPLE_SIGNING_IDENTITY": "Fixture identity",
            "APPLE_TEAM_ID": "FIXTURE",
            "FIXTURE_ROOT": str(ROOT),
            "SAILRY_NOTARIZE": "1",
            "SAILRY_RELEASE_TOOL": "/usr/bin/true",
            "SAILRY_SIGNING": "developer-id",
        })
        missing_key = subprocess.run(["bash", "-c", script], env=environment)
        self.assertNotEqual(missing_key.returncode, 0)
        environment["SAILRY_UPDATE_SIGNING_KEY"] = "fixture-seed"
        environment["SAILRY_RELEASE_TOOL"] = str(ROOT / "missing-release-tool")
        missing_tool = subprocess.run(["bash", "-c", script], env=environment)
        self.assertNotEqual(missing_tool.returncode, 0)
        environment["SAILRY_RELEASE_TOOL"] = "/usr/bin/true"
        environment["SAILRY_NOTARIZE"] = "0"
        missing_notary = subprocess.run(["bash", "-c", script], env=environment)
        self.assertNotEqual(missing_notary.returncode, 0)
        environment["SAILRY_NOTARIZE"] = "1"
        self.assertEqual(subprocess.run(["bash", "-c", script], env=environment).returncode, 0)

    def test_host_does_not_require_update_signing(self):
        capture = self.source.split(
            "# Keep the update publisher seed out of every unrelated packaging child.", 1
        )[1].split("# Package existing builds", 1)[0]
        setup = self.source.split('task_signing="${SAILRY_SIGNING:-ad-hoc}"', 1)[1]
        setup = 'task_signing="${SAILRY_SIGNING:-ad-hoc}"' + setup.split("esac", 1)[0] + "esac"
        script = f"set -euo pipefail\n{capture}\ntask_desktop=0\n{setup}\n"
        environment = os.environ.copy()
        environment.pop("SAILRY_UPDATE_SIGNING_KEY", None)
        environment.update({"APPLE_SIGNING_IDENTITY": "Fixture identity", "APPLE_TEAM_ID": "FIXTURE",
                            "SAILRY_SIGNING": "developer-id", "SAILRY_NOTARIZE": "1",
                            "SAILRY_RELEASE_TOOL": str(ROOT / "missing-release-tool")})
        self.assertEqual(subprocess.run(["bash", "-c", script], env=environment).returncode, 0)

    def test_ad_hoc_never_signs_updates(self):
        package = self.source.split("# Automatic updates", 1)[1]
        package = package.split("tar -czf", 1)[0]
        self.assertIn('if test "$task_signing" = developer-id; then', package)
        self.assertNotIn('if test -n "${SAILRY_UPDATE_SIGNING_KEY:-}"; then', package)

    def test_scopes_key_to_archive_signer(self):
        capture = self.source.index("unset SAILRY_UPDATE_SIGNING_KEY")
        first_child = self.source.index("uname -s")
        self.assertLess(capture, first_child)
        package = self.source.split("# Automatic updates", 1)[1]
        self.assertIn(
            'SAILRY_UPDATE_SIGNING_KEY="$task_update_signing_key" \\\n'
            '      "$task_release_tool" sign-archive',
            package,
        )
        describe = package.split('"$task_release_tool" describe', 1)[0]
        self.assertIn("unset task_update_signing_key", describe)


if __name__ == "__main__":
    unittest.main()
