"""Development recipes reuse the application default unless a profile is explicit."""

import hashlib
import io
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

from test_python_runtime import archive as python_archive
from test_llama_runtime import fixture as llama_fixture


ROOT = Path(__file__).resolve().parents[2]


class DesktopProfile(unittest.TestCase):
    def setUp(self):
        recipes = (ROOT / "justfile").read_text()
        desktop = recipes.split("\ndesktop ", 1)[1].split("\n\n", 1)[0]
        self.parameter, self.command = desktop.split(":\n", 1)
        self.preview = recipes.split("\npreview:\n", 1)[1].split("\n\n", 1)[0]

    def arguments(self, path):
        command = self.command.replace("{{quote(data_dir)}}", shlex.quote(path))
        result = subprocess.run(
            ["bash", "-c", "bash() { printf '%s\\0' \"$@\"; }\n" + command],
            check=True, capture_output=True,
        )
        return result.stdout.decode().rstrip("\0").split("\0")

    def test_reuses_application_default(self):
        self.assertEqual(
            self.parameter,
            'data_dir=env_var_or_default("SAILRY_DESKTOP_DATA_DIR", "")',
        )
        self.assertEqual(self.arguments(""), ["scripts/run-desktop.sh"])

    def test_preserves_explicit_path(self):
        for path in ("/private/profile", "/private/项目's profile 🙂"):
            with self.subTest(path=path):
                self.assertEqual(
                    self.arguments(path),
                    ["scripts/run-desktop.sh", "--data-dir", path],
                )

    def test_keeps_preview_separate(self):
        self.assertEqual(self.preview.strip(), "bash scripts/run-desktop.sh --preview")


