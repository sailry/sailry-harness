"""Failed updates restore the existing installation before a retry."""

import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import unittest

from scripts.tests.test_host_install import Fixture, ROOT, VERSION


class HostUpdate(Fixture, unittest.TestCase):
    def setUp(self):
        super().setUp()
        self.home.rename(self.root / "private home")
        self.home = self.root / "private home"
        self.environment["HOME"] = str(self.home)
        self.system = self.root / "system"
        self.program = self.system / "libexec/sailry"
        self.link = self.system / "bin/sailry"
        self.unit = self.system / "units/sailry-host.service"
        self.active = self.root / "active"
        self.failure = self.root / "failure"
        self.environment.update({
            "SAILRY_FIXTURE_ACTIVE": str(self.active),
            "SAILRY_FIXTURE_FAILURE": str(self.failure),
            "SAILRY_FIXTURE_PROGRAM": str(self.program),
        })
        self.tool("id", '#!/bin/sh\nif test "$1" = -u; then echo 0; else echo fixture; fi\n')
        self.installer = self.root / "install-host.sh"
        source = (ROOT / "scripts/install-host.sh").read_text()
        for previous, current in (
            ("/usr/local/libexec", self.system / "libexec"),
            ("/usr/local/bin/sailry", self.link),
            ("/etc/systemd/system/sailry-host.service", self.unit),
        ):
            source = source.replace(previous, str(current))
        self.installer.write_text(source)
        commands = {name: shutil.which(name) for name in ("mv", "cat", "ln", "tar", "chmod")}
        self.environment["SAILRY_FIXTURE_TOOLS"] = json.dumps(commands)
        self.tool("systemctl", f'#!{sys.executable}\n' + '''import os, pathlib, sys
arguments = sys.argv[1:]
if arguments[0] == '--user': arguments.pop(0)
action = arguments[0]
active = pathlib.Path(os.environ['SAILRY_FIXTURE_ACTIVE'])
failure = pathlib.Path(os.environ['SAILRY_FIXTURE_FAILURE'])
with open(os.environ['SAILRY_FIXTURE_LOG'], 'a') as log:
    log.write('systemctl ' + ' '.join(arguments) + '\\n')
if action in ('status', 'is-active'):
    raise SystemExit(0 if active.read_text() == 'active' else 3)
if action == 'stop': active.write_text('inactive')
if action in ('start', 'restart'): active.write_text('active')
phase = failure.read_text() if failure.exists() else ''
if (action == phase and phase in ('start', 'stop')) or (action == 'daemon-reload' and phase == 'reload'):
    failure.unlink()
    raise SystemExit(17)
''')
        shim = f'#!{sys.executable}\n' + '''import json, os, pathlib, subprocess, sys
name = pathlib.Path(sys.argv[0]).name
arguments = sys.argv[1:]
failure = pathlib.Path(os.environ['SAILRY_FIXTURE_FAILURE'])
phase = failure.read_text() if failure.exists() else ''
previous = os.environ['SAILRY_FIXTURE_PREVIOUS']
tools = json.loads(os.environ['SAILRY_FIXTURE_TOOLS'])
matches = (
    (name == 'mv' and phase == 'move' and arguments[0] == previous) or
    (name == 'mv' and phase == 'publish' and arguments[0].endswith('/package')) or
    (name == 'restorecon' and phase == 'permissions') or
    (name == 'cat' and phase == 'unit' and arguments[0].endswith('/service')) or
    (name == 'ln' and phase == 'link') or
    (name == 'tar' and phase == 'unpack') or
    (name == 'chmod' and phase == 'validate')
)
if matches:
    failure.unlink()
    if phase in ('move', 'link'):
        result = subprocess.run([tools[name], *arguments])
        if result.returncode: raise SystemExit(result.returncode)
    if phase == 'unit': sys.stdout.write('incomplete unit\\n')
    raise SystemExit(17)
if name != 'restorecon': os.execv(tools[name], [name, *arguments])
'''
        for command in (*commands, "restorecon"):
            self.tool(command, shim)

    def existing(self, *, legacy=True, active=True, command=True):
        self.previous = self.home / ".local/lib/sailry" if legacy else self.program
        self.environment["SAILRY_FIXTURE_PREVIOUS"] = str(self.previous)
        shutil.copytree(self.root / f"package-{VERSION}", self.previous, symlinks=True)
        (self.previous / "sailry").chmod(0o700)
        self.link.parent.mkdir(parents=True, exist_ok=True)
        if command:
            self.link.symlink_to(self.previous / "sailry")
        self.unit.parent.mkdir(parents=True, exist_ok=True)
        self.unit.write_text(
            '[Unit]\nDescription=Existing Host\n[Service]\n'
            f'ExecStart="{self.previous}/sailry-host" --data-dir "{self.home}/.sailry-host" '
            '--relay "https://relay.example/custom" --bootstrap\n'
            'Restart=on-failure\nUMask=0077\n[Install]\nWantedBy=multi-user.target\n'
        )
        self.unit.chmod(0o600)
        self.active.write_text("active" if active else "inactive")
        profile = self.home / ".sailry-host"
        profile.mkdir()
        (profile / "link.key").write_bytes(b"existing identity")
        (profile / "marker").write_bytes(b"existing data")
        self.retained = self.program.parent / ".sailry-previous-retained"
        self.retained.mkdir(parents=True)
        (self.retained / "marker").write_bytes(b"previous backup")

    def update(self):
        return subprocess.run(["bash", str(self.installer), "--update", "--version", VERSION],
                              env=self.environment, capture_output=True, text=True, timeout=20)

    @staticmethod
    def tree(path):
        return {
            str(child.relative_to(path)): (
                stat.S_IMODE(child.lstat().st_mode),
                os.readlink(child) if child.is_symlink() else child.read_bytes() if child.is_file() else None,
            )
            for child in path.rglob("*")
        }

    def fails(self, phase, *, legacy=True, active=True, command=True):
        self.existing(legacy=legacy, active=active, command=command)
        before = self.tree(self.previous)
        unit = self.unit.read_bytes()
        profile = self.tree(self.home / ".sailry-host")
        self.failure.write_text(phase)
        result = self.update()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertFalse(self.failure.exists(), result.stderr)
        self.assertEqual(self.tree(self.previous), before)
        self.assertEqual(self.unit.read_bytes(), unit)
        self.assertEqual(stat.S_IMODE(self.unit.stat().st_mode), 0o600)
        self.assertEqual(self.active.read_text(), "active" if active else "inactive")
        self.assertEqual(self.link.is_symlink(), command)
        if command:
            self.assertEqual(self.link.readlink(), self.previous / "sailry")
        if legacy:
            self.assertFalse(self.program.exists())
        self.assertEqual(self.tree(self.home / ".sailry-host"), profile)
        self.assertEqual(list(self.program.parent.glob(".sailry-previous-*")), [self.retained])
        self.assertFalse(list(self.program.parent.glob(".sailry-install-*")))
        self.assertEqual((self.retained / "marker").read_bytes(), b"previous backup")
        if phase in ("unpack", "validate"):
            self.assertNotIn("systemctl stop", self.log.read_text())
        else:
            self.assertIn("the previous installation was restored", result.stderr)
        retry = self.update()
        self.assertEqual(retry.returncode, 0, retry.stderr)
        self.assertEqual(self.link.readlink(), self.program / "sailry")
        self.assertEqual(self.active.read_text(), "active")
        self.assertEqual(self.tree(self.home / ".sailry-host"), profile)
        expected = unit.replace(str(self.previous).encode(), str(self.program).encode()) if legacy else unit
        self.assertEqual(self.unit.read_bytes(), expected)
        backups = set(self.program.parent.glob(".sailry-previous-*")) - {self.retained}
        self.assertEqual(len(backups), 1)
        self.assertEqual(self.tree(backups.pop()), before)
        self.assertEqual((self.retained / "marker").read_bytes(), b"previous backup")

    def test_restores_after_stop_failure(self):
        self.fails("stop")

    def test_restores_after_old_tree_move(self):
        self.fails("move")

    def test_restores_after_publish_failure(self):
        self.fails("publish")

    def test_restores_after_permission_failure(self):
        self.fails("permissions")

    def test_restores_partial_unit_writes(self):
        self.fails("unit")

    def test_restores_after_reload_failure(self):
        self.fails("reload")

    def test_restores_after_link_failure(self):
        self.fails("link")

    def test_restores_after_partial_start(self):
        self.fails("start")

    def test_preserves_inactive_services(self):
        self.fails("start", active=False)

    def test_restores_normal_updates(self):
        self.fails("start", legacy=False)

    def test_preserves_missing_managed_links(self):
        self.fails("start", legacy=False, command=False)

    def test_unpack_failure_preserves_service(self):
        self.fails("unpack")

    def test_validation_precedes_service_stop(self):
        self.fails("validate")

    def test_refuses_unmanaged_service_paths(self):
        self.existing()
        self.unit.write_bytes(self.unit.read_bytes().replace(b'/sailry-host" ', b'/custom-host" '))
        unit = self.unit.read_bytes()
        before = self.tree(self.previous)
        profile = self.tree(self.home / ".sailry-host")
        result = self.update()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match its managed program path", result.stderr)
        self.assertNotIn("systemctl stop", self.log.read_text())
        self.assertEqual(self.unit.read_bytes(), unit)
        self.assertEqual(self.tree(self.previous), before)
        self.assertEqual(self.tree(self.home / ".sailry-host"), profile)
        self.assertEqual(self.link.readlink(), self.previous / "sailry")
        self.assertEqual(self.active.read_text(), "active")
        self.assertFalse(self.program.exists())
        self.assertEqual(list(self.program.parent.glob(".sailry-previous-*")), [self.retained])


