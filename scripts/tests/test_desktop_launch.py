"""Development recipes reuse the application default unless a profile is explicit."""

from pathlib import Path
import shlex
import subprocess
import unittest


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


if __name__ == "__main__":
    unittest.main()
