"""Check application icon resources without starting the desktop or a Node."""

from pathlib import Path
import hashlib
import plistlib
import shutil
import struct
import subprocess
import unittest
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]
BRANDING = ROOT / "assets/branding"


class AppIcons(unittest.TestCase):
    def test_preserves_approved_vector(self):
        source = BRANDING / "sailry-mark.svg"
        self.assertEqual(
            hashlib.sha256(source.read_bytes()).hexdigest(),
            "24037717d989467f29747ec8ff48edd499b3312054d261d9cd68df919182c456",
        )
        svg = ET.parse(source).getroot()
        paths = svg.findall(".//{http://www.w3.org/2000/svg}path")
        self.assertEqual([path.attrib["fill"] for path in paths], ["#79C9F1", "#2874DA"])

    def test_png_has_high_resolution_alpha(self):
        data = (BRANDING / "sailry.png").read_bytes()
        self.assertEqual(data[:8], b"\x89PNG\r\n\x1a\n")
        self.assertEqual(data[12:16], b"IHDR")
        self.assertEqual(struct.unpack_from(">IIBB", data, 16), (1024, 1024, 8, 6))

    def test_windows_sizes(self):
        data = (BRANDING / "sailry.ico").read_bytes()
        reserved, kind, count = struct.unpack_from("<HHH", data)
        self.assertEqual((reserved, kind, count), (0, 1, 9))
        sizes = []
        end = 6 + 16 * count
        for index in range(count):
            width, height, _, _, planes, depth, length, offset = struct.unpack_from(
                "<BBBBHHII", data, 6 + 16 * index
            )
            self.assertEqual(width, height)
            self.assertEqual((planes, depth), (1, 32))
            self.assertGreater(length, 0)
            self.assertEqual(offset, end)
            end = offset + length
            self.assertLessEqual(end, len(data))
            sizes.append(width or 256)
        self.assertEqual(end, len(data))
        self.assertEqual(sorted(sizes), [16, 20, 24, 32, 40, 48, 64, 128, 256])

    def test_macos_representations_and_bundle_reference(self):
        with (ROOT / "apps/desktop/macos/Info.plist").open("rb") as source:
            name = plistlib.load(source)["CFBundleIconFile"]
        data = (BRANDING / name).read_bytes()
        self.assertEqual(data[:4], b"icns")
        self.assertEqual(struct.unpack_from(">I", data, 4)[0], len(data))
        offset = 8
        types = set()
        while offset < len(data):
            kind, length = struct.unpack_from(">4sI", data, offset)
            self.assertGreater(length, 8)
            if kind in {b"ic11", b"ic12", b"ic13", b"ic14", b"ic10"}:
                png = data[offset + 8 : offset + length]
                self.assertEqual(png[:8], b"\x89PNG\r\n\x1a\n")
                # The white tile has transparent outer padding. A fully
                # opaque square causes macOS to synthesize a gray backdrop.
                self.assertEqual(struct.unpack_from("BB", png, 24), (8, 6))
            offset += length
            self.assertLessEqual(offset, len(data))
            types.add(kind)
        self.assertEqual(offset, len(data))
        # Retina entries include 16/32/128/256/512 logical pixels at 2x.
        self.assertTrue({b"ic11", b"ic12", b"ic13", b"ic14", b"ic10"} <= types)

    @unittest.skipUnless(shutil.which("magick"), "requires ImageMagick")
    def test_macos_white_tile_with_clear_padding(self):
        data = (BRANDING / "Sailry.icns").read_bytes()
        offset = 8
        while offset < len(data):
            kind, length = struct.unpack_from(">4sI", data, offset)
            if kind == b"ic10":
                png = data[offset + 8 : offset + length]
                break
            offset += length
        else:
            self.fail("Missing 1024-pixel macOS representation")
        result = subprocess.run(
            [
                "magick",
                "png:-",
                "-format",
                "%[fx:p{0,0}.a] %[fx:p{128,512}.r] %[fx:p{128,512}.g] "
                "%[fx:p{128,512}.b] %[fx:p{128,512}.a]",
                "info:",
            ],
            input=png,
            capture_output=True,
            check=True,
        )
        self.assertEqual(
            [float(value) for value in result.stdout.split()], [0, 1, 1, 1, 1]
        )


if __name__ == "__main__":
    unittest.main()
