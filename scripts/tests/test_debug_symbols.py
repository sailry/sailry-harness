"""Native symbol fixtures replace platform tools and never contact a server."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("debug_symbols", ROOT / "scripts/package/debug-symbols.py")
SYMBOLS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SYMBOLS)


class Collection(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.root / "native/sailry-desktop"
        self.binary.parent.mkdir()
        self.binary.write_bytes(b"native executable")
        self.output = self.root / "target/debug-symbols/desktop/fixture"

    def tools(self, command, **options):
        self.assertTrue(options["capture_output"])
        if command == ["dsymutil", "--help"]:
            return SimpleNamespace(stdout=getattr(self, "help", ""))
        self.assertTrue(options["check"])
        if "--debug-info" in command:
            return SimpleNamespace(stdout=b"DW_TAG_compile_unit\nDW_AT_stmt_list (0)")
        if "--uuid" in command:
            uuids = getattr(self, "uuids", {})
            uuid = uuids.get(command[-1], uuids.get(Path(command[-1]).suffix, "0" * 8 + "-0000-0000-0000-" + "0" * 12))
            return SimpleNamespace(stdout=f"UUID: {uuid} (arm64) {command[-1]}\n".encode())
        destination = Path(command[-1])
        if command[0] == "dsymutil":
            destination.mkdir()
            (destination / "fixture").write_bytes(b"DWARF")
        else:
            destination.write_bytes(b"ELF debug")
        return SimpleNamespace(stdout=b"", stderr=b"")

    def test_keeps_dsym_separate(self):
        with patch.object(SYMBOLS.shutil, "which", return_value="dwarfdump"), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=self.tools) as run:
            destination = SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output)
        self.assertEqual(destination, self.output / "sailry-desktop.dSYM")
        self.assertEqual((destination / "fixture").read_bytes(), b"DWARF")
        self.assertEqual(run.call_args_list[0].args[0], ["dsymutil", "--help"])
        self.assertEqual(run.call_args_list[1].args[0][:3], ["dsymutil", str(self.binary), "-o"])
        self.assertEqual(run.call_args_list[2].args[0][:3],
                         ["dwarfdump", "--debug-info", "--recurse-depth=0"])
        self.assertEqual(self.binary.read_bytes(), b"native executable")

    def test_keeps_elf_debug_separate(self):
        with patch.object(SYMBOLS.shutil, "which", side_effect=lambda name: name), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=self.tools) as run:
            destination = SYMBOLS.collect(self.binary, "aarch64-unknown-linux-gnu", self.output)
        self.assertEqual(destination, self.output / "sailry-desktop.debug")
        self.assertEqual(destination.read_bytes(), b"ELF debug")
        self.assertEqual(run.call_args_list[0].args[0][:3],
                         ["llvm-objcopy", "--only-keep-debug", str(self.binary)])
        self.assertEqual(self.binary.read_bytes(), b"native executable")

    def test_keeps_pdb_separate(self):
        binary = self.binary.with_suffix(".exe")
        self.binary.rename(binary)
        binary.with_name("sailry_desktop.pdb").write_bytes(b"PDB fixture")
        binary.with_suffix(".pdb").write_bytes(b"literal name fixture")
        with patch.object(SYMBOLS.subprocess, "run") as run:
            destination = SYMBOLS.collect(binary, "x86_64-pc-windows-msvc", self.output)
        self.assertEqual(destination, self.output / "sailry_desktop.pdb")
        self.assertEqual(destination.read_bytes(), b"PDB fixture")
        run.assert_not_called()

    def test_keeps_literal_pdb_name(self):
        binary = self.binary.with_suffix(".exe")
        self.binary.rename(binary)
        binary.with_suffix(".pdb").write_bytes(b"PDB fixture")
        with patch.object(SYMBOLS.subprocess, "run") as run:
            destination = SYMBOLS.collect(binary, "x86_64-pc-windows-msvc", self.output)
        self.assertEqual(destination, self.output / "sailry-desktop.pdb")
        self.assertEqual(destination.read_bytes(), b"PDB fixture")
        run.assert_not_called()

    def test_allows_missing_local_symbols(self):
        with patch.object(SYMBOLS.subprocess, "run") as run:
            self.assertIsNone(SYMBOLS.collect(self.binary, "x86_64-pc-windows-msvc", self.output, True))
            self.assertIsNone(SYMBOLS.collect(self.root / "missing", "aarch64-apple-darwin", self.output, True))
        run.assert_not_called()

    def test_requires_symbols_for_release(self):
        with self.assertRaisesRegex(ValueError, "PDB"):
            SYMBOLS.collect(self.binary, "x86_64-pc-windows-msvc", self.output)
        with self.assertRaisesRegex(ValueError, "missing"):
            SYMBOLS.collect(self.root / "missing", "aarch64-apple-darwin", self.output)

    def test_rejects_missing_line_tables(self):
        def without_lines(command, **options):
            result = self.tools(command, **options)
            return SimpleNamespace(stdout=b"No debug information") if "--debug-info" in command else result
        with patch.object(SYMBOLS.shutil, "which", return_value="dwarfdump"), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=without_lines):
            self.assertIsNone(SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output, True))
            with self.assertRaisesRegex(ValueError, "did not produce debug symbols"):
                SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output)
        self.assertEqual(list(self.output.iterdir()), [])

    def test_preserves_completed_symbols_on_failure(self):
        self.output.mkdir(parents=True)
        existing = self.output / "sailry-desktop.debug"
        existing.write_bytes(b"preserved previous build")
        with patch.object(SYMBOLS.shutil, "which", side_effect=lambda name: name), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=subprocess.CalledProcessError(1, ["fixture"])):
            with self.assertRaisesRegex(ValueError, "extraction failed"):
                SYMBOLS.collect(self.binary, "x86_64-unknown-linux-gnu", self.output)
        self.assertEqual(existing.read_bytes(), b"preserved previous build")
        self.assertEqual(list(self.output.iterdir()), [existing])

    def test_replaces_symbols_for_another_build(self):
        with patch.object(SYMBOLS.shutil, "which", return_value="dwarfdump"), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=self.tools):
            destination = SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output)
            (destination / "obsolete").write_bytes(b"old")
            self.uuids = {str(destination): "1A2B3C4D-0000-0000-0000-000000000002"}
            SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output)
        self.assertFalse((destination / "obsolete").exists())
        self.assertEqual(self.binary.read_bytes(), b"native executable")

    def test_reuses_symbols_for_same_build(self):
        with patch.object(SYMBOLS.shutil, "which", return_value="dwarfdump"), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=self.tools) as run:
            destination = SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output)
            run.reset_mock()
            self.assertEqual(SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output, True), destination)
        self.assertEqual([call.args[0][:2] for call in run.call_args_list], [["dwarfdump", "--uuid"]] * 2)
        self.assertEqual((destination / "fixture").read_bytes(), b"DWARF")

    def linked(self, failing=None, optional=False):
        """Linker commands collection ran, with `failing` breaking the matching attempt."""
        if self.output.exists():
            shutil.rmtree(self.output)
        self.attempt = ""

        def tools(command, **options):
            if failing == "run" and command[:2] == ["dsymutil", "--linker"]:
                raise subprocess.CalledProcessError(1, command)
            if failing == "uuid" and "--uuid" in command and "parallel" in self.attempt:
                if Path(command[-1]).suffix == ".dSYM":
                    return SimpleNamespace(stdout=b"UUID: FFFFFFFF-0000-0000-0000-000000000000 (arm64)\n")
            if command[0] == "dsymutil" and command != ["dsymutil", "--help"]:
                self.attempt = " ".join(command)
            return self.tools(command, **options)
        with patch.object(SYMBOLS.shutil, "which", return_value="dwarfdump"), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=tools) as run:
            destination = SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output, optional)
        self.assertEqual(destination, self.output / "sailry-desktop.dSYM")
        return [call.args[0][:3] for call in run.call_args_list
                if call.args[0][0] == "dsymutil" and call.args[0] != ["dsymutil", "--help"]]

    def test_prefers_parallel_linker(self):
        self.help = "  --linker <DWARF linker type>\n"
        self.assertEqual(self.linked(), [["dsymutil", "--linker", "parallel"]])

    def test_falls_back_to_classic_linker(self):
        self.help = "  --linker <DWARF linker type>\n"
        for failing in ("run", "uuid"):
            with self.subTest(failing=failing):
                self.assertEqual(self.linked(failing),
                                 [["dsymutil", "--linker", "parallel"], ["dsymutil", str(self.binary), "-o"]])

    def test_uses_classic_linker_without_option(self):
        self.help = "  --num-threads <threads>\n"
        self.assertEqual(self.linked(optional=True), [["dsymutil", str(self.binary), "-o"]])

    def test_fails_when_every_linker_fails(self):
        self.help = "  --linker <DWARF linker type>\n"
        with patch.object(SYMBOLS.shutil, "which", return_value="dwarfdump"), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=lambda command, **options: (
                    self.tools(command, **options) if command[0] != "dsymutil" or "--help" in command
                    else (_ for _ in ()).throw(subprocess.CalledProcessError(1, command)))):
            self.assertIsNone(SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output, True))
            with self.assertRaisesRegex(ValueError, "extraction failed"):
                SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output)

    def test_rejects_unverifiable_uuids(self):
        def unreadable(command, **options):
            if "--uuid" in command:
                raise subprocess.CalledProcessError(1, command)
            return self.tools(command, **options)
        with patch.object(SYMBOLS.shutil, "which", return_value="dwarfdump"), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=unreadable) as run:
            self.assertIsNone(SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output, True))
            with self.assertRaisesRegex(ValueError, "did not produce debug symbols"):
                SYMBOLS.collect(self.binary, "aarch64-apple-darwin", self.output)
        linked = [call for call in run.call_args_list
                  if call.args[0][0] == "dsymutil" and "--help" not in call.args[0]]
        self.assertEqual(len(linked), 2)
        self.assertFalse((self.output / "sailry-desktop.dSYM").exists())


class Cli(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.cache = self.root / "cache"
        self.data = b"pinned fixture executable"
        self.name = "sentry-cli-Darwin-arm64"
        self.lock = self.root / "lock.json"
        self.lock.write_text(json.dumps({
            "version": "3.8.2", "assets": {self.name: hashlib.sha256(self.data).hexdigest()}
        }))
        self.addCleanup(patch.stopall)
        patch.object(SYMBOLS, "LOCK", self.lock).start()
        patch.object(SYMBOLS.platform, "system", return_value="Darwin").start()
        patch.object(SYMBOLS.platform, "machine", return_value="arm64").start()

    def test_downloads_locked_asset(self):
        with patch.object(SYMBOLS.urllib.request, "urlopen", return_value=io.BytesIO(self.data)) as request:
            executable = SYMBOLS.cli(self.cache)
        self.assertEqual(executable.read_bytes(), self.data)
        self.assertEqual(request.call_args.args,
                         ("https://github.com/getsentry/sentry-cli/releases/download/3.8.2/" + self.name,))
        self.assertEqual(executable.stat().st_mode & 0o777, 0o755)

    def test_reuses_only_verified_cache(self):
        self.cache.mkdir()
        executable = self.cache / self.name
        executable.write_bytes(self.data)
        with patch.object(SYMBOLS.urllib.request, "urlopen") as request:
            self.assertEqual(SYMBOLS.cli(self.cache), executable)
        request.assert_not_called()
        executable.write_bytes(b"modified executable")
        with patch.object(SYMBOLS.urllib.request, "urlopen", return_value=io.BytesIO(self.data)) as request:
            SYMBOLS.cli(self.cache)
        request.assert_called_once()
        self.assertEqual(executable.read_bytes(), self.data)

    def test_rejects_checksum_mismatch(self):
        with patch.object(SYMBOLS.urllib.request, "urlopen", return_value=io.BytesIO(b"wrong artifact")):
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                SYMBOLS.cli(self.cache)
        self.assertFalse((self.cache / self.name).exists())

    def test_download_failure_redacts_details(self):
        with patch.object(SYMBOLS.urllib.request, "urlopen", side_effect=OSError("synthetic-upload-token")):
            with self.assertRaisesRegex(ValueError, "Sentry CLI download failed") as raised:
                SYMBOLS.cli(self.cache)
        self.assertNotIn("synthetic-upload-token", str(raised.exception))
        self.assertTrue(raised.exception.__suppress_context__)
        self.assertFalse((self.cache / self.name).exists())

    def test_rejects_unsupported_platform(self):
        with patch.object(SYMBOLS.platform, "machine", return_value="unsupported"), \
                patch.object(SYMBOLS.urllib.request, "urlopen") as request:
            with self.assertRaisesRegex(ValueError, "Unsupported"):
                SYMBOLS.cli(self.cache)
        request.assert_not_called()

    def test_lock_covers_native_producers(self):
        lock = json.loads((ROOT / "scripts/package/sentry-cli.lock.json").read_text())
        self.assertEqual(lock["version"], "3.8.2")
        for system, architectures in {"Darwin": ["arm64", "x86_64"],
                                      "Linux": ["aarch64", "x86_64"],
                                      "Windows": ["aarch64", "x86_64"]}.items():
            for architecture in architectures:
                name = f"sentry-cli-{system}-{architecture}" + (".exe" if system == "Windows" else "")
                self.assertRegex(lock["assets"][name], r"^[0-9a-f]{64}$")


class Upload(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.symbols = self.root / "sailry-host.dSYM"
        self.symbols.mkdir()
        self.payload = self.symbols / "fixture"
        self.payload.write_bytes(b"retained native debug")
        self.environment = {"SENTRY_AUTH_TOKEN": "synthetic-upload-token",
                            "SENTRY_ORG": "fixture-org", "SENTRY_PROJECT": "fixture-project"}

    def test_requires_all_credentials(self):
        environments = [{}, {"SAILRY_SENTRY_DSN": "synthetic-dsn"}]
        environments.extend({key: value for key, value in self.environment.items() if key != missing}
                            for missing in self.environment)
        for environment in environments:
            with self.subTest(environment=environment), patch.object(SYMBOLS, "cli") as cli, \
                    patch("sys.stdout", new_callable=io.StringIO) as output:
                self.assertFalse(SYMBOLS.upload(self.symbols, environment))
                cli.assert_not_called()
                self.assertEqual(output.getvalue(),
                                 "Native symbols preserved; upload credentials are not configured\n")
                self.assertEqual(self.payload.read_bytes(), b"retained native debug")

    def test_keeps_token_in_environment(self):
        with patch.object(SYMBOLS, "cli", return_value=Path("fixture-cli")), \
                patch.object(SYMBOLS.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run, \
                patch("sys.stdout", new_callable=io.StringIO) as output:
            self.assertTrue(SYMBOLS.upload(self.symbols, self.environment))
        command = run.call_args.args[0]
        self.assertEqual(command, ["fixture-cli", "debug-files", "upload", "--org", "fixture-org", "--project",
                                   "fixture-project", "--no-sources", str(self.symbols)])
        self.assertNotIn(self.environment["SENTRY_AUTH_TOKEN"], repr(command))
        self.assertNotIn(self.environment["SENTRY_AUTH_TOKEN"], output.getvalue())
        self.assertEqual(run.call_args.kwargs["env"]["SENTRY_AUTH_TOKEN"], "synthetic-upload-token")
        self.assertTrue(run.call_args.kwargs["capture_output"])
        self.assertEqual(run.call_args.kwargs["timeout"], 600)
        self.assertNotIn("--include-sources", command)

    def test_failure_keeps_symbols_and_redacts_output(self):
        with patch.object(SYMBOLS, "cli", return_value=Path("fixture-cli")), \
                patch.object(SYMBOLS.subprocess, "run", return_value=SimpleNamespace(
                    returncode=1, stderr=b"synthetic-upload-token")):
            with self.assertRaisesRegex(ValueError, "Sentry debug symbol upload failed") as raised:
                SYMBOLS.upload(self.symbols, self.environment)
        self.assertNotIn("synthetic-upload-token", str(raised.exception))
        self.assertEqual(self.payload.read_bytes(), b"retained native debug")

    def test_timeout_preserves_symbols_and_redacts_details(self):
        error = subprocess.TimeoutExpired(["synthetic-upload-token"], 600,
                                          output=b"synthetic-upload-token", stderr=b"synthetic-upload-token")
        with patch.object(SYMBOLS, "cli", return_value=Path("fixture-cli")), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=error):
            with self.assertRaisesRegex(ValueError, "upload timed out after 600 seconds") as raised:
                SYMBOLS.upload(self.symbols, self.environment)
        self.assertNotIn("synthetic-upload-token", str(raised.exception))
        self.assertTrue(raised.exception.__suppress_context__)
        self.assertEqual(self.payload.read_bytes(), b"retained native debug")

    def test_launch_failure_preserves_symbols(self):
        with patch.object(SYMBOLS, "cli", return_value=Path("fixture-cli")), \
                patch.object(SYMBOLS.subprocess, "run", side_effect=OSError("synthetic-upload-token")):
            with self.assertRaisesRegex(ValueError, "Sentry CLI could not start") as raised:
                SYMBOLS.upload(self.symbols, self.environment)
        self.assertNotIn("synthetic-upload-token", str(raised.exception))
        self.assertTrue(raised.exception.__suppress_context__)
        self.assertEqual(self.payload.read_bytes(), b"retained native debug")

    def test_preparation_failure_preserves_symbols(self):
        with patch.object(SYMBOLS, "cli", side_effect=OSError("synthetic-upload-token")), \
                patch.object(SYMBOLS.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "Sentry CLI preparation failed") as raised:
                SYMBOLS.upload(self.symbols, self.environment)
        run.assert_not_called()
        self.assertNotIn("synthetic-upload-token", str(raised.exception))
        self.assertTrue(raised.exception.__suppress_context__)
        self.assertEqual(self.payload.read_bytes(), b"retained native debug")

    def test_accepts_native_files(self):
        for suffix in (".debug", ".pdb"):
            symbols = self.root / ("sailry_host" + suffix)
            symbols.write_bytes(b"retained native debug")
            with self.subTest(suffix=suffix), \
                    patch.object(SYMBOLS, "cli", return_value=Path("fixture-cli")), \
                    patch.object(SYMBOLS.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run, \
                    patch("sys.stdout", new_callable=io.StringIO):
                self.assertTrue(SYMBOLS.upload(symbols, self.environment))
                self.assertEqual(run.call_args.args[0][-2:], ["--no-sources", str(symbols)])
            self.assertEqual(symbols.read_bytes(), b"retained native debug")

    def test_rejects_missing_artifacts(self):
        with patch.object(SYMBOLS, "cli") as cli, \
                patch.object(SYMBOLS.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "Native debug symbols are missing"):
                SYMBOLS.upload(self.root / "missing", self.environment)
        cli.assert_not_called()
        run.assert_not_called()

    def test_accepts_mixed_directory(self):
        files = [self.root / "sailry-host.debug", self.root / "sailry_host.pdb"]
        for symbols in files:
            symbols.write_bytes(b"retained native debug")
        with patch.object(SYMBOLS, "cli", return_value=Path("fixture-cli")), \
                patch.object(SYMBOLS.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run, \
                patch("sys.stdout", new_callable=io.StringIO):
            self.assertTrue(SYMBOLS.upload(self.root, self.environment))
        command = run.call_args.args[0]
        self.assertEqual(command[-2:], ["--no-sources", str(self.root)])
        self.assertEqual(self.payload.read_bytes(), b"retained native debug")
        for symbols in files:
            self.assertEqual(symbols.read_bytes(), b"retained native debug")


class Workflow(unittest.TestCase):
    def test_preserves_symbols_without_upload_credentials(self):
        for name, job in [("release.yml", "build"), ("release-host.yml", "build"),
                          ("release-host.yml", "host")]:
            with self.subTest(workflow=name, job=job):
                source = (ROOT / ".github/workflows" / name).read_text()
                section = re.split(r"\n  [a-z_]+:\n", source.split(f"\n  {job}:\n", 1)[1], maxsplit=1)[0]
                self.assertIn("CARGO_PROFILE_RELEASE_DEBUG: line-tables-only", section)
                self.assertIn("CARGO_PROFILE_RELEASE_STRIP: none", section)
                collect = section.index("scripts/package/debug-symbols.py collect")
                artifact = section.index("name: debug-symbols-")
                upload = section.index("scripts/package/debug-symbols.py upload")
                self.assertLess(collect, artifact)
                self.assertLess(artifact, upload)
                artifact_step = section[collect:upload]
                self.assertIn("if: always() && steps.symbols.outcome == 'success'", artifact_step)
                self.assertIn("if-no-files-found: error", artifact_step)
                self.assertNotIn("if: env.SENTRY", artifact_step)
                self.assertIn("SENTRY_AUTH_TOKEN: ${{ secrets.SENTRY_AUTH_TOKEN }}", section)
                self.assertIn("SENTRY_ORG:", section)
                self.assertIn("SENTRY_PROJECT:", section)
                self.assertNotIn("--include-sources", section)
                upload_step = section.rfind("      - name: Upload", artifact, upload)
                self.assertIn("continue-on-error: true", section[upload_step:upload])
                self.assertIn("if: steps.symbol-upload.outcome == 'failure'", section)
                self.assertIn("::warning::", section)
                self.assertNotIn("debug-symbols/", section.split("      - name: Build", 1)[0])

    def test_local_packages_collect_without_uploading(self):
        for name in ("package-macos.sh", "package-host.sh", "package-windows.ps1"):
            with self.subTest(script=name):
                source = (ROOT / "scripts" / name).read_text()
                self.assertIn("debug-symbols.py", source)
                self.assertIn("collect --optional", source)
                self.assertIn("target/debug-symbols/", source)
                self.assertNotIn("debug-symbols.py upload", source)


if __name__ == "__main__":
    unittest.main()
