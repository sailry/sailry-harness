"""Rust jobs fit standard runner resources without changing acceptance checks."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]


class RustResources(unittest.TestCase):
    def setUp(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.rust = workflow.split("\n  rust:\n", 1)[1].split("\n  mobile:\n", 1)[0]

    def test_caps_compilation(self):
        self.assertIn("CARGO_BUILD_JOBS: '2'", self.rust)
        self.assertIn(
            "BUILD_JOBS: ${{ matrix.kind == 'desktop' && 1 || matrix.platform.jobs }}",
            self.rust,
        )
        commands = [
            line for line in self.rust.splitlines()
            if "cargo " in line and "cargo clean " not in line and "cargo fmt " not in line
        ]
        self.assertEqual(len(commands), 8)
        self.assertTrue(all('--jobs "$BUILD_JOBS"' in command for command in commands))
        cache_env = next(line for line in self.rust.splitlines() if "env-vars:" in line)
        self.assertNotIn("BUILD_JOBS", cache_env)

    def test_bounds_fixture_parallelism(self):
        matrix = self.rust.split("    runs-on:", 1)[0]
        arm, intel = matrix.split("          - runner: macos-15-intel", 1)
        self.assertIn("threads: 4", arm)
        self.assertIn("threads: 2", intel)
        self.assertIn("--test-threads=${{ matrix.platform.threads }}", self.rust)
        self.assertIn("-p sailry-desktop --jobs \"$BUILD_JOBS\" -- --test-threads=1", self.rust)


if __name__ == "__main__":
    unittest.main()
