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
                                           "button.primary.background": "#FFFFFF"})
        result = audit.Audit()
        result.variant("dark", variant)
        self.assertFalse(result.errors)
        variant["theme"]["colors"]["primary.background"] = "#000000"
        variant["theme"]["colors"]["switch.checked.background"] = "#808080"
        variant["theme"]["colors"]["button.primary.background"] = "#808080"
        result = audit.Audit()
        result.variant("dark", variant)
        self.assertTrue(any("checked switch track" in error for error in result.errors))


class Controls(unittest.TestCase):
    def setUp(self):
        self.variant = fixture()["dark"]
        self.colors = self.variant["theme"]["colors"]
        self.colors.update({"background": "#000000", "foreground": "#FFFFFF",
                            "group_box.background": "#000000", "popover.background": "#000000"})

    def test_preserves_normal_button_opacity(self):
        self.colors.update({"background": "#FFFFFF", "foreground": "#000000",
                            "group_box.background": "#FFFFFF", "popover.background": "#FFFFFF"})
        for prefix in ("button", "button.primary", "button.secondary"):
            self.colors[prefix + ".background"] = "#707070"
            self.colors[prefix + ".foreground"] = "#FFFFFF"
        result = audit.Audit()
        result.variant("light", self.variant)
        self.assertFalse(result.errors)
        self.assertNotIn("light: button.primary.background.base.foreground", result.skipped)

    def test_rejects_indistinct_selection(self):
        self.colors.update({"tab_bar.segmented.background": "#111111",
                            "tab.active.foreground": "#FFFFFF", "tab.foreground": "#FFFFFF"})
        result = audit.Audit()
        result.variant("dark", self.variant)
        self.assertEqual({error.rsplit(": ", 1)[0] for error in result.errors},
                         {f"dark: segmented.{surface}.selection"
                          for surface in ("base", "group_box", "popover")})
        self.assertTrue(all("requires 3:1" in error for error in result.errors))

    def test_requires_readable_segmented_text(self):
        self.colors.update({"tab_bar.segmented.background": "#767676",
                            "tab.active.foreground": "#FFFFFF", "tab.foreground": "#FFFFFF"})
        result = audit.Audit()
        result.variant("dark", self.variant)
        self.assertFalse(result.errors)
        for key, value, state in (("tab.active.foreground", "#000000", "selected"),
                                  ("tab.foreground", "#767676", "unselected"),
                                  ("tab.active.foreground", "#767676", "hovered")):
            with self.subTest(state=state):
                variant = {"theme": {"colors": {**self.colors, key: value}}}
                result = audit.Audit()
                result.variant("dark", variant)
                self.assertEqual({error.rsplit(": ", 1)[0] for error in result.errors},
                                 {f"dark: segmented.{surface}.{state}.foreground"
                                  for surface in ("base", "group_box", "popover")})
                self.assertTrue(all("requires 4.5:1" in error for error in result.errors))

    def test_distinguishes_active_tab_fill(self):
        self.colors.update({"tab_bar.segmented.background": "#767676",
                            "tab.active.background": "#FFFFFF",
                            "tab.active.foreground": "#FFFFFF", "tab.foreground": "#FFFFFF"})
        result = audit.Audit()
        result.variant("dark", self.variant)
        self.assertEqual([error.rsplit(": ", 1)[0] for error in result.errors],
                         ["dark: tab.active.background"])
        self.assertNotIn("dark: segmented.base.selected.foreground", result.skipped)

    def test_composites_segmented_layers(self):
        self.colors.update({"background": "#FFFFFF80", "foreground": "#000000",
                            "group_box.background": "#00000080", "popover.background": "#FFFFFF80",
                            "tab_bar.segmented.background": "#00000080",
                            "tab.active.foreground": "#000000", "tab.foreground": "#FFFFFF"})
        result = audit.Audit()
        result.variant("dark", self.variant)
        self.assertEqual([error.rsplit(": ", 1)[0] for error in result.errors],
                         ["dark: segmented.base.hovered.foreground",
                          "dark: segmented.group_box.hovered.foreground",
                          "dark: segmented.popover.selection",
                          "dark: segmented.popover.hovered.foreground"])
        self.assertTrue(result.errors[2].endswith("requires 3:1"))
        self.assertTrue(all(result.errors[index].endswith("requires 4.5:1") for index in (0, 1, 3)))

    def test_rejects_indistinct_primary_states(self):
        for fill, foreground, part, minimum in (("#111111", "#FFFFFF", "surface", 3),
                                                ("#FFFFFF", "#FFFFFF", "foreground", 4.5)):
            with self.subTest(part=part):
                self.colors["button.primary.foreground"] = foreground
                for state in ("background", "hover.background", "active.background"):
                    self.colors["button.primary." + state] = fill
                result = audit.Audit()
                result.variant("dark", self.variant)
                self.assertEqual({error.rsplit(": ", 1)[0] for error in result.errors},
                                 {f"dark: button.primary.{state}.{surface}.{part}"
                                  for state in ("background", "hover.background", "active.background")
                                  for surface in ("base", "group_box", "popover")})
                self.assertTrue(all(f"requires {minimum:g}:1" in error for error in result.errors))

    def test_composites_primary_over_each_surface(self):
        self.colors.update({"background": "#FFFFFF80", "foreground": "#000000",
                            "group_box.background": "#00000080", "popover.background": "#FFFFFF80",
                            "button.primary.background": "#00000080",
                            "button.primary.foreground": "#FFFFFF"})
        result = audit.Audit()
        result.variant("dark", self.variant)
        self.assertEqual({error.rsplit(": ", 1)[0] for error in result.errors},
                         {f"dark: button.primary.background.{surface}.surface"
                          for surface in ("base", "group_box")})
        self.assertNotIn("dark: button.primary.background.popover.surface", result.skipped)

    def test_skips_unresolved_control_fills(self):
        self.colors.update({"tab.active.foreground": "#FFFFFF", "tab.foreground": "#000000",
                            "button.primary.foreground": "#000000"})
        for value in (None, "white", "linear-gradient(white, black)"):
            with self.subTest(value=value):
                self.colors["tab_bar.segmented.background"] = value
                for state in ("background", "hover.background", "active.background"):
                    self.colors["button.primary." + state] = value
                result = audit.Audit()
                result.variant("dark", self.variant)
                self.assertFalse(result.errors)
                self.assertEqual(result.measurements, 1)
                for surface in ("base", "group_box", "popover"):
                    for part in ("selection", "selected.foreground", "unselected.foreground", "hovered.foreground"):
                        self.assertIn(f"dark: segmented.{surface}.{part}", result.skipped)
                    for state in ("background", "hover.background", "active.background"):
                        for part in ("surface", "foreground"):
                            self.assertIn(f"dark: button.primary.{state}.{surface}.{part}", result.skipped)

    def test_skips_unresolved_control_surfaces(self):
        self.colors.update({"tab_bar.segmented.background": "#767676",
                            "tab.active.foreground": "#FFFFFF", "tab.foreground": "#FFFFFF",
                            "button.primary.foreground": "#000000"})
        for state in ("background", "hover.background", "active.background"):
            self.colors["button.primary." + state] = "#FFFFFF"
        for value in (None, "black", "linear-gradient(black, white)"):
            with self.subTest(value=value):
                self.colors["group_box.background"] = value
                self.colors["popover.background"] = value
                result = audit.Audit()
                result.variant("dark", self.variant)
                self.assertFalse(result.errors)
                self.assertEqual(result.measurements, 11)
                for surface in ("group_box", "popover"):
                    for part in ("selection", "selected.foreground", "unselected.foreground", "hovered.foreground"):
                        self.assertIn(f"dark: segmented.{surface}.{part}", result.skipped)
                    for state in ("background", "hover.background", "active.background"):
                        for part in ("surface", "foreground"):
                            self.assertIn(f"dark: button.primary.{state}.{surface}.{part}", result.skipped)


