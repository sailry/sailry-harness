"""Native libraries accompany cached Rust build-script outputs."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]


class MobileSpeech(unittest.TestCase):
    def setUp(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.mobile = workflow.split("\n  mobile:\n", 1)[1].split("\n  android:\n", 1)[0]

    def test_restores_before_bridge(self):
        restore = self.mobile.index("- name: Speech native libraries")
        bridge = self.mobile.index("- name: Native shared bridge")
        self.assertLess(restore, bridge)
        step = self.mobile[restore:].split("\n      - ", 1)[0]
        self.assertIn("uses: actions/cache/restore@", step)
        self.assertIn("id: speech-cache", step)
        self.assertIn("path: target/sherpa-onnx-prebuilt", step)
        self.assertIn("key: speech-aarch64-apple-darwin-${{ hashFiles('Cargo.lock') }}", step)

    def test_prepares_and_saves_misses(self):
        prepare = self.mobile.index("- name: Prepare uncached speech libraries")
        save = self.mobile.index("- name: Save speech native libraries")
        bridge = self.mobile.index("- name: Native shared bridge")
        self.assertLess(prepare, save)
        self.assertLess(save, bridge)
        step = self.mobile[prepare:].split("\n      - ", 1)[0]
        self.assertIn("if: steps.speech-cache.outputs.cache-hit != 'true'", step)
        self.assertIn("cargo clean --locked -p sherpa-onnx-sys", step)
        self.assertIn("cargo build --locked -p sailry-speech --jobs 1", step)
        step = self.mobile[save:].split("\n      - ", 1)[0]
        self.assertIn("if: steps.speech-cache.outputs.cache-hit != 'true'", step)
        self.assertIn("uses: actions/cache/save@", step)
        self.assertIn("path: target/sherpa-onnx-prebuilt", step)
        self.assertIn("key: ${{ steps.speech-cache.outputs.cache-primary-key }}", step)


if __name__ == "__main__":
    unittest.main()
