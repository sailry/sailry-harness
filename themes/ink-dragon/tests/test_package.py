"""Portable artifact checks; no Sailry source or build is required."""

import json
from pathlib import Path
import unittest

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]


def rgba(value):
    value = value.removeprefix("#")
    channels = [
        int(value[index:index + 2], 16) / 255 for index in range(0, len(value), 2)
    ]
    return channels if len(channels) == 4 else channels + [1.0]


def composite(overlay, background):
    return [
        overlay[index] * overlay[3] + background[index] * (1 - overlay[3])
        for index in range(3)
    ] + [1.0]


def luminance(color):
    linear = [
        value / 12.92 if value <= 0.04045 else ((value + 0.055) / 1.055) ** 2.4
        for value in color[:3]
    ]
    return sum(value * weight for value, weight in zip(linear, [0.2126, 0.7152, 0.0722]))


def contrast(first, second):
    values = sorted([luminance(first), luminance(second)])
    return (values[1] + 0.05) / (values[0] + 0.05)


class Package(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest = json.loads((ROOT / "theme.json").read_text())
        cls.variants = [cls.manifest[mode] for mode in ["light", "dark"]]
        cls.paths = {
            asset["path"]
            for variant in cls.variants
            for asset in variant["assets"].values()
        }

    def test_png_resources(self):
        self.assertEqual(self.paths, {
            path.relative_to(ROOT).as_posix() for path in (ROOT / "assets").iterdir()
        })
        for relative in sorted(self.paths):
            with self.subTest(resource=relative):
                path = ROOT / relative
                self.assertTrue(path.resolve().is_relative_to((ROOT / "assets").resolve()))
                with Image.open(path) as image:
                    self.assertEqual(image.format, "PNG")
                    image.verify()
                with Image.open(path) as image:
                    image.load()
                    self.assertIn("transparency", image.info)
                    alpha = image.convert("RGBA").getchannel("A")
                    self.assertEqual(alpha.getextrema()[0], 0)
        sizes = []
        for variant in self.variants:
            wallpaper = variant["assets"]["background.window"]
            self.assertEqual(wallpaper["fit"], "cover")
            with Image.open(ROOT / wallpaper["path"]) as image:
                self.assertGreaterEqual(image.width, 1500)
                self.assertGreaterEqual(image.height, 900)
                sizes.append(image.size)
        self.assertEqual(sizes[0], sizes[1])

    def test_resource_budget(self):
        encoded = (ROOT / "theme.json").stat().st_size
        self.assertLess(encoded, 256 * 1024)
        decoded = 0
        for relative in self.paths:
            path = ROOT / relative
            self.assertLess(path.stat().st_size, 8 * 1024 * 1024)
            encoded += path.stat().st_size
            with Image.open(path) as image:
                decoded += image.width * image.height * 4
        self.assertLess(encoded, 32 * 1024 * 1024)
        self.assertLess(decoded, 32 * 1024 * 1024)

    def test_text_contrast(self):
        for variant in self.variants:
            colors = variant["theme"]["colors"]
            base = rgba(colors["background"])
            pairs = [(colors["foreground"], base), (colors["muted.foreground"], base)]
            for card in variant["cards"].values():
                for state in ["background", "hover", "active"]:
                    surface = composite(rgba(card[state]), base)
                    pairs.extend([(card["foreground"], surface), (card["detail"], surface)])
            user = variant["messages"]["user"]
            pairs.append((user["foreground"], composite(rgba(user["background"]), base)))
            pairs.append((colors["tab.active.foreground"], composite(
                rgba(colors["tab.active.background"]), base
            )))
            for foreground, surface in pairs:
                with self.subTest(mode=variant["theme"]["mode"], foreground=foreground):
                    self.assertGreaterEqual(contrast(rgba(foreground), surface), 4.5)

    def test_wallpaper_text_contrast(self):
        # Check both ink extremes through the default panel tints. User-selected
        # surface opacity can change contrast independently of this package.
        for variant in self.variants:
            colors = variant["theme"]["colors"]
            base = rgba(colors["background"])
            opacity = variant["assets"]["background.window"]["opacity"]
            for level in [0.0, 1.0]:
                wallpaper = composite([level, level, level, opacity], base)
                for background, tint, foreground in [
                    (colors["background"], 0.7, colors["muted.foreground"]),
                    (colors["sidebar.background"], 0.7, colors["sidebar.foreground"]),
                    (colors["title_bar.background"], 0.45, colors["foreground"]),
                ]:
                    surface = composite(rgba(background)[:3] + [tint], wallpaper)
                    with self.subTest(mode=variant["theme"]["mode"], level=level):
                        self.assertGreaterEqual(contrast(rgba(foreground), surface), 4.5)

    def test_translucent_states(self):
        for variant in self.variants:
            for name, card in variant["cards"].items():
                for state in ["background", "hover", "active"]:
                    with self.subTest(card=name, state=state):
                        self.assertGreater(rgba(card[state])[3], 0)
                        self.assertLess(rgba(card[state])[3], 1)
                texture = variant["assets"]["background.card." + name]
                self.assertEqual(texture["fit"], "tile")
                self.assertLessEqual(texture["opacity"], 0.2)


if __name__ == "__main__":
    unittest.main()
