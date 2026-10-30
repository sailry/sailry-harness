"""Host builds retain caller flags and include Linux symbol identities."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class HostBuild(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="sailry-host-build-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.project = self.root / "project space"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        scripts = self.project / "scripts"
        (scripts / "package").mkdir(parents=True)
        for name in ("build-host.sh", "python.sh", "telemetry-env.py"):
            shutil.copyfile(ROOT / "scripts" / name, scripts / name)
        (scripts / "package/code-parsers.py").write_text('''
import argparse
from pathlib import Path
parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
args, _ = parser.parse_known_args()
args.output.mkdir(parents=True, exist_ok=True)
print("file:///synthetic-parser-bundle")
''')
        (scripts / "package/linux-sysroot.py").write_text("# The sysroot suite verifies native link inputs.\n")
        self.record = self.root / "build.json"
        self.program(self.bin / "rustc", "#!/bin/sh\nprintf '%s\\n' 'host: aarch64-apple-darwin'\n")
        for name in ("llvm-ar", "llvm-strip"):
            self.program(self.bin / name, "#!/bin/sh\nexit 0\n")
        compiler = f'''#!{sys.executable}
import json, os, sys
from pathlib import Path
if sys.argv[1:] == ["--version"]:
    print("cargo-zigbuild 0.23.4")
    raise SystemExit(0)
Path({str(self.record)!r}).write_text(json.dumps({{
    "args": sys.argv[1:], "flags": os.environ.get("RUSTFLAGS"),
    "encoded": os.environ.get("CARGO_ENCODED_RUSTFLAGS"),
    "target": os.environ.get("CARGO_TARGET_DIR"),
}}))
raise SystemExit(73)
'''
        self.program(self.project / "target/tooling/bin/cargo-zigbuild", compiler)
        self.program(self.bin / "cargo", compiler)

    def program(self, path, source):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source)
        path.chmod(0o755)

    def build(self, target, flags):
        environment = {name: value for name, value in os.environ.items()
                       if not name.startswith(("SAILRY_", "CARGO_")) and name != "RUSTFLAGS"}
        environment.update({"PATH": str(self.bin) + os.pathsep + "/usr/bin:/bin",
                            "SAILRY_PYTHON": sys.executable,
                            "CARGO_TARGET_DIR": str(self.root / "build output"), **flags})
        result = subprocess.run(["/bin/bash", str(self.project / "scripts/build-host.sh"),
                                 "release", target], env=environment, capture_output=True,
                                text=True, timeout=20)
        self.assertEqual(result.returncode, 73, result.stderr)
        result = json.loads(self.record.read_text())
        self.assertEqual(result["target"], environment["CARGO_TARGET_DIR"])
        return result

    def test_adds_identity_for_both_linux_targets(self):
        for target in ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"):
            with self.subTest(target=target):
                result = self.build(target, {})
                self.assertEqual(result["flags"], "-C link-arg=-Wl,--build-id")
                self.assertIsNone(result["encoded"])
                self.assertEqual(result["args"], ["zigbuild", "--locked", "-p", "sailry-host",
                                                 "--profile", "release", "--target", target + ".2.34",
                                                 "--target-dir", str(self.root / "build output")])

    def test_preserves_plain_flags(self):
        result = self.build("x86_64-unknown-linux-gnu", {"RUSTFLAGS": "-C force-frame-pointers=yes"})
        self.assertEqual(result["flags"], "-C force-frame-pointers=yes -C link-arg=-Wl,--build-id")

    def test_preserves_encoded_flag_precedence(self):
        for encoded in ("", '--cfg\x1ffeature="value with spaces"'):
            with self.subTest(encoded=encoded):
                result = self.build("aarch64-unknown-linux-gnu", {
                    "RUSTFLAGS": "ignored caller flags", "CARGO_ENCODED_RUSTFLAGS": encoded})
                expected = (encoded + "\x1f" if encoded else "") + "-C\x1flink-arg=-Wl,--build-id"
                self.assertEqual(result["encoded"], expected)
                self.assertEqual(result["flags"], "ignored caller flags")

    def test_keeps_macos_flags_unchanged(self):
        flags = {"RUSTFLAGS": "plain caller flags", "CARGO_ENCODED_RUSTFLAGS": "encoded\x1fflags"}
        result = self.build("aarch64-apple-darwin", flags)
        self.assertEqual(result["flags"], flags["RUSTFLAGS"])
        self.assertEqual(result["encoded"], flags["CARGO_ENCODED_RUSTFLAGS"])
        self.assertEqual(result["args"], ["build", "--locked", "-p", "sailry-host", "--profile", "release"])


if __name__ == "__main__":
    unittest.main()
