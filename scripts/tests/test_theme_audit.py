"""Generic theme audits use temporary palettes, never distributable packages."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("theme_audit", ROOT / "scripts/check-theme.py")
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def fixture():
    return {mode: {"theme": {"colors": {"background": "#FFFFFF", "foreground": "#000000"}}}
            for mode in ("light", "dark")}


class Colors(unittest.TestCase):
    def test_resolves_alpha(self):
        self.assertEqual(audit.rgba("#11223380"), [17 / 255, 34 / 255, 51 / 255, 128 / 255])
        self.assertEqual(audit.composite([0, 0, 0, 0.5], [1, 1, 1, 1]), [0.5, 0.5, 0.5, 1])

    def test_bounds_contrast(self):
        self.assertEqual(audit.contrast(audit.rgba("#FFFFFF"), audit.rgba("#000000")), 21)
        self.assertEqual(audit.contrast(audit.rgba("#112233"), audit.rgba("#112233")), 1)

    def test_reports_unresolved_colors(self):
        result = audit.Audit()
        result.pair("gradient", "#FFFFFF", "linear-gradient(blue, black)", [0, 0, 0, 1])
        self.assertEqual(result.skipped, {"gradient"})
        self.assertEqual(result.measurements, 0)

    def test_composites_translucent_text(self):
        result = audit.Audit()
        result.pair("text", "#FFFFFF20", "#000000", [0, 0, 0, 1])
        self.assertEqual(len(result.errors), 1)

    def test_checks_normal_action_before_hover(self):
        variant = fixture()["dark"]
        variant["theme"]["colors"] = {"background": "#000000", "muted.background": "#111111"}
        variant["composer"] = {"send": {"foreground": "#FFFFFF", "background": "#CCCCCC",
                                         "hover": "#000000", "active": "#000000"}}
        result = audit.Audit()
        result.variant("dark", variant)
        self.assertTrue(any("send.background.glyph" in error for error in result.errors))
        self.assertFalse(any("send.hover.glyph" in error for error in result.errors))

    def test_checks_card_hover_text(self):
        variant = fixture()["light"]
        variant["cards"] = {"explore": {"foreground": "#000000", "background": "#FFFFFF",
                                          "hover": "#111111", "active": "#FFFFFF"}}
        result = audit.Audit()
        result.variant("light", variant)
        self.assertTrue(any("card.explore.hover.foreground" in error for error in result.errors))

    def test_uses_primary_for_checked_track(self):
        variant = fixture()["dark"]
        variant["theme"]["colors"].update({"background": "#000000", "foreground": "#FFFFFF", "group_box.background": "#000000",
                                           "switch.background": "#808080", "switch.thumb.background": "#FFFFFF",
                                           "primary.background": "#808080", "switch.checked.background": "#000000",
                                           "button.primary.background": "#000000"})
        result = audit.Audit()
        result.variant("dark", variant)
        self.assertFalse(result.errors)
        variant["theme"]["colors"]["primary.background"] = "#000000"
        variant["theme"]["colors"]["switch.checked.background"] = "#808080"
        variant["theme"]["colors"]["button.primary.background"] = "#808080"
        result = audit.Audit()
        result.variant("dark", variant)
        self.assertTrue(any("checked switch track" in error for error in result.errors))


class Command(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "theme.json").write_text(json.dumps(fixture()))

    def test_accepts_partial_palettes(self):
        result = audit.inspect(self.root)
        self.assertEqual(result.measurements, 2)
        self.assertFalse(result.errors)
        self.assertTrue(result.skipped)

    def test_propagates_import_failure(self):
        result = subprocess.CompletedProcess([], 1, stdout="", stderr="invalid theme slot")
        with patch.object(audit.subprocess, "run", return_value=result) as run:
            with self.assertRaisesRegex(ValueError, "invalid theme slot"):
                audit.inspect(self.root, Path("desktop"))
            run.assert_called_once_with(["desktop", "--check-theme", str(self.root)],
                                        capture_output=True, text=True, check=False)

    def test_returns_failure_for_missing_manifest(self):
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(audit.main([str(self.root / "missing")]), 1)

    def test_returns_failure_for_low_contrast(self):
        value = fixture()
        value["dark"]["theme"]["colors"]["foreground"] = "#EEEEEE"
        (self.root / "theme.json").write_text(json.dumps(value))
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(audit.main([str(self.root)]), 1)
        self.assertIn("dark: foreground", output.getvalue())

    def test_rejects_symbolic_manifest(self):
        target = self.root / "source.json"
        (self.root / "theme.json").rename(target)
        (self.root / "theme.json").symlink_to(target)
        with self.assertRaisesRegex(ValueError, "symbolic"):
            audit.inspect(self.root)

    def test_returns_failure_for_malformed_variant(self):
        (self.root / "theme.json").write_text(json.dumps({"light": [], "dark": []}))
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(audit.main([str(self.root)]), 1)


if __name__ == "__main__":
    unittest.main()
