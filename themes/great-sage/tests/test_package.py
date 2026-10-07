"""Portable checks for the Great Sage artwork package."""

import json
from pathlib import Path
import re
import unittest

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]


def luminance(value):
    channels = [int(value[index:index + 2], 16) / 255 for index in [1, 3, 5]]
    linear = [
        channel / 12.92 if channel <= 0.04045 else ((channel + 0.055) / 1.055) ** 2.4
        for channel in channels
    ]
    return sum(channel * weight for channel, weight in zip(linear, [0.2126, 0.7152, 0.0722]))


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

    def test_button_readability(self):
        for variant in self.variants:
            colors = variant["theme"]["colors"]
            for state in ["background", "hover.background", "active.background"]:
                fill = colors["button.primary." + state]
                stops = re.findall(r"#[0-9A-Fa-f]{6}", fill)
                self.assertTrue(fill.startswith("#"), "primary buttons use solid fills")
                self.assertEqual(len(stops), 1)
                for stop in stops:
                    with self.subTest(mode=variant["theme"]["mode"], state=state):
                        self.assertGreaterEqual(contrast(colors["button.primary.foreground"], stop), 4.5)

    def test_composer_action_contrast(self):
        for variant in self.variants:
            for key in ["send", "stop"]:
                action = variant["composer"][key]
                for state in ["background", "hover", "active"]:
                    with self.subTest(mode=variant["theme"]["mode"], action=key, state=state):
                        self.assertGreaterEqual(contrast(action["foreground"], action[state]), 4.5)
                        self.assertGreaterEqual(contrast(action[state], variant["theme"]["colors"]["muted.background"]), 3)

    def test_switch_readability(self):
        for variant in self.variants:
            colors = variant["theme"]["colors"]
            tracks = [colors["switch.background"]] + re.findall(
                r"#[0-9A-Fa-f]{6}", colors["button.primary.background"]
            )
            for track in tracks:
                with self.subTest(mode=variant["theme"]["mode"], track=track):
                    self.assertGreaterEqual(contrast(colors["switch.thumb.background"], track), 3)

    def test_gilding_transparency(self):
        for variant in self.variants:
            gilding = variant["assets"]["button.new_session"]
            with Image.open(ROOT / gilding["path"]) as image:
                alpha = image.convert("RGBA").getchannel("A")
                # The edge texture must leave the caption free of gold fill.
                opening = alpha.crop((image.width // 4, image.height * 3 // 8,
                                      image.width * 3 // 4, image.height * 9 // 16))
                self.assertLessEqual(opening.getextrema()[1], 2)
                visible = alpha.point(lambda value: 255 if value >= 32 else 0)
                self.assertEqual(visible.getbbox(), (0, 0, image.width, image.height))

    def test_wallpaper_text_contrast(self):
        for variant in self.variants:
            colors = variant["theme"]["colors"]
            opacity = variant["assets"]["background.window"]["opacity"]
            base = colors["background"]
            texture = variant["assets"]["background.content"]
            with Image.open(ROOT / texture["path"]) as image:
                grain = image.convert("RGBA").getcolors(image.width * image.height)
            # Shell paints its theme background above the window wallpaper.
            # Its lowest permitted tint is 45%; cover both artwork extremes there.
            residual = 1 - 0.45 * (int(base[7:9], 16) / 255 if len(base) == 9 else 1)
            for level in [0, 255]:
                channels = [round(int(base[index:index + 2], 16)
                                  * (1 - opacity * residual) + level * opacity * residual)
                            for index in [1, 3, 5]]
                surface = "#" + "".join(f"{channel:02X}" for channel in channels)
                with self.subTest(mode=variant["theme"]["mode"], level=level):
                    self.assertGreaterEqual(contrast(colors["foreground"], surface), 4.5)
                    for _, pixel in grain:
                        alpha = texture["opacity"] * pixel[3] / 255
                        tinted = "#" + "".join(
                            f"{round(channel * (1 - alpha) + pixel[index] * alpha):02X}"
                            for index, channel in enumerate(channels)
                        )
                        self.assertGreaterEqual(contrast(colors["foreground"], tinted), 4.5)

    def test_modal_tab_contrast(self):
        for variant in self.variants:
            colors = variant["theme"]["colors"]
            surface = colors["popover.background"]
            track = colors["tab_bar.segmented.background"]
            opacity = int(track[-2:], 16) / 255
            channels = [round(int(track[index:index + 2], 16) * opacity
                              + int(surface[index:index + 2], 16) * (1 - opacity))
                        for index in [1, 3, 5]]
            surface = "#" + "".join(f"{channel:02X}" for channel in channels)
            for state in ["tab", "tab.active"]:
                fill = colors[state + ".background"]
                opacity = int(fill[-2:], 16) / 255
                channels = [round(int(fill[index:index + 2], 16) * opacity
                                  + int(surface[index:index + 2], 16) * (1 - opacity))
                            for index in [1, 3, 5]]
                background = "#" + "".join(f"{channel:02X}" for channel in channels)
                with self.subTest(mode=variant["theme"]["mode"], state=state):
                    self.assertGreaterEqual(contrast(colors[state + ".foreground"], background), 4.5)


if __name__ == "__main__":
    unittest.main()
