"""Local recipes reject unsupported interpreters and preserve invocation arguments."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class Launcher(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.bin = Path(self.directory.name)

    def interpreter(self, name, supported):
        path = self.bin / name
        path.write_text(
            '#!/bin/sh\nif test "$1" = -c; then exit '
            + ("0" if supported else "1")
            + '; fi\nprintf "%s\\0" "$@"\n',
        )
        path.chmod(0o755)
        return path

    def run_launcher(self, explicit=None):
        environment = {**os.environ, "PATH": str(self.bin)}
        environment.pop("SAILRY_PYTHON", None)
        if explicit is not None:
            environment["SAILRY_PYTHON"] = str(explicit)
        return subprocess.run(
            ["/bin/bash", str(ROOT / "scripts/python.sh"), "script path.py", "项目 🙂"],
            env=environment, capture_output=True,
        )

    def test_selects_supported_candidate(self):
        self.interpreter("python3", False)
        self.interpreter("python3.12", True)
        result = self.run_launcher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.decode().split("\0"), ["script path.py", "项目 🙂", ""])

    def test_explicit_interpreter_does_not_fall_back(self):
        unsupported = self.interpreter("old-python", False)
        self.interpreter("python3.12", True)
        result = self.run_launcher(unsupported)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"Python 3.12", result.stderr)

    def test_reports_missing_interpreter(self):
        result = self.run_launcher()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"SAILRY_PYTHON", result.stderr)


if __name__ == "__main__":
    unittest.main()
