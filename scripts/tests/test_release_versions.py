"""Distribution metadata uses isolated manifests and never changes product tags."""

import importlib.util
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("versions", ROOT / "scripts/package/versions.py")
versions = importlib.util.module_from_spec(spec)
spec.loader.exec_module(versions)


class NativeVersions(unittest.TestCase):
    def test_preserves_core_for_stable_and_preview_releases(self):
        for value in ("0.1.0", "0.1.0-alpha.1", "0.1.0-beta.10", "1.2.3-rc.1"):
            with self.subTest(value=value):
                self.assertEqual(versions.apple(value), value.split("-", 1)[0])

    def test_rejects_invalid_identifiers(self):
        for value in ("0.1", "v0.1.0", "01.1.0", "0.1.0-alpha.01", "0.1.0-bad_name"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                versions.apple(value)

    def test_previews_do_not_replace_latest_stable(self):
        self.assertEqual(versions.release_flags("0.1.0-alpha.1"), ["--prerelease", "--latest=false"])
        self.assertEqual(versions.release_flags("0.1.0"), ["--latest"])


class ProductVersions(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        for app in ("desktop", "host", "mobile"):
            (self.root / f"apps/{app}").mkdir(parents=True)
        for app in ("desktop", "host"):
            (self.root / f"apps/{app}/Cargo.toml").write_text('[package]\nversion = "0.1.0-alpha.1"\n')
        (self.root / "apps/mobile/pubspec.yaml").write_text('name: fixture\nversion: 0.1.0-alpha.1+1\n')

    def test_matches_every_application_and_build_counter(self):
        self.assertEqual(versions.product(self.root), ("0.1.0-alpha.1", 1))

    def test_reports_mismatched_versions_and_invalid_counters(self):
        for value in ("0.1.0-alpha.2+1", "0.1.0-alpha.1", "0.1.0-alpha.1+0", "0.1.0-alpha.1+invalid"):
            (self.root / "apps/mobile/pubspec.yaml").write_text(f"version: {value}\n")
            with self.subTest(value=value), self.assertRaises(ValueError):
                versions.product(self.root)


if __name__ == "__main__":
    unittest.main()
