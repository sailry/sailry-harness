"""Every Node integration feature belongs to one compiled test suite."""

from collections import Counter
from pathlib import Path
import re
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2] / "crates/node-runtime"


class Registration(unittest.TestCase):
    def test_includes_each_feature_once(self):
        manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
        self.assertFalse(manifest["package"]["autotests"])
        features = set((ROOT / "tests").glob("*.rs"))
        registered = []
        suites = manifest["test"]
        self.assertEqual(len({suite["name"] for suite in suites}), len(suites))
        for suite in suites:
            path = ROOT / suite["path"]
            self.assertTrue(path.is_file(), path)
            if path.parent == ROOT / "tests":
                registered.append(path)
            else:
                for source in re.findall(r'#\[path = "([^"]+)"\]', path.read_text()):
                    feature = (path.parent / source).resolve()
                    self.assertIn(feature, features)
                    registered.append(feature)
        self.assertEqual(set(registered), features)
        self.assertTrue(all(count == 1 for count in Counter(registered).values()))


if __name__ == "__main__":
    unittest.main()
