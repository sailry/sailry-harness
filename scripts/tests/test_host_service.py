"""Service acceptance refuses unsupported environments and existing installations."""

import importlib.util
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("host_service", ROOT / "scripts/check-host-install.py")
service = importlib.util.module_from_spec(spec)
spec.loader.exec_module(service)


class HostService(unittest.TestCase):
    def test_requires_a_root_systemd_runner(self):
        for platform, uid in (("darwin", 0), ("linux", 1000)):
            with self.subTest(platform=platform, uid=uid), \
                    mock.patch.object(service.sys, "platform", platform), \
                    mock.patch.object(service.os, "geteuid", return_value=uid), \
                    mock.patch.object(service.subprocess, "run") as command, \
                    mock.patch.object(service.tempfile, "mkdtemp") as temporary:
                with self.assertRaisesRegex(ValueError, "isolated root-owned"):
                    service.check(Path("unused"))
                command.assert_not_called()
                temporary.assert_not_called()

    def test_preserves_existing_service_and_command(self):
        with tempfile.TemporaryDirectory(prefix="sailry-service-guard-") as directory:
            root = Path(directory)
            unit = root / "unit"
            executable = root / "sailry"
            unit.write_bytes(b"preserve service")
            executable.write_bytes(b"preserve command")
            with mock.patch.object(service.sys, "platform", "linux"), \
                    mock.patch.object(service.os, "geteuid", return_value=0), \
                    mock.patch.object(Path, "is_dir", return_value=True), \
                    mock.patch.object(service, "UNIT", unit), \
                    mock.patch.object(service, "COMMAND", executable), \
                    mock.patch.object(service.subprocess, "run", return_value=SimpleNamespace(returncode=1)), \
                    mock.patch.object(service.tempfile, "mkdtemp") as temporary:
                with self.assertRaisesRegex(ValueError, "existing Host"):
                    service.check(Path("unused"))
                temporary.assert_not_called()
            self.assertEqual(unit.read_bytes(), b"preserve service")
            self.assertEqual(executable.read_bytes(), b"preserve command")


if __name__ == "__main__":
    unittest.main()
