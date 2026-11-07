"""Pinned computer-use helper archives, platform layouts and bundle signing."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import platform
import plistlib
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("agent_desktop", ROOT / "scripts/package/agent-desktop.py")
helper = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(helper)
TARGETS = {
    "aarch64-apple-darwin": ("macos", "arm64", ["agent-desktop", "agent-desktop-macos-helper"]),
    "x86_64-apple-darwin": ("macos", "x86_64", ["agent-desktop", "agent-desktop-macos-helper"]),
    "x86_64-pc-windows-msvc": ("windows", "amd64", ["agent-desktop.exe"]),
    "aarch64-pc-windows-msvc": ("windows", "arm64", ["agent-desktop.exe"]),
    "x86_64-unknown-linux-gnu": ("linux", "x86_64", ["agent-desktop", "agent-desktop-linux-clipboard-owner"]),
    "aarch64-unknown-linux-gnu": ("linux", "aarch64", ["agent-desktop", "agent-desktop-linux-clipboard-owner"]),
}


def archive(members):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w:gz") as bundle:
        for name, content in members.items():
            entry = tarfile.TarInfo(name)
            entry.size = len(content)
            entry.mode = 0o755
            bundle.addfile(entry, io.BytesIO(content))
    return stream.getvalue()


def fixture(target, contents):
    """A lock and asset pinning synthetic members for one target."""
    platform_name, arch, _ = TARGETS[target]
    files = [{"name": name, "member": name, "size": len(content),
              "sha256": hashlib.sha256(content).hexdigest()} for name, content in contents.items()]
    data = archive(contents)
    asset = {"platform": platform_name, "arch": arch, "archive": f"fixture-{target}.tar.gz",
             "size": len(data), "sha256": hashlib.sha256(data).hexdigest(),
             "executable": "agent-desktop.exe" if platform_name == "windows" else "agent-desktop", "files": files}
    lock = {"version": 1, "release": "1.2.0", "revision": "a" * 40,
            "source": "https://release.invalid/", "targets": {target: asset}}
    return lock, asset, data


class Inputs(unittest.TestCase):
    def test_pins_official_release_archives(self):
        lock = json.loads(helper.LOCK.read_text())
        self.assertEqual(lock["repository"], "https://github.com/vercel-labs/agent-desktop")
        self.assertEqual(lock["revision"], "2b0a7334fc111730454b726e6800aa41cdafc093")
        self.assertEqual(lock["source"], f"{lock['repository']}/releases/download/v{lock['release']}/")
        self.assertEqual(set(lock["targets"]), set(TARGETS))
        for target, (platform_name, arch, names) in TARGETS.items():
            with self.subTest(target=target):
                _, asset = helper.specification(target)
                self.assertEqual((asset["platform"], asset["arch"]), (platform_name, arch))
                self.assertEqual(asset["archive"], f"agent-desktop-v{lock['release']}-{target}.tar.gz")
                self.assertEqual([file["name"] for file in asset["files"]], names)
                self.assertIn(asset["executable"], names)
                for item in [asset, *asset["files"]]:
                    self.assertRegex(item["sha256"], r"^[0-9a-f]{64}$")
                    self.assertGreater(item["size"], 0)
        with self.assertRaisesRegex(ValueError, "Unsupported"):
            helper.specification("x86_64-unknown-linux-musl")

    def test_verifies_archive_members(self):
        contents = {"agent-desktop": b"\x7fELFfixture", "agent-desktop-linux-clipboard-owner": b"\x7fELFowner"}
        _, asset, data = fixture("x86_64-unknown-linux-gnu", contents)
        self.assertEqual(helper.extract(data, asset), contents)
        changed = archive({**contents, "agent-desktop": b"\x7fELFchanged"})
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            helper.extract(changed, asset)
        with self.assertRaisesRegex(ValueError, "unexpected entries"):
            helper.extract(archive({**contents, "../escaped": b"x"}), asset)
        with self.assertRaisesRegex(ValueError, "unexpected entries"):
            helper.extract(archive({"agent-desktop": contents["agent-desktop"]}), asset)
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w:gz") as bundle:
            for name in contents:
                entry = tarfile.TarInfo(name)
                entry.type = tarfile.SYMTYPE
                entry.linkname = "/bin/sh"
                bundle.addfile(entry)
        with self.assertRaisesRegex(ValueError, "entry is invalid"):
            helper.extract(stream.getvalue(), asset)

    def test_rejects_changed_cached_archives(self):
        contents = {"agent-desktop.exe": b"MZfixture"}
        lock, asset, data = fixture("x86_64-pc-windows-msvc", contents)
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary)
            (cache / asset["archive"]).write_bytes(data)
            self.assertEqual(helper.NATIVE.archive_bytes(lock, asset, cache), data)
            (cache / asset["archive"]).write_bytes(data[:-1] + bytes([data[-1] ^ 0xff]))
            with self.assertRaisesRegex(ValueError, "checksum"):
                helper.NATIVE.archive_bytes(lock, asset, cache)

    def test_checks_windows_architecture(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "agent-desktop.exe"
            for arch, machine in (("amd64", 0x8664), ("arm64", 0xaa64)):
                data = bytearray(128)
                data[:2] = b"MZ"
                struct.pack_into("<I", data, 0x3c, 64)
                data[64:68] = b"PE\0\0"
                struct.pack_into("<H", data, 68, machine)
                path.write_bytes(data)
                helper.architecture(path, {"platform": "windows", "arch": arch})
                other = "arm64" if arch == "amd64" else "amd64"
                with self.assertRaisesRegex(ValueError, "architecture"):
                    helper.architecture(path, {"platform": "windows", "arch": other})

    def test_checks_linux_architecture(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "agent-desktop"
            for arch, machine in (("x86_64", 62), ("aarch64", 183)):
                header = bytearray(64)
                header[:6] = b"\x7fELF\x02\x01"
                struct.pack_into("<H", header, 18, machine)
                path.write_bytes(header)
                helper.architecture(path, {"platform": "linux", "arch": arch})
                other = "aarch64" if arch == "x86_64" else "x86_64"
                with self.assertRaisesRegex(ValueError, "architecture"):
                    helper.architecture(path, {"platform": "linux", "arch": other})
            header[4] = 1
            path.write_bytes(header)
            with self.assertRaisesRegex(ValueError, "64-bit Linux"):
                helper.architecture(path, {"platform": "linux", "arch": "aarch64"})

    @unittest.skipUnless(platform.system() == "Darwin", "Requires macOS lipo")
    def test_checks_macos_architecture(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "agent-desktop"
            for arch, cpu, subtype in (("arm64", 0x100000c, 0), ("x86_64", 0x1000007, 3)):
                with self.subTest(arch=arch):
                    path.write_bytes(struct.pack("<8I", 0xfeedfacf, cpu, subtype, 2, 0, 0, 0, 0))
                    helper.architecture(path, {"platform": "macos", "arch": arch})
                    other = "x86_64" if arch == "arm64" else "arm64"
                    with self.assertRaises(subprocess.CalledProcessError):
                        helper.architecture(path, {"platform": "macos", "arch": other})


class Staging(unittest.TestCase):
    def stage(self, target, contents, output, **options):
        lock, asset, data = fixture(target, contents)
        cache = output.parent / "cache"
        cache.mkdir(parents=True, exist_ok=True)
        (cache / asset["archive"]).write_bytes(data)
        with patch.object(helper, "specification", return_value=(lock, asset)), \
                patch.object(helper, "architecture") as architecture:
            path = helper.stage(target, output, cache, **options)
        return path, architecture

    def test_builds_helper_bundle_with_sailry_identity(self):
        contents = {"agent-desktop": b"\xcf\xfa\xed\xfemain", "agent-desktop-macos-helper": b"\xcf\xfa\xed\xfecompanion"}
        for development, identifier, name in ((False, "dev.sailry.computer", "Sailry Computer Use"),
                                              (True, "dev.sailry.computer.dev", "Sailry Computer Use (Dev)")):
            with self.subTest(development=development), tempfile.TemporaryDirectory() as temporary:
                output = Path(temporary) / "Sailry.app/Contents/Helpers"
                stale = output / helper.APP / "Contents/MacOS/OpenComputerUse"
                stale.parent.mkdir(parents=True)
                stale.write_bytes(b"previous helper")
                (output / helper.RECEIPT).write_text("misplaced receipt")
                with patch.object(helper, "run") as run:
                    path, architecture = self.stage("aarch64-apple-darwin", contents, output,
                                                    development=development, identity="-")
                app = output / helper.APP
                self.assertEqual(path, app / "Contents/MacOS/agent-desktop")
                self.assertFalse(stale.exists())
                self.assertFalse((output / helper.RECEIPT).exists())
                for executable, content in contents.items():
                    staged = app / "Contents/MacOS" / executable
                    self.assertEqual(staged.read_bytes(), content)
                    self.assertEqual(staged.stat().st_mode & 0o777, 0o755)
                self.assertEqual(sorted(call.args[0] for call in architecture.call_args_list),
                                 sorted(app / "Contents/MacOS" / name for name in contents))
                info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
                self.assertEqual(info["CFBundleIdentifier"], identifier)
                self.assertEqual(info["CFBundleDisplayName"], name)
                self.assertEqual(info["CFBundleExecutable"], "agent-desktop")
                self.assertEqual(info["LSMinimumSystemVersion"], "13.0")
                self.assertTrue(info["LSUIElement"])
                self.assertEqual((app / "Contents/Resources/agent-desktop-LICENSE").read_bytes(),
                                 helper.LICENSE.read_bytes())
                self.assertEqual((app / "Contents/Resources/Sailry.icns").read_bytes(), helper.ICON.read_bytes())
                self.assertEqual([call.args[1:] for call in run.call_args_list], [
                    ("--force", "--sign", "-", "--timestamp=none", app / "Contents/MacOS/agent-desktop-macos-helper"),
                    ("--force", "--sign", "-", "--timestamp=none", app),
                    ("--verify", "--deep", "--strict", app),
                ])
                receipt = json.loads((output.parent / "Resources" / helper.RECEIPT).read_text())
                self.assertEqual((receipt["target"], receipt["development"]), ("aarch64-apple-darwin", development))
                self.assertEqual(receipt["files"], helper.inventory(output))
                path.write_bytes(b"resigned helper")
                helper.finalize(output)
                receipt = json.loads((output.parent / "Resources" / helper.RECEIPT).read_text())
                self.assertEqual(receipt["files"], helper.inventory(output))

    def test_hardens_distribution_signatures(self):
        with tempfile.TemporaryDirectory() as temporary:
            app = Path(temporary) / helper.APP
            companion = app / "Contents/MacOS/agent-desktop-macos-helper"
            companion.parent.mkdir(parents=True)
            companion.write_bytes(b"companion")
            (app / "Contents/MacOS/agent-desktop").write_bytes(b"main")
            with patch.object(helper, "run") as run:
                helper.sign(Path(temporary), "Developer ID Application: Fixture", timestamp=True)
            for call in run.call_args_list[:2]:
                self.assertIn("--timestamp", call.args)
                self.assertEqual(call.args[call.args.index("--options") + 1], "runtime")
            self.assertEqual([call.args[-1] for call in run.call_args_list], [companion, app, app])

    @unittest.skipUnless(os.name == "posix", "Requires POSIX open-file replacement")
    def test_replaces_open_executables(self):
        contents = {"agent-desktop": b"\x7fELFupdated", "agent-desktop-linux-clipboard-owner": b"\x7fELFowner"}
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "host/helpers"
            output.mkdir(parents=True)
            binary = output / "agent-desktop"
            binary.write_bytes(b"running helper")
            with binary.open("rb") as running:
                inode = os.fstat(running.fileno()).st_ino
                path, _ = self.stage("x86_64-unknown-linux-gnu", contents, output)
                self.assertEqual(running.read(), b"running helper")
            self.assertEqual(path.read_bytes(), b"\x7fELFupdated")
            self.assertNotEqual(path.stat().st_ino, inode)

    def test_places_host_and_windows_helpers(self):
        cases = (
            ("x86_64-unknown-linux-gnu", {"agent-desktop": b"\x7fELFmain", "agent-desktop-linux-clipboard-owner": b"\x7fELFowner"}),
            ("aarch64-pc-windows-msvc", {"agent-desktop.exe": b"MZfixture"}),
        )
        for target, contents in cases:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                output = Path(temporary) / "host/helpers"
                path, _ = self.stage(target, contents, output)
                self.assertEqual(path, output / TARGETS[target][2][0])
                self.assertEqual(sorted(entry.name for entry in output.iterdir()),
                                 sorted([*contents, "agent-desktop-LICENSE", helper.RECEIPT]))
                receipt = json.loads((output / helper.RECEIPT).read_text())
                self.assertEqual(receipt["revision"], "a" * 40)
                self.assertEqual(receipt["files"], helper.inventory(output))
                with self.assertRaisesRegex(ValueError, "requires macOS"):
                    self.stage(target, contents, output, identity="-")

    def test_rejects_linked_destinations(self):
        contents = {"agent-desktop.exe": b"MZfixture"}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "elsewhere").mkdir()
            output = root / "helpers"
            output.symlink_to(root / "elsewhere", target_is_directory=True)
            with self.assertRaisesRegex(ValueError, "symlink"):
                self.stage("x86_64-pc-windows-msvc", contents, output)
            self.assertEqual(list((root / "elsewhere").iterdir()), [])


if __name__ == "__main__":
    unittest.main()