class MacUpdate(Fixture, unittest.TestCase):
    def setUp(self):
        super().setUp()
        self.home.rename(self.root / "private home")
        self.home = self.root / "private home"
        self.environment["HOME"] = str(self.home)
        self.program = self.home / ".local/lib/sailry"
        self.link = self.home / ".local/bin/sailry"
        self.unit = self.home / "Library/LaunchAgents/ai.sailry.host.plist"
        self.active = self.root / "active"
        self.failure = self.root / "failure"
        self.environment.update({
            "SAILRY_FIXTURE_ACTIVE": str(self.active),
            "SAILRY_FIXTURE_FAILURE": str(self.failure),
        })
        self.tool("uname", '#!/bin/sh\nif test "$1" = -s; then echo Darwin; else echo arm64; fi\n')
        self.tool("launchctl", f'#!{sys.executable}\n' + '''import os, pathlib, sys
arguments = sys.argv[1:]
action = arguments[0]
active = pathlib.Path(os.environ['SAILRY_FIXTURE_ACTIVE'])
failure = pathlib.Path(os.environ['SAILRY_FIXTURE_FAILURE'])
phase = failure.read_text() if failure.exists() else ''
with open(os.environ['SAILRY_FIXTURE_LOG'], 'a') as log:
    log.write('launchctl ' + ' '.join(arguments) + '\\n')
if action == 'print':
    if arguments[1] == 'gui/1000': raise SystemExit(0)
    raise SystemExit(0 if active.read_text() == 'active' else 113)
if action == 'bootout':
    if phase in ('stop', 'rollback-stop'):
        failure.unlink()
        sys.stderr.write('fixture launchctl permission failure\\n')
        raise SystemExit(17)
    if active.read_text() == 'inactive':
        with open(os.environ['SAILRY_FIXTURE_LOG'], 'a') as log: log.write('bootout absent\\n')
        sys.stderr.write('Boot-out failed: 3: No such process\\n')
        raise SystemExit(3)
    active.write_text('inactive')
if action == 'bootstrap':
    if phase in ('start', 'start-active', 'recovery'):
        if phase == 'start-active': active.write_text('active')
        if phase == 'recovery': failure.write_text('rollback-stop')
        else: failure.unlink()
        sys.stderr.write('fixture launchctl bootstrap failure\\n')
        raise SystemExit(17)
    active.write_text('active')
''')
        target = "aarch64-apple-darwin"
        original = self.assets / "sailry-host-aarch64-unknown-linux-gnu.tar.gz"
        archive = self.assets / f"sailry-host-{target}.tar.gz"
        shutil.copyfile(original, archive)
        checksum = (self.assets / "SHA256SUMS-aarch64-unknown-linux-gnu").read_text()
        (self.assets / f"SHA256SUMS-{target}").write_text(
            checksum.replace("aarch64-unknown-linux-gnu", target))

    def existing(self, active):
        shutil.copytree(self.root / f"package-{VERSION}", self.program, symlinks=True)
        (self.program / "sailry").chmod(0o700)
        self.link.parent.mkdir(parents=True)
        self.link.symlink_to(self.program / "sailry")
        self.unit.parent.mkdir(parents=True)
        self.unit.write_bytes(b"existing launch agent and network settings\n")
        self.unit.chmod(0o600)
        profile = self.home / ".sailry-host"
        profile.mkdir()
        (profile / "link.key").write_bytes(b"existing identity")
        (profile / "marker").write_bytes(b"existing data")
        self.active.write_text("active" if active else "inactive")
        self.retained = self.program.parent / ".sailry-previous-retained"
        self.retained.mkdir()
        (self.retained / "marker").write_bytes(b"previous backup")

    def update(self):
        return self.install("--update")

    def fails(self, phase, *, active=True):
        self.existing(active)
        before = HostUpdate.tree(self.program)
        profile = HostUpdate.tree(self.home / ".sailry-host")
        unit = self.unit.read_bytes()
        self.failure.write_text(phase)
        result = self.update()
        self.assertEqual(result.returncode, 17, result.stderr)
        self.assertFalse(self.failure.exists(), result.stderr)
        self.assertIn("the previous installation was restored", result.stderr)
        self.assertEqual(HostUpdate.tree(self.program), before)
        self.assertEqual(self.link.readlink(), self.program / "sailry")
        self.assertEqual(self.unit.read_bytes(), unit)
        self.assertEqual(stat.S_IMODE(self.unit.stat().st_mode), 0o600)
        self.assertEqual(HostUpdate.tree(self.home / ".sailry-host"), profile)
        self.assertEqual(self.active.read_text(), "active" if active else "inactive")
        self.assertEqual(list(self.program.parent.glob(".sailry-previous-*")), [self.retained])
        self.assertFalse(list(self.program.parent.glob(".sailry-install-*")))
        retry = self.update()
        self.assertEqual(retry.returncode, 0, retry.stderr)
        self.assertEqual(self.active.read_text(), "active")
        self.assertEqual(HostUpdate.tree(self.home / ".sailry-host"), profile)
        self.assertEqual(self.unit.read_bytes(), unit)
        backups = set(self.program.parent.glob(".sailry-previous-*")) - {self.retained}
        self.assertEqual(len(backups), 1)
        self.assertEqual(HostUpdate.tree(backups.pop()), before)
        self.assertEqual((self.retained / "marker").read_bytes(), b"previous backup")
        return result

    def test_restores_after_unloaded_start_failure(self):
        self.fails("start")
        self.assertIn("bootout absent", self.log.read_text())

    def test_restores_after_partial_start(self):
        self.fails("start-active")

    def test_preserves_inactive_services(self):
        self.fails("start", active=False)
        self.assertIn("bootout absent", self.log.read_text())

    def test_keeps_real_stop_failures(self):
        result = self.fails("stop")
        self.assertIn("fixture launchctl permission failure", result.stderr)

    def test_retains_files_after_recovery_failure(self):
        self.existing(True)
        before = HostUpdate.tree(self.program)
        profile = HostUpdate.tree(self.home / ".sailry-host")
        self.failure.write_text("recovery")
        result = self.update()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.failure.exists(), result.stderr)
        self.assertIn("Host recovery failed", result.stderr)
        self.assertIn("fixture launchctl permission failure", result.stderr)
        self.assertNotIn("the previous installation was restored", result.stderr)
        backups = set(self.program.parent.glob(".sailry-previous-*")) - {self.retained}
        self.assertEqual(len(backups), 1)
        self.assertEqual(HostUpdate.tree(backups.pop()), before)
        self.assertEqual(HostUpdate.tree(self.home / ".sailry-host"), profile)
        self.assertEqual(len(list(self.program.parent.glob(".sailry-install-*"))), 1)


if __name__ == "__main__":
    unittest.main()
