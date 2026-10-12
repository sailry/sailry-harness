"""UI tasks preserve the CI runner's coverage and bounded concurrency."""

from pathlib import Path
import re
import shlex
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]


class UI(unittest.TestCase):
    def test_preserves_packages_and_targets(self):
        recipe = (ROOT / "justfile").read_text().split("\ntest-ui:\n", 1)[1].split("\n\n", 1)[0]
        self.assertEqual(shlex.split(recipe), [
            "cargo", "nextest", "run", "--locked",
            "-p", "sailry-content", "-p", "sailry-desktop",
            "--lib", "--bin", "sailry-desktop", "--profile", "ci",
        ])

    def test_reuses_bounded_profile(self):
        config = tomllib.loads((ROOT / ".config/nextest.toml").read_text())
        profile = config["profile"]["ci"]
        self.assertEqual(profile["test-threads"], 2)
        self.assertEqual(profile["retries"], 0)
        self.assertNotIn("default-filter", profile)

    def test_documents_pinned_runner(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        version = re.search(r"tool: cargo-nextest@([0-9.]+)", workflow).group(1)
        self.assertIn(
            f"cargo install --locked cargo-nextest --version {version}",
            (ROOT / "CONTRIBUTING.md").read_text(),
        )


if __name__ == "__main__":
    unittest.main()
