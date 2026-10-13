import hashlib
import json
import re
import unittest
from pathlib import Path
from xml.etree import ElementTree

import generate

DIRECTORY = Path(__file__).parent
SVG = "{http://www.w3.org/2000/svg}"


class Catalog(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.catalog = generate.generate(DIRECTORY)
        cls.source = (DIRECTORY / "file-type.tsx").read_text()

    def test_pinned_sources(self):
        for name, digest in generate.SOURCES.items():
            self.assertEqual(hashlib.sha256((DIRECTORY / name).read_bytes()).hexdigest(), digest)

    def test_complete_table(self):
        types = self.catalog["types"]
        self.assertEqual(len(types), 113)
        self.assertEqual(sum(name != entry["category"] for name, entry in types.items()), 99)
        self.assertEqual(sum(name == entry["category"] for name, entry in types.items()), 14)
        self.assertEqual(len({entry["category"] for entry in types.values()}), 14)

    def test_self_contained_artwork(self):
        forbidden = ["href=", "<image", "<script", "<foreignObject", "url(", "currentColor", "var("]
        for name, entry in self.catalog["types"].items():
            for edition in ["compact", "regular"]:
                document = ElementTree.fromstring(entry[edition])
                self.assertEqual(document.attrib["viewBox"], "0 0 24 24")
                for value in forbidden:
                    self.assertNotIn(value, entry[edition], (name, edition))

    def test_source_geometry(self):
        paths = {
            name: re.search(rf'const {name} =\s*"([^"]+)"', self.source).group(1)
            for name in ["PAGE_PATH", "FLAP_PATH", "FOLD_PATH"]
        }
        for name, entry in self.catalog["types"].items():
            for edition in ["compact", "regular"]:
                for value in paths.values():
                    self.assertIn(f'd="{value}"', entry[edition], (name, edition))

    def test_labels_and_families(self):
        for name, entry in self.catalog["types"].items():
            for edition in ["compact", "regular"]:
                document = ElementTree.fromstring(entry[edition])
                label = document.find(f".//{SVG}text")
                if name == entry["category"]:
                    self.assertIsNone(label)
                else:
                    self.assertEqual(label.text, name.upper())
                    self.assertEqual(label.attrib["x"], "12")
                    self.assertEqual(label.attrib["y"], "15")
                if name in ["empty", "file"]:
                    self.assertEqual(len(document.findall(f".//{SVG}path")), 3)

    def test_responsive_sizes(self):
        for name in ["pdf", "docx", "xlsx", "png", "zip", "woff2"]:
            for edition, size in [("regular", "28"), ("compact", "20")]:
                document = ElementTree.fromstring(self.catalog["types"][name][edition])
                self.assertEqual(document.attrib["width"], size)
                self.assertEqual(document.attrib["height"], size)
                label = document.find(f".//{SVG}text")
                self.assertEqual(label.attrib["font-weight"], "600" if edition == "compact" else "500")
        self.assertGreater(
            float(ElementTree.fromstring(self.catalog["types"]["pdf"]["compact"]).find(f".//{SVG}text").attrib["font-size"]),
            float(ElementTree.fromstring(self.catalog["types"]["pdf"]["regular"]).find(f".//{SVG}text").attrib["font-size"]),
        )

    def test_format_colors(self):
        for name, fill, foreground in [
            ("pdf", "#f54900", "#ffffff"),
            ("docx", "#155dfc", "#ffffff"),
            ("xlsx", "#00a63e", "#ffffff"),
            ("png", "#f0b100", "#432004"),
            ("zip", "#bcbdc2", "#52525a"),
        ]:
            document = ElementTree.fromstring(self.catalog["types"][name]["regular"])
            self.assertEqual(document.find(f".//{SVG}path").attrib["fill"], fill)
            self.assertEqual(document.find(f".//{SVG}text").attrib["fill"], foreground)

    def test_mime_table(self):
        for value, name in self.catalog["mime"].items():
            self.assertIn(name, self.catalog["types"], value)
        self.assertEqual(self.catalog["mime"]["application/pdf"], "pdf")
        self.assertEqual(self.catalog["mime"]["image/jpeg"], "jpg")
        self.assertEqual(self.catalog["mime"]["application/vnd.ms-excel.sheet.macroenabled.12"], "xlsm")

    def test_reproducible_catalog(self):
        self.assertEqual(
            (DIRECTORY / "catalog.json").read_text(),
            json.dumps(self.catalog, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        )


if __name__ == "__main__":
    unittest.main()