class NativeAssets(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="sailry-desktop-launch-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.project = self.root / "project space"
        self.target = self.root / "build output"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for name in ("run-desktop.sh", "python.sh", "package/onnx-runtime.py",
                     "package/python-runtime.py", "package/python.entitlements.plist", "package/llama-runtime.py"):
            destination = self.project / "scripts" / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / "scripts" / name, destination)
        for name in ("apps/desktop/macos/Info.plist", "assets/branding/Sailry.icns",
                     "apps/desktop/macos/zh_CN.lproj/InfoPlist.strings"):
            destination = self.project / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text("resource fixture")
        (self.project / "scripts/package/ocu-helper.py").write_text("# The dedicated helper suite verifies native staging and signing.\n")
        (self.project / "scripts/package/code-parsers.py").write_text('''
# The dedicated parser suite verifies archive checksums and native signing.
import argparse
from pathlib import Path
parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--cache", type=Path, required=True)
args, _ = parser.parse_known_args()
args.output.mkdir(parents=True, exist_ok=True)
(args.output / "code-parsers.json").write_text('{"synthetic": true}')
source = args.cache / "synthetic-query.fixture"
source.parent.mkdir(parents=True, exist_ok=True)
source.write_bytes(b"synthetic query fixture")
print("file://" + str(source.absolute()))
''')
        self.old_calls = self.root / "old-python-calls"
        self.calls = self.root / "native-python-calls"
        self.signatures = self.root / "signatures"
        self.build = self.root / "build.json"
        self.program("python3", f'''#!/bin/sh
printf '%s\\n' "$1" >> {shlex.quote(str(self.old_calls))}
if test "$1" = -c; then exit 1; fi
exit 91
''', python=False)
        self.program("python3.14", f'''#!/bin/sh
if test "$1" != -c; then
    printf '%s\\0' "$@" >> {shlex.quote(str(self.calls))}
    printf '\\0' >> {shlex.quote(str(self.calls))}
fi
exec {shlex.quote(sys.executable)} "$@"
''', python=False)
        self.program("rustc", '#!/bin/sh\nprintf \'%s\\n\' \'host: aarch64-apple-darwin\'\n', python=False)
        self.program("uname", '#!/bin/sh\nprintf \'%s\\n\' Darwin\n', python=False)
        self.program("security", '#!/bin/sh\nprintf \'%s\\n\' \'  1) FIXTURE "Apple Development: Fixture User (FIXTURE)"\'\n', python=False)
        self.program("cargo", f'''
import json, os, sys
from pathlib import Path
Path({str(self.build)!r}).write_text(json.dumps({{
    "args": sys.argv[1:], "native": os.environ["SHERPA_ONNX_LIB_DIR"],
    "parsers": os.environ["TSLP_SOURCE_BUNDLE_URL"],
    "offline": os.environ.get("TSLP_OFFLINE"), "languages": os.environ.get("TSLP_LANGUAGES")
}}))
binary = Path(os.environ["CARGO_TARGET_DIR"]) / "debug/sailry-desktop"
binary.write_bytes(b"compiled fixture")
binary.chmod(0o755)
''')
        self.program("codesign", f'''#!/bin/sh
printf '%s\\0' "$@" >> {shlex.quote(str(self.signatures))}
printf '\\0' >> {shlex.quote(str(self.signatures))}
for task_target do :; done
if test -d "$task_target"; then exit 73; fi
printf '%s' 'signature fixture' >> "$task_target"
''', python=False)
        self.contents = {"libonnxruntime.dylib": b"runtime fixture",
                         "libsherpa-onnx-c-api.dylib": b"speech fixture"}
        self.archive()
        llama_fixture(self.project / "scripts/package", self.target / "llama-prebuilt")
        self.python_contents = {"bin/python3.12": b"\xcf\xfa\xed\xfeinterpreter fixture",
                                "lib/python3.12/fixture.so": b"\xcf\xfa\xed\xfeextension fixture"}
        data = python_archive(
            [("python/lib/python3.12/fixture.so", self.python_contents["lib/python3.12/fixture.so"], None)],
            interpreter=self.python_contents["bin/python3.12"],
        )
        self.python_hash = hashlib.sha256(data).hexdigest()
        (self.project / "scripts/package/python-runtime.lock.json").write_text(json.dumps({
            "version": 1, "python_version": "3.12.13", "targets": {"aarch64-apple-darwin": {
                "url": "https://release.invalid/python.tar.gz", "archive": "python.tar.gz",
                "size": len(data), "sha256": self.python_hash}}}))
        cache = self.target / "python-cache"
        cache.mkdir()
        (cache / "python.tar.gz").write_bytes(data)

    def program(self, name, content, python=True):
        path = self.bin / name
        path.write_text((f"#!{sys.executable}\n" if python else "") + content)
        path.chmod(0o755)

    def arguments(self, path):
        return [[argument.decode() for argument in record.split(b"\0")]
                for record in path.read_bytes().split(b"\0\0") if record]

    def archive(self):
        stream = io.BytesIO()
        files = []
        with tarfile.open(fileobj=stream, mode="w:bz2") as package:
            for name, content in self.contents.items():
                member = tarfile.TarInfo("fixed/lib/" + name)
                member.size = len(content)
                package.addfile(member, io.BytesIO(content))
                files.append({"name": name, "member": member.name, "size": len(content),
                              "sha256": hashlib.sha256(content).hexdigest(),
                              "role": "runtime" if name == "libonnxruntime.dylib" else "speech"})
        data = stream.getvalue()
        self.archive_hash = hashlib.sha256(data).hexdigest()
        lock = {"version": 1, "runtime_version": "1.28.2", "sherpa_version": "1.13.8",
                "source": "https://release.invalid/", "targets": {"aarch64-apple-darwin": {
                    "archive": "fixed.tar.bz2", "size": len(data),
                    "sha256": self.archive_hash, "files": files}}}
        (self.project / "scripts/package/onnx-runtime.lock.json").write_text(json.dumps(lock))
        cache = self.target / "sherpa-onnx-prebuilt"
        cache.mkdir(parents=True)
        (cache / "fixed.tar.bz2").write_bytes(data)

    def test_selects_python_for_each_native_stage(self):
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("SAILRY_")}
        environment.update({"PATH": str(self.bin) + os.pathsep + "/usr/bin:/bin",
                            "CARGO_TARGET_DIR": str(self.target),
                            "TSLP_OFFLINE": "1", "TSLP_LANGUAGES": "all"})
        result = subprocess.run(["/bin/bash", str(self.project / "scripts/run-desktop.sh")],
                                env=environment, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 73, result.stderr)
        calls = self.arguments(self.calls)
        native = self.target / "onnx-native/aarch64-apple-darwin"
        debug = self.target / "debug"
        app = debug / "Sailry.app"
        stages = [("code-parsers.py", debug / "code-parsers"), ("python-runtime.py", debug),
                  ("onnx-runtime.py", native), ("llama-runtime.py", native),
                  ("onnx-runtime.py", debug), ("llama-runtime.py", debug),
                  ("code-parsers.py", app / "Contents/Resources/code-parsers"),
                  ("python-runtime.py", app / "Contents/MacOS"),
                  ("python-runtime.py", app / "Contents/MacOS"), ("onnx-runtime.py", app / "Contents/MacOS"),
                  ("onnx-runtime.py", app / "Contents/MacOS"), ("llama-runtime.py", app / "Contents/MacOS"),
                  ("ocu-helper.py", app / "Contents/Helpers")]
        self.assertEqual(self.old_calls.read_text().splitlines(), ["-c"] * len(stages))
        self.assertEqual(len(calls), len(stages))
        for arguments, (script, output) in zip(calls, stages):
            self.assertEqual(arguments[0], str(self.project / "scripts/package" / script))
            self.assertEqual(arguments[arguments.index("--output") + 1], str(output))
        self.assertEqual(calls[0][calls[0].index("--sign") + 1], "-")
        self.assertEqual(calls[6][calls[6].index("--sign") + 1], "FIXTURE")
        self.assertEqual(calls[8][calls[8].index("--sign") + 1], "FIXTURE")
        self.assertIn("--finalize", calls[-3])
        self.assertEqual(calls[-2][calls[-2].index("--source") + 1], str(debug))
        self.assertEqual(calls[-2][calls[-2].index("--sign") + 1], "FIXTURE")
        self.assertIn("--development", calls[-1])
        self.assertEqual(calls[-1][calls[-1].index("--sign") + 1], "FIXTURE")
        self.assertEqual(json.loads(self.build.read_text()), {
            "args": ["build", "--locked", "-p", "sailry-desktop"], "native": str(native),
            "parsers": "file://" + str(self.project / ".runtime/code-index/resources/synthetic-query.fixture"),
            "offline": None, "languages": None})
        self.assertEqual((app / "Contents/Resources/code-parsers/code-parsers.json").read_text(),
                         '{"synthetic": true}')
        signatures = self.arguments(self.signatures)
        python = app / "Contents/Resources/python"
        link = app / "Contents/MacOS/python"
        self.assertTrue(link.is_symlink())
        self.assertEqual(link.resolve(), python.resolve())
        self.assertEqual([arguments[-1] for arguments in signatures], [
            str(python / "lib/python3.12/fixture.so"), str(python / "bin/python3.12"),
        ] + [
            str(app / "Contents/MacOS" / name) for name in self.contents] + [
            str(app / "Contents/MacOS/libllama.0.dylib"), str(app)])
        self.assertTrue(all(arguments[arguments.index("--sign") + 1] == "FIXTURE"
                            for arguments in signatures))
        child = next(arguments for arguments in signatures
                     if arguments[-1] == str(python / "bin/python3.12"))
        self.assertEqual(child[child.index("--entitlements") + 1],
                         str(self.project / "scripts/package/python.entitlements.plist"))
        self.assertEqual(child[child.index("--options") + 1], "runtime")
        self.assertTrue(all("--entitlements" not in arguments for arguments in signatures
                            if arguments is not child))
        for directory, signed in [(debug, False), (app / "Contents/MacOS", True)]:
            python = directory / "python"
            receipt = json.loads((python / "python-runtime.json").read_text())
            self.assertEqual(receipt["target"], "aarch64-apple-darwin")
            self.assertEqual(receipt["python_version"], "3.12.13")
            self.assertEqual(receipt["source"]["sha256"], self.python_hash)
            self.assertEqual((python / "bin/python3").resolve(), (python / "bin/python3.12").resolve())
            self.assertTrue((python / "lib/python3.12/EXTERNALLY-MANAGED").is_file())
            self.assertFalse(python.stat().st_mode & 0o200)
            for name, content in self.python_contents.items():
                expected = content + (b"signature fixture" if signed else b"")
                self.assertEqual((python / name).read_bytes(), expected)
                recorded = next(file for file in receipt["files"] if file["path"] == name)
                self.assertEqual(recorded["sha256"], hashlib.sha256(expected).hexdigest())
                self.assertEqual(recorded["size"], len(expected))
                self.assertFalse((python / name).stat().st_mode & 0o200)
        for directory in (native, debug, debug / "deps", debug / "examples"):
            metadata = json.loads((directory / "onnx-runtime.json").read_text())
            self.assertEqual(metadata["target"], "aarch64-apple-darwin")
            self.assertEqual(metadata["runtime_version"], "1.28.2")
            self.assertEqual(metadata["sherpa_version"], "1.13.8")
            self.assertEqual(metadata["source"]["sha256"], self.archive_hash)
            for file in metadata["files"]:
                self.assertEqual((directory / file["name"]).read_bytes(), self.contents[file["name"]])
                self.assertEqual(file["sha256"], hashlib.sha256(self.contents[file["name"]]).hexdigest())
                self.assertEqual(file["source_sha256"], file["sha256"])
                self.assertEqual(file["size"], len(self.contents[file["name"]]))
        metadata = json.loads((app / "Contents/Resources/onnx-runtime.json").read_text())
        self.assertEqual(metadata["target"], "aarch64-apple-darwin")
        self.assertEqual(metadata["runtime_version"], "1.28.2")
        self.assertEqual(metadata["sherpa_version"], "1.13.8")
        self.assertEqual(metadata["source"]["sha256"], self.archive_hash)
        self.assertFalse((app / "Contents/MacOS/onnx-runtime.json").exists())
        self.assertEqual((app / "Contents/MacOS/sailry-desktop").read_bytes(), b"compiled fixture")
        for file in metadata["files"]:
            content = self.contents[file["name"]]
            self.assertEqual((app / "Contents/MacOS" / file["name"]).read_bytes(), content + b"signature fixture")
            self.assertEqual(file["source_sha256"], hashlib.sha256(content).hexdigest())
            self.assertEqual(file["sha256"], hashlib.sha256(content + b"signature fixture").hexdigest())
            self.assertNotEqual(file["sha256"], file["source_sha256"])
            self.assertEqual(file["size"], len(content + b"signature fixture"))

    def test_stages_windows_helpers_before_execution(self):
        target = "x86_64-pc-windows-msvc"
        self.program("rustc", f"#!/bin/sh\nprintf '%s\\n' 'host: {target}'\n", python=False)
        self.program("uname", "#!/bin/sh\nprintf '%s\\n' MINGW64_NT\n", python=False)
        for name in ("onnx-runtime.py", "llama-runtime.py", "python-runtime.py"):
            (self.project / "scripts/package" / name).write_text("# Runtime staging fixture\n")
        (self.target / "debug").mkdir(parents=True)
        environment = {key: value for key, value in os.environ.items() if not key.startswith("SAILRY_")}
        environment.update({"PATH": str(self.bin) + os.pathsep + "/usr/bin:/bin",
                            "CARGO_TARGET_DIR": str(self.target),
                            "TSLP_OFFLINE": "1", "TSLP_LANGUAGES": "all"})
        result = subprocess.run(["/bin/bash", str(self.project / "scripts/run-desktop.sh")],
                                env=environment, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        helper = self.arguments(self.calls)[-1]
        self.assertEqual(helper[0], str(self.project / "scripts/package/ocu-helper.py"))
        self.assertEqual(helper[helper.index("--target") + 1], target)
        self.assertEqual(helper[helper.index("--output") + 1], str(self.target / "debug/helpers"))
        build = json.loads(self.build.read_text())
        self.assertEqual(build["args"], ["run", "--locked", "-p", "sailry-desktop", "--"])
        self.assertEqual(build["parsers"],
                         "file://" + str(self.project / ".runtime/code-index/resources/synthetic-query.fixture"))
        self.assertIsNone(build["offline"])
        self.assertIsNone(build["languages"])
        self.assertFalse(self.signatures.exists())


if __name__ == "__main__":
    unittest.main()
