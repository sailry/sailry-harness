"""Check the pinned tick patch without downloading or rebuilding assets."""
import json
from pathlib import Path
import subprocess
import unittest
import zipfile

import vendor


class ChartTicks(unittest.TestCase):
    def run_ticks(self, calls, bounded=True):
        with zipfile.ZipFile(Path(__file__).with_name("viewers.zip")) as archive:
            source = archive.read("@jvmr/pptx-to-html/dist/index.js").decode()
        start = source.index("function niceNum(")
        end = source.index("function formatNumber(", start)
        ticks = vendor.BOUNDED_TICKS if bounded else vendor.CHART_TICKS
        script = ticks.decode() + "\n" + source[start:end] + "\n" + calls
        result = subprocess.run(
            ["node", "--input-type=module"], input=script, text=True,
            capture_output=True, check=True, timeout=5,
        )
        return json.loads(result.stdout)

    def test_exact_source_guard(self):
        self.assertEqual(
            vendor.patched("@jvmr/pptx-to-html", "dist/index.js", vendor.CHART_TICKS),
            vendor.BOUNDED_TICKS,
        )
        for data in [b"changed source", vendor.CHART_TICKS * 2]:
            with self.assertRaises(ValueError):
                vendor.patched("@jvmr/pptx-to-html", "dist/index.js", data)
        self.assertEqual(vendor.patched("jszip", "dist/jszip.min.js", b"unchanged"), b"unchanged")
        with zipfile.ZipFile(Path(__file__).with_name("viewers.zip")) as archive:
            source = archive.read("@jvmr/pptx-to-html/dist/index.js")
        self.assertEqual(source.count(vendor.BOUNDED_TICKS), 1)

    def test_preserves_ordinary_scales(self):
        calls = "console.log(JSON.stringify([[0, 1], [-10, 10], [-25, -12], [12, 25], [5, 5], [0, 1000000], [0, 1.7e308]].map(([min, max]) => computeTicks(min, max, 4))));"
        self.assertEqual(self.run_ticks(calls), self.run_ticks(calls, bounded=False))

    def test_bounds_tiny_scales(self):
        counts = self.run_ticks(
            "console.log(JSON.stringify([1e-12, 1e-20, 1e-100].map(max => computeTicks(0, max, 4).length)));"
        )
        self.assertTrue(all(1 <= count <= 8 for count in counts), counts)

    def test_rejects_invalid_progression(self):
        results = self.run_ticks(
            "const cases = [[0, Infinity, 4], [NaN, 1, 4], [1, 0, 4], [0, 1, 0], [0, 1e-323, 4], [1e20, 1e20 + 16384, 4]]; console.log(JSON.stringify(cases.map(args => { try { computeTicks(...args); return false; } catch { return true; } })));"
        )
        self.assertTrue(all(results), results)


if __name__ == "__main__":
    unittest.main()
