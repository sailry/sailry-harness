import re
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]


class ReleaseWorkflowTests(unittest.TestCase):
    def test_integration_release_packages_still_belong_to_harness(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        packages = workflow.split("      RELEASE_PACKAGES: >-\n", 1)[1].split("    steps:\n", 1)[0]
        paths = re.findall(r"(?m)^        (\./\S+)\s*$", packages)
        self.assertTrue(paths)
        for path in paths:
            with self.subTest(package=path):
                self.assertTrue(any((ROOT / path).glob("*.go")), f"Missing Harness package: {path}")

    def test_historical_release_never_uploads_to_testflight_by_default(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertNotRegex(workflow, r"(?m)^  (push|pull_request|workflow_call):")
        request = workflow.split("      upload_testflight:\n", 1)[1].split("\n\n", 1)[0]
        self.assertIn("        type: boolean\n", request)
        self.assertRegex(request, r"(?m)^        default: false$")
        upload = workflow.split("      - name: Upload to TestFlight\n", 1)[1].split("      - name:", 1)[0]
        self.assertIn("        if: inputs.upload_testflight\n", upload)
        self.assertIn("APPLE_API_PRIVATE_KEY: ${{ secrets.APPLE_API_PRIVATE_KEY }}", upload)
        self.assertIn("xcrun altool --upload-", upload)


if __name__ == "__main__":
    unittest.main()
