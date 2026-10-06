"""Installer and service commands run in isolated homes with fixture release downloads."""

import hashlib
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
TARGETS = {"x86_64":"x86_64-unknown-linux-gnu", "aarch64":"aarch64-unknown-linux-gnu"}
VERSION = "0.1.0-alpha.1"


class HostInstall(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="sailry-install-fixture-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.assets = self.root / "assets"
        self.assets.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.log = self.root / "commands"
        self.environment = {**os.environ, "HOME":str(self.home), "PATH":f"{self.bin}:{os.environ['PATH']}",
                            "SAILRY_FIXTURE_ASSETS":str(self.assets), "SAILRY_FIXTURE_LOG":str(self.log), "SAILRY_FIXTURE_ARCH":"x86_64"}
        self.tool("uname", '#!/bin/sh\nif test "$1" = -s; then echo Linux; else echo "$SAILRY_FIXTURE_ARCH"; fi\n')
        self.tool("id", '#!/bin/sh\nif test "$1" = -u; then echo 1000; else echo fixture; fi\n')
        for command in ("systemctl", "loginctl"):
            self.tool(command, '#!/bin/sh\nprintf "%s\\n" "$*" >> "$SAILRY_FIXTURE_LOG"\n')
        self.tool("curl", '''#!/usr/bin/env python3
import os, pathlib, shutil, sys
arguments = sys.argv[1:]
url = next(argument for argument in arguments if argument.startswith('https://'))
if not url.startswith('https://github.com/sailry/sailry-harness/releases/download/v'):
    raise SystemExit('Unexpected release origin')
asset = url.rsplit('/', 1)[1]
source = pathlib.Path(os.environ['SAILRY_FIXTURE_ASSETS']) / asset
destination = pathlib.Path(arguments[arguments.index('--output') + 1])
with open(os.environ['SAILRY_FIXTURE_LOG'], 'a') as log: log.write(url + '\\n')
if not source.is_file(): raise SystemExit('Missing fixture release asset')
shutil.copyfile(source, destination)
''')
        self.release(VERSION)

    def tool(self, name, content):
        path = self.bin / name
        path.write_text(content)
        path.chmod(0o755)

    def release(self, version):
        package = self.root / f"package-{version}"
        package.mkdir()
        binary = package / "sailry-host"
        binary.write_text(f'#!/bin/sh\ntest "$1" = --version\necho "Sailry Host {version}"\n')
        binary.chmod(0o755)
        (package / "sailry").write_bytes((ROOT / "scripts/host-command.sh").read_bytes())
        (package / "install-host.sh").write_bytes((ROOT / "scripts/install-host.sh").read_bytes())
        (package / "service.sh").write_bytes((ROOT / "crates/node-runtime/src/ssh/install/service.sh").read_bytes())
        for target in TARGETS.values():
            name = f"sailry-host-{version}-{target}.tar.gz"
            archive = self.assets / name
            with tarfile.open(archive, "w:gz") as output:
                for path in package.iterdir(): output.add(path, arcname=path.name)
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            (self.assets / f"SHA256SUMS-{target}").write_text(f"{digest}  {name}\n")

    def install(self, *arguments):
        return subprocess.run(["bash", str(ROOT / "scripts/install-host.sh"), "--version", VERSION, *arguments],
                              env=self.environment, capture_output=True, text=True, timeout=20)

    def command(self, *arguments):
        return subprocess.run([str(self.home / ".local/bin/sailry"), *arguments], env=self.environment,
                              capture_output=True, text=True, timeout=20)

    def test_installs_user_services_on_both_linux_architectures(self):
        for architecture, target in TARGETS.items():
            with self.subTest(architecture=architecture):
                self.environment["SAILRY_FIXTURE_ARCH"] = architecture
                result = self.install()
                self.assertEqual(result.returncode, 0, result.stderr)
                unit = self.home / ".config/systemd/user/sailry-host.service"
                self.assertIn('--internet --bootstrap', unit.read_text())
                self.assertIn('--user enable --now sailry-host.service', self.log.read_text())
                self.assertIn(f"-{target}.tar.gz", self.log.read_text())
                self.assertEqual(self.command("version").stdout.strip(), f"Sailry Host {VERSION}")
                # Each architecture uses a different fresh home, not profile deletion.
                self.home = self.root / f"home-{architecture}"
                self.home.mkdir()
                self.environment["HOME"] = str(self.home)

    def test_service_commands_use_the_existing_user_service(self):
        self.assertEqual(self.install().returncode, 0)
        for command in ("start", "stop", "restart", "status"):
            self.assertEqual(self.command(command).returncode, 0)
            self.assertIn(f"--user {command} sailry-host.service", self.log.read_text())

    def test_existing_data_and_unrelated_command_are_preserved(self):
        profile = self.home / ".sailry-host"
        profile.mkdir()
        marker = profile / "user-data"
        marker.write_bytes(b"preserve")
        self.assertNotEqual(self.install().returncode, 0)
        self.assertEqual(marker.read_bytes(), b"preserve")
        self.assertFalse(self.log.exists())
        profile.rename(self.home / "saved-profile")
        command = self.home / ".local/bin/sailry"
        command.parent.mkdir(parents=True)
        command.write_bytes(b"unrelated command")
        self.assertNotEqual(self.install().returncode, 0)
        self.assertEqual(command.read_bytes(), b"unrelated command")

    def test_checksum_failure_never_installs_or_starts(self):
        checksum = self.assets / "SHA256SUMS-x86_64-unknown-linux-gnu"
        checksum.write_text("0" * 64 + f"  sailry-host-{VERSION}-x86_64-unknown-linux-gnu.tar.gz\n")
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum does not match", result.stderr)
        self.assertFalse((self.home / ".local/lib/sailry").exists())
        self.assertFalse((self.home / ".sailry-host").exists())
        self.assertNotIn("enable --now", self.log.read_text())

    def test_update_restarts_without_changing_profile_data(self):
        self.assertEqual(self.install().returncode, 0)
        profile = self.home / ".sailry-host"
        marker = profile / "user-data"
        marker.write_bytes(b"preserve")
        version = "0.1.0-alpha.2"
        self.release(version)
        result = self.command("update", "--version", version)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(marker.read_bytes(), b"preserve")
        self.assertEqual(self.command("version").stdout.strip(), f"Sailry Host {version}")
        self.assertIn("--user stop sailry-host.service", self.log.read_text())
        self.assertIn("--user start sailry-host.service", self.log.read_text())
        backups = list((self.home / ".local/lib").glob(".sailry-previous-*"))
        self.assertEqual(len(backups), 1)
        self.assertTrue((backups[0] / "sailry-host").is_file())

    def test_invalid_version_does_not_download_or_mutate(self):
        result = subprocess.run(["bash", str(ROOT / "scripts/install-host.sh"), "--version", "../other"],
                                env=self.environment, capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main()
