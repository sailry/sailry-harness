#!/usr/bin/env python3
"""Rebuild the offline viewer archive from integrity-pinned upstream packages.

No npm install, lifecycle scripts, build-time network or runtime CDN is needed.
License notices are retained unmodified. One checked chart-tick fix is applied
to the pinned PPTX renderer; all other upstream distributions stay unchanged.
"""
import base64
import hashlib
import io
import json
from pathlib import Path
import tarfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent
PACKAGES = json.loads((ROOT / "dependencies.json").read_text())
FILES = {
    "docx-preview": {"dist/docx-preview.min.js"},
    "jszip": {"dist/jszip.min.js"},
    "@jvmr/pptx-to-html": {"dist/index.js"},
    "xlsx": {"xlsx.mjs"},
    "pdfjs-dist": {
        "legacy/build/pdf.mjs", "legacy/build/pdf.worker.mjs",
        "legacy/web/pdf_viewer.mjs", "legacy/web/pdf_viewer.css",
    },
}

# @jvmr/pptx-to-html 1.1.2 uses a fixed 1e-9 tick epsilon. Tiny chart domains
# can therefore generate billions of ticks, while non-advancing float steps
# never terminate. Replace only this exact function with a step-relative
# epsilon, a finite tick budget, and a checked positive progression.
CHART_TICKS = b"""function computeTicks(min, max, count) {
\tconst span = max - min;
\tif (span <= 0) return [min, max];
\tconst step = niceNum(span / count, true);
\tconst ticks = [];
\tlet v = Math.ceil(min / step) * step;
\twhile (v <= max + 1e-9) {
\t\tticks.push(Number(v.toFixed(10)));
\t\tv += step;
\t}
\treturn ticks;
}"""
BOUNDED_TICKS = b"""function computeTicks(min, max, count) {
\tif (!Number.isFinite(min) || !Number.isFinite(max) || !Number.isFinite(count) || count <= 0) throw new Error("Chart scale is invalid");
\tconst span = max - min;
\tif (!Number.isFinite(span) || span < 0) throw new Error("Chart scale is invalid");
\tif (span === 0) return [min, max];
\tconst step = niceNum(span / count, true);
\tif (!Number.isFinite(step) || step <= 0) throw new Error("Chart scale is invalid");
\tconst limit = Math.ceil(span / step) + 2;
\tif (!Number.isFinite(limit) || limit > 50000) throw new Error("Chart scale is invalid");
\tconst epsilon = step * 1e-9;
\tconst ticks = [];
\tlet v = Math.ceil(min / step) * step;
\tif (!Number.isFinite(v)) throw new Error("Chart scale is invalid");
\tfor (let index = 0; index < limit && v <= max + epsilon; index++) {
\t\tticks.push(Number(v.toFixed(10)));
\t\tconst next = v + step;
\t\tif (next > max + epsilon) break;
\t\tif (!Number.isFinite(next) || next <= v) {
\t\t\tif (v >= max - epsilon) break;
\t\t\tthrow new Error("Chart scale is invalid");
\t\t}
\t\tv = next;
\t}
\treturn ticks;
}"""


def patched(name, path, data):
    if name != "@jvmr/pptx-to-html" or path != "dist/index.js":
        return data
    if PACKAGES[name]["version"] != "1.1.2" or data.count(CHART_TICKS) != 1:
        raise ValueError("The PPTX chart-tick patch requires reviewed source")
    return data.replace(CHART_TICKS, BOUNDED_TICKS, 1)


def selected(name, path):
    if path in FILES[name] or "LICENSE" in path.upper() or "NOTICE" in path.upper():
        return True
    return name == "pdfjs-dist" and path.startswith(
        ("cmaps/", "standard_fonts/", "wasm/", "legacy/web/images/")
    )


def main():
    contents = {}
    for name, entry in PACKAGES.items():
        version = entry["version"]
        package = name.rsplit("/", 1)[-1]
        url = entry.get("url", f"https://registry.npmjs.org/{name}/-/{package}-{version}.tgz")
        data = urllib.request.urlopen(url, timeout=60).read()
        digest = "sha512-" + base64.b64encode(hashlib.sha512(data).digest()).decode()
        if digest != entry["integrity"]:
            raise ValueError(f"Integrity mismatch: {name}")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
            for member in archive.getmembers():
                path = member.name.removeprefix("package/")
                if member.isfile() and selected(name, path):
                    contents[f"{name}/{path}"] = patched(name, path, archive.extractfile(member).read())
    with zipfile.ZipFile(ROOT / "viewers.zip", "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path, data in sorted(contents.items()):
            info = zipfile.ZipInfo(path)
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
    print(f"Bundled {len(contents)} viewer resources")


if __name__ == "__main__":
    main()
