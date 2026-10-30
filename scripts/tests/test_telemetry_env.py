import importlib.util
from pathlib import Path
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "telemetry-env.py"
SPEC = importlib.util.spec_from_file_location("telemetry_env", SOURCE)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class EnvironmentTests(unittest.TestCase):
    def test_preserves_explicit_overrides(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".env.telemetry").write_text(
                'SAILRY_SENTRY_DSN="synthetic-dsn"\n'
                "SAILRY_TELEMETRY_KEY=synthetic-key\n"
                "UNRELATED=ignored\n"
            )
            values = MODULE.environment(root, {"SAILRY_SENTRY_DSN": "", "PATH": "original"})
            self.assertEqual(values, {
                "SAILRY_SENTRY_DSN": "", "SAILRY_TELEMETRY_KEY": "synthetic-key", "PATH": "original"
            })

    def test_keeps_shell_substitution_literal(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".env.telemetry").write_text("SAILRY_TELEMETRY_KEY='$(touch sentinel)'\n")
            values = MODULE.environment(root, {})
            self.assertEqual(values["SAILRY_TELEMETRY_KEY"], "$(touch sentinel)")
            self.assertFalse((root / "sentinel").exists())

    def test_missing_file_keeps_environment(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(MODULE.environment(Path(directory), {"PATH": "original"}), {"PATH": "original"})


if __name__ == "__main__":
    unittest.main()