class Links(unittest.TestCase):
    def test_checks_each_state_and_surface(self):
        colors = {"background": "#FFFFFF", "foreground": "#000000",
                  "group_box.background": "#FFFFFF", "popover.background": "#FFFFFF",
                  "link": "#000000", "link.hover": "#000000", "link.active": "#000000"}
        for state in ("link", "link.hover", "link.active"):
            for color in ("#EEEEEE", "#00000020"):
                with self.subTest(state=state, color=color):
                    variant = {"theme": {"colors": {**colors, state: color}}}
                    result = audit.Audit()
                    result.variant("light", variant)
                    self.assertEqual({error.rsplit(": ", 1)[0] for error in result.errors},
                                     {f"light: {state}.{surface}"
                                      for surface in ("base", "group_box", "popover")})

    def test_checks_message_backgrounds(self):
        variant = {"theme": {"colors": {
            "background": "#000000", "foreground": "#FFFFFF", "link": "#FFFFFF",
            "link.hover": "#FFFFFF", "link.active": "#FFFFFF"}},
            "messages": {"user": {"background": "#FFFFFF80", "foreground": "#000000"},
                         "assistant": {"foreground": "#FFFFFF"}}}
        result = audit.Audit()
        result.variant("dark", variant)
        self.assertEqual({error.rsplit(": ", 1)[0] for error in result.errors},
                         {f"dark: message.user.{state}"
                          for state in ("link", "link.hover", "link.active")})
        for state in ("link", "link.hover", "link.active"):
            self.assertNotIn(f"dark: message.assistant.{state}", result.skipped)

    def test_skips_unresolved_colors(self):
        for color in (None, "blue-300", "linear-gradient(blue, black)"):
            with self.subTest(color=color):
                variant = fixture()["light"]
                variant["theme"]["colors"]["link"] = color
                variant["messages"] = {"assistant": {"foreground": "#000000"}}
                result = audit.Audit()
                result.variant("light", variant)
                self.assertFalse(result.errors)
                self.assertIn("light: link.base", result.skipped)
                self.assertIn("light: message.assistant.link", result.skipped)


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
