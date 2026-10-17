"""Local recipes reuse CI commands without changing package and target boundaries."""

import importlib.util
from pathlib import Path
import re
import shlex
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("ci", ROOT / "scripts/ci.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


def recipe(name):
    source = (ROOT / "justfile").read_text()
    body = re.search(rf"^{re.escape(name)}(?=[ :])[^\n]*\n((?:[ \t]+[^\n]*\n)+)", source, re.MULTILINE)
    if body is None:
        raise AssertionError(f"Missing recipe: {name}")
    return [shlex.split(line.strip()) for line in body.group(1).splitlines()]


def commands(name):
    result = []
    for arguments in recipe(name):
        if arguments[:4] != ["bash", "scripts/python.sh", "scripts/ci.py", "check"]:
            raise AssertionError(f"Recipe does not use the shared check runner: {name}")
        result.extend(ci.check_commands(arguments[4], root=ROOT))
    return result


def packages(arguments):
    return {arguments[index + 1] for index, value in enumerate(arguments) if value == "-p"}


class UI(unittest.TestCase):
    def test_preserves_packages_and_targets(self):
        selected = commands("test-ui")
        self.assertEqual(len(selected), 1)
        arguments = selected[0]["argv"]
        self.assertEqual(arguments[:3], ["cargo", "nextest", "run"])
        self.assertEqual(packages(arguments), {"sailry-content", "sailry-desktop"})
        self.assertIn("--lib", arguments)
        self.assertEqual(arguments[arguments.index("--bin") + 1], "sailry-desktop")
        self.assertNotIn("--test", arguments)
        self.assertNotIn("--release", arguments)
        self.assertEqual(arguments[arguments.index("--profile") + 1], "ci")

    def test_preserves_backend_graph(self):
        selected = commands("test-backend")
        self.assertEqual(len(selected), 3)
        arguments = selected[0]["argv"]
        self.assertEqual(arguments[:3], ["cargo", "nextest", "run"])
        self.assertEqual(packages(arguments), {
            "sailry-protocol", "sailry-link", "sailry-client",
            "sailry-node-runtime", "sailry-release",
        })
        self.assertEqual(arguments[arguments.index("--profile") + 1], "ci")
        self.assertNotIn("--release", arguments)

    def test_preserves_mobile_checks(self):
        selected = commands("check-mobile")
        self.assertEqual(len(selected), 3)
        self.assertTrue(all(command["cwd"] == "apps/mobile" for command in selected))
        formatting, analysis, tests = [command["argv"] for command in selected]
        self.assertEqual(formatting[:2], ["dart", "format"])
        self.assertIn("--set-exit-if-changed", formatting)
        self.assertEqual(set(formatting[4:]), {"lib", "preview", "test", "integration_test", "tool"})
        self.assertEqual(analysis, ["flutter", "analyze", "--no-pub"])
        self.assertEqual(tests, ["flutter", "test", "--no-pub"])
        self.assertIn("check-mobile: mobile-deps", (ROOT / "justfile").read_text())

    def test_forwards_local_arguments(self):
        source = (ROOT / "justfile").read_text()
        self.assertIn("[positional-arguments]\nci *args:", source)
        self.assertEqual(recipe("ci"), [["bash", "scripts/python.sh", "scripts/ci.py", "local", "$@"]])

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
