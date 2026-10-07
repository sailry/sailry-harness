"""Portable checks for the Great Sage artwork package."""

import json
from pathlib import Path
import unittest

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]


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

    def test_wallpaper(self):
        for variant in self.variants:
            wallpaper = variant["assets"]["background.window"]
            self.assertEqual(wallpaper["fit"], "cover")
            with Image.open(ROOT / wallpaper["path"]) as image:
                self.assertGreaterEqual(image.width, 1400)
                self.assertGreaterEqual(image.height, 1000)
        self.assertLess(
            self.variants[0]["assets"]["background.window"]["opacity"],
            self.variants[1]["assets"]["background.window"]["opacity"],
        )

    def test_translucent_states(self):
        for variant in self.variants:
            self.assertEqual(set(variant["cards"]), {"explore", "build", "review", "plan"})
            for name, card in variant["cards"].items():
                self.assertNotEqual(card["background"], card["hover"])
                self.assertNotEqual(card["hover"], card["active"])
                for state in ["background", "hover", "active"]:
                    with self.subTest(card=name, state=state):
                        self.assertEqual(len(card[state]), 9)
                        opacity = int(card[state][-2:], 16)
                        self.assertGreater(opacity, 0)
                        self.assertLess(opacity, 255)


if __name__ == "__main__":
    unittest.main()
