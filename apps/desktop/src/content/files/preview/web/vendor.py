#!/usr/bin/env python3
"""Rebuild the offline viewer archive from integrity-pinned upstream packages.

No npm install, lifecycle scripts, build-time network or runtime CDN is needed.
Upstream distributions and their license notices are retained unmodified.
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
    "pdfjs-dist": {
        "legacy/build/pdf.mjs", "legacy/build/pdf.worker.mjs",
        "legacy/web/pdf_viewer.mjs", "legacy/web/pdf_viewer.css",
    },
}


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
        url = f"https://registry.npmjs.org/{name}/-/{name}-{version}.tgz"
        data = urllib.request.urlopen(url, timeout=60).read()
        digest = "sha512-" + base64.b64encode(hashlib.sha512(data).digest()).decode()
        if digest != entry["integrity"]:
            raise ValueError(f"Integrity mismatch: {name}")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
            for member in archive.getmembers():
                path = member.name.removeprefix("package/")
                if member.isfile() and selected(name, path):
                    contents[f"{name}/{path}"] = archive.extractfile(member).read()
    with zipfile.ZipFile(ROOT / "viewers.zip", "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path, data in sorted(contents.items()):
            info = zipfile.ZipInfo(path)
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
    print(f"Bundled {len(contents)} viewer resources")


if __name__ == "__main__":
    main()
