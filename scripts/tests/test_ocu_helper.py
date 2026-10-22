"""Pinned helper inputs, platform artifacts and session cleanup acknowledgements."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import platform
import socket
import struct
import subprocess
import tarfile
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("ocu_helper", ROOT / "scripts/package/ocu-helper.py")
helper = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(helper)


class Inputs(unittest.TestCase):
    def test_verifies_source_and_patch_identity(self):
        for target in ("aarch64-apple-darwin", "x86_64-apple-darwin",
                       "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"):
            lock, _, patches = helper.specification(target)
            self.assertEqual(lock["revision"], "8fdda42639c94fa2ce22ea2d9d706b5919a83b42")
            self.assertEqual(lock["source"]["sha256"], "77ae885b68ea33fc0d465b3d6cbf4788dc2d8daaa816dddfdfe160a60f017f44")
            self.assertEqual(len(patches), 1)
        with self.assertRaisesRegex(ValueError, "Unsupported"):
            helper.specification("x86_64-unknown-linux-gnu")

    def test_rejects_changed_source_archives(self):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary)
            data = b"source fixture"
            source = {"url": "https://invalid.test/source", "size": len(data),
                      "sha256": hashlib.sha256(data).hexdigest()}
            path = cache / "archives" / (source["sha256"] + ".tar.gz")
            path.parent.mkdir()
            path.write_bytes(data)
            self.assertEqual(helper.archive({"source": source}, cache), data)
            path.write_bytes(b"changed source")
            with self.assertRaisesRegex(ValueError, "checksum"):
                helper.archive({"source": source}, cache)

    def test_checks_windows_architecture(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "helper.exe"
            for arch, machine in (("amd64", 0x8664), ("arm64", 0xaa64)):
                data = bytearray(128)
                data[:2] = b"MZ"
                struct.pack_into("<I", data, 0x3c, 64)
                data[64:68] = b"PE\0\0"
                struct.pack_into("<H", data, 68, machine)
                path.write_bytes(data)
                helper.architecture(path, {"platform": "windows", "arch": arch})
                with self.assertRaisesRegex(ValueError, "architecture"):
                    helper.architecture(path, {"platform": "windows", "arch": "arm64" if arch == "amd64" else "amd64"})

    def test_rejects_escaping_archives(self):
        with tempfile.TemporaryDirectory() as temporary:
            data = io.BytesIO()
            with tarfile.open(fileobj=data, mode="w:gz") as archive:
                entry = tarfile.TarInfo("source/../../escaped")
                entry.size = 1
                archive.addfile(entry, io.BytesIO(b"x"))
            directory = Path(temporary)
            with patch.object(helper, "archive", return_value=data.getvalue()):
                with self.assertRaisesRegex(ValueError, "unexpected root"):
                    helper.source_tree({"revision": "a" * 40}, [], directory, "fixture")
            self.assertFalse((directory / "escaped").exists())

    def test_signs_helper_before_its_bundle(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            binary = directory / helper.APP / "Contents/MacOS/OpenComputerUse"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"native fixture")
            with patch.object(helper, "run") as run:
                helper.sign(directory, "-", False)
            self.assertEqual([call.args[-1] for call in run.call_args_list],
                             [binary, directory / helper.APP, directory / helper.APP])
            self.assertTrue(all("--preserve-metadata=entitlements" in call.args
                                for call in run.call_args_list[:2]))
            self.assertIn("--deep", run.call_args.args)


class Staging(unittest.TestCase):
    def test_moves_desktop_receipt_and_cleans_legacy(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            source = directory / "cache"
            binary = source / helper.APP / "Contents/MacOS/OpenComputerUse"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"unsigned helper")
            (source / helper.RECEIPT).write_text(json.dumps({"version": 1, "files": []}))
            output = directory / "Sailry.app/Contents/Helpers"
            output.mkdir(parents=True)
            (output / helper.RECEIPT).write_text("legacy manifest")
            receipt = output.parent / "Resources" / helper.RECEIPT

            def sign(staged, identity):
                self.assertFalse((output / helper.RECEIPT).exists())
                self.assertTrue(receipt.is_file())
                (staged / helper.APP / "Contents/MacOS/OpenComputerUse").write_bytes(b"signed helper")

            with patch.object(helper, "build", return_value=source), patch.object(helper, "sign", side_effect=sign):
                path = helper.stage("aarch64-apple-darwin", output, source, identity="-")
            self.assertEqual(path.read_bytes(), b"signed helper")
            self.assertEqual(json.loads(receipt.read_text())["files"], helper.inventory(output))
            self.assertEqual(sorted(path.name for path in output.iterdir()), [helper.APP])
            path.write_bytes(b"resigned helper")
            helper.finalize(output)
            self.assertEqual(json.loads(receipt.read_text())["files"], helper.inventory(output))

    def test_preserves_host_and_windows_layouts(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            source = directory / "cache"
            source.mkdir()
            (source / helper.RECEIPT).write_text(json.dumps({"version": 1, "files": []}))
            (source / "open-computer-use.exe").write_bytes(b"helper fixture")
            for name, target in (("host/helpers", "aarch64-apple-darwin"),
                                 ("windows/helpers", "x86_64-pc-windows-msvc")):
                with self.subTest(target=target), patch.object(helper, "build", return_value=source):
                    output = directory / name
                    helper.stage(target, output, source)
                    self.assertEqual(helper.receipt_path(output), output / helper.RECEIPT)
                    self.assertEqual(json.loads((output / helper.RECEIPT).read_text())["files"],
                                     helper.inventory(output))
            self.assertEqual(json.loads((source / helper.RECEIPT).read_text())["files"], [])


@unittest.skipUnless(platform.system() == "Darwin" and os.environ.get("SAILRY_OCU_HELPER"),
                     "Requires an explicitly built macOS helper")
class Sessions(unittest.TestCase):
    def exchange(self, acknowledge):
        executable = Path(os.environ["SAILRY_OCU_HELPER"]).resolve()
        bundle = executable.parents[2]
        with tempfile.TemporaryDirectory(prefix="ocu-session-") as temporary:
            namespace = str(Path(temporary))
            suffix = hashlib.sha256(namespace.encode()).hexdigest()[:16]
            path = Path(tempfile.gettempdir()) / f"open-computer-use-agent-{suffix}.sock"
            operations = []
            failures = []
            with socket.socket(socket.AF_UNIX) as listener:
                listener.bind(str(path))
                listener.listen(1)
                listener.settimeout(10)

                def serve():
                    try:
                        connection, _ = listener.accept()
                        with connection, connection.makefile("rwb") as stream:
                            while line := stream.readline():
                                request = json.loads(line)
                                operations.append(request["kind"])
                                if request["kind"] == "agentInfo":
                                    response = {"bundleURL": str(bundle), "processStartTime": time.time()}
                                elif acknowledge is None:
                                    break
                                else:
                                    response = {"ok": acknowledge}
                                stream.write(json.dumps(response).encode() + b"\n")
                                stream.flush()
                    except Exception as error:
                        failures.append(error)

                thread = threading.Thread(target=serve)
                thread.start()
                try:
                    result = subprocess.run([executable, "mcp"], input="", text=True, capture_output=True,
                                            env=dict(os.environ, OPEN_COMPUTER_USE_AGENT_SOCKET_NAMESPACE=namespace),
                                            timeout=10)
                finally:
                    thread.join(11)
                    path.unlink(missing_ok=True)
            self.assertFalse(thread.is_alive())
            self.assertEqual(failures, [])
            self.assertEqual(operations, ["agentInfo", "endSession"])
            return result

    def test_accepts_cleanup_acknowledgements(self):
        self.assertEqual(self.exchange(True).returncode, 0)

    def test_rejects_failed_cleanup_acknowledgements(self):
        self.assertNotEqual(self.exchange(False).returncode, 0)

    def test_rejects_lost_cleanup_acknowledgements(self):
        self.assertNotEqual(self.exchange(None).returncode, 0)
