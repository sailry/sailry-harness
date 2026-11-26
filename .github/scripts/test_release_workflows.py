import re
from pathlib import Path
import unittest

from library_release_notes import release_notes


ROOT = Path(__file__).resolve().parents[2]


class ReleaseWorkflowTests(unittest.TestCase):
    def test_workflows_do_not_fetch_package_submodules(self):
        for path in sorted((ROOT / ".github/workflows").glob("*.yml")):
            with self.subTest(workflow=path.name):
                self.assertNotRegex(path.read_text(), r"(?m)^\s+submodules:\s*(?:recursive|true)\s*$")

    def test_default_integration_packages_belong_to_harness(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        packages = workflow.split("      DEFAULT_INTEGRATION_PACKAGES: >-\n", 1)[1].split("    steps:\n", 1)[0]
        paths = re.findall(r"(?m)^        (\./\S+)\s*$", packages)
        self.assertTrue(paths)
        for path in paths:
            with self.subTest(package=path):
                self.assertTrue(any((ROOT / path).glob("*.go")), f"Missing Harness package: {path}")

    def test_library_notes_use_only_the_tagged_entry(self):
        changelog = "## [1.2.3](url)\n\n- Stable release.\n\n## [1.2.3-rc.1](url)\n\n- Preview.\n"
        self.assertEqual(release_notes("v1.2.3", changelog), "- Stable release.")
        self.assertEqual(release_notes("v1.2.3-rc.1", changelog), "- Preview.")
        for tag in ("v1.2.2", "1.2.3", "v1.02.3", "v1.2.3-rc..1", "v1.2.3-01", "v1.2.3+build"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release_notes(tag, changelog)
        with self.assertRaises(ValueError):
            release_notes("v1.2.3", "## [1.2.3](url)\n\n## [1.2.2](url)\n- Older.\n")


if __name__ == "__main__":
    unittest.main()
