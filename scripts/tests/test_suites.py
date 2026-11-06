"""Every integration feature in a crate without test discovery belongs to one compiled suite."""

from collections import Counter
from pathlib import Path
import re
import tempfile
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]


def registered_crates():
    """Crates that disable Cargo's automatic integration-test discovery."""
    crates = []
    for manifest in sorted(ROOT.glob("*/*/Cargo.toml")):
        package = tomllib.loads(manifest.read_text()).get("package", {})
        if package.get("autotests") is False:
            crates.append(manifest.parent)
    return crates


def compiled_features(crate, suite):
    """Top-level feature files a suite compiles, through `#[path]` or sibling `mod` items."""
    path = crate / suite["path"]
    tests = crate / "tests"
    if path.parent == tests:
        # A suite root's `mod` items without `#[path]` resolve to sibling feature files.
        source = path.read_text()
        modules = []
        for item in re.finditer(r"^mod (\w+);", source, re.MULTILINE):
            before = source[:item.start()]
            attributes = before[max(before.rfind(";"), before.rfind("}")) + 1:]
            if "#[path" not in attributes and (tests / f"{item.group(1)}.rs").is_file():
                modules.append(tests / f"{item.group(1)}.rs")
        return [path, *modules]
    return [(path.parent / source).resolve()
            for source in re.findall(r'#\[path = "([^"]+)"\]', path.read_text())]


class Registration(unittest.TestCase):
    def test_covers_node_runtime(self):
        self.assertIn(ROOT / "crates/node-runtime", registered_crates())

    def test_includes_each_feature_once(self):
        for crate in registered_crates():
            with self.subTest(crate=crate.name):
                suites = tomllib.loads((crate / "Cargo.toml").read_text())["test"]
                self.assertEqual(len({suite["name"] for suite in suites}), len(suites))
                tests = crate / "tests"
                features = set(tests.glob("*.rs"))
                registered = []
                for suite in suites:
                    self.assertTrue((crate / suite["path"]).is_file(), suite["path"])
                    for feature in compiled_features(crate, suite):
                        self.assertTrue(feature.is_file(), feature)
                        self.assertTrue(feature.is_relative_to(tests), feature)
                        if feature.parent == tests:
                            registered.append(feature)
                self.assertEqual(set(registered), features)
                self.assertTrue(all(count == 1 for count in Counter(registered).values()))


class Layouts(unittest.TestCase):
    def test_reads_module_and_path_suites(self):
        with tempfile.TemporaryDirectory() as directory:
            crate = Path(directory)
            (crate / "tests/suites").mkdir(parents=True)
            (crate / "tests/transport.rs").write_text("mod connections;\nmod streams;\n")
            (crate / "tests/connections.rs").write_text("")
            (crate / "tests/streams.rs").write_text("")
            (crate / "tests/suites/node.rs").write_text('#[path = "../agent.rs"]\nmod agent;\n')
            (crate / "tests/single.rs").write_text("#[test]\nfn passes() {}\n")
            self.assertEqual(compiled_features(crate, {"path": "tests/transport.rs"}),
                             [crate / "tests/transport.rs", crate / "tests/connections.rs", crate / "tests/streams.rs"])
            self.assertEqual(compiled_features(crate, {"path": "tests/suites/node.rs"}),
                             [(crate / "tests/agent.rs").resolve()])
            self.assertEqual(compiled_features(crate, {"path": "tests/single.rs"}), [crate / "tests/single.rs"])


if __name__ == "__main__":
    unittest.main()
