#!/usr/bin/env python3
"""Convert pinned ReUI Solid artwork into offline, color-preserving SVG assets."""
import argparse
import hashlib
import json
import math
import re
from pathlib import Path
from xml.etree import ElementTree

SOURCES = {
    "file-type.tsx": "1a0310d5fe017b6940ff48e0b95367a93b61ed123035632cc8e5c379847b83c9",
    "theme.css": "ce91efc0ca2e741e56fe263662e5282dc0a16f19a8647f0c38b60fcc4e2119b1",
}


def between(source, start, end):
    return source.split(start, 1)[1].split(end, 1)[0]


def lab(lightness, chroma, hue):
    angle = math.radians(hue)
    return lightness, chroma * math.cos(angle), chroma * math.sin(angle)


def rgb(color):
    lightness, a, b = color
    l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s = (lightness - 0.0894841775 * a - 1.2914855480 * b) ** 3
    linear = (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )
    def encode(channel):
        value = max(0, min(1, channel))
        value = value * 12.92 if value <= 0.0031308 else 1.055 * value ** (1 / 2.4) - 0.055
        return round(value * 255)
    return "#" + "".join(f"{encode(channel):02x}" for channel in linear)


def mix(color, target, fraction):
    return tuple(left * fraction + right * (1 - fraction) for left, right in zip(color, target))


def colors(source):
    return {
        name: lab(float(lightness) / 100, float(chroma), float(hue))
        for name, lightness, chroma, hue in re.findall(
            r"--color-([\w-]+): oklch\(([\d.]+)% ([\d.]+) ([\d.]+)\)", source
        )
    }


def palette(value, theme):
    if value == "NEUTRAL":
        color = theme["zinc-500"]
        return rgb(mix(color, (1, 0, 0), 0.45)), rgb(mix(color, (0, 0, 0), 0.8))
    if value == "YELLOW":
        return rgb(theme["yellow-500"]), rgb(theme["yellow-950"])
    return rgb(theme[value.removeprefix('"text-').removesuffix('"')]), "#ffffff"


def glyphs(source):
    block = between(source, "const GLYPHS:", "\n/* -------------------------------------------------------------------------- */")
    result = {}
    entries = re.findall(r"  (\w+): ([\s\S]*?)(?=\n  \w+:|\n})", block)
    for category, content in entries:
        nodes = []
        for tag, attributes in re.findall(r"<(path|rect|circle)\b([\s\S]*?)/>", content):
            attributes = dict(re.findall(r'(\w+)="([^"]*)"', attributes))
            node = ElementTree.Element(tag, attributes)
            nodes.append(ElementTree.tostring(node, encoding="unicode"))
        assert nodes, category
        result[category] = "".join(nodes)
    return result


def artwork(name, category, value, compact, paths, shapes, theme):
    fill, foreground = palette(value, theme)
    box = 20 if compact else 28
    stroke = 24 / box
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{box}" height="{box}" viewBox="0 0 24 24">',
        f'<g stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round">',
        f'<path d="{paths["PAGE_PATH"]}" fill="{fill}" stroke="{fill}"/>',
        f'<path d="{paths["FLAP_PATH"]}" fill="#ffffff" fill-opacity=".3" stroke="none"/>',
        f'<path d="{paths["FOLD_PATH"]}" fill="none" stroke="#ffffff" stroke-opacity=".45"/>',
    ]
    if name != category:
        label = name.upper()
        if compact:
            # Upstream's <=21px rule, evaluated at the existing 20px media slot.
            font_size = min(0.325 * 24, 5.8 * 24 / 20, 0.66 * 24 / (len(label) * 0.9))
        else:
            font_size = min(0.205 * 24, 0.656 * 24 / (len(label) * 0.8))
        parts.append(
            f'<text x="12" y="15" text-anchor="middle" dominant-baseline="central" '
            f'font-family="sans-serif" font-weight="{600 if compact else 500}" '
            f'font-size="{font_size:.6f}" fill="{foreground}" stroke="none">{label}</text>'
        )
    elif category in shapes:
        transform = "" if compact else ' transform="translate(3 3.75) scale(.75)"'
        # The JSX paths use non-scaling-stroke. Compensate for the glyph-only
        # transform explicitly because resvg does not implement vector-effect.
        glyph_stroke = stroke if compact else stroke / 0.75
        parts.append(
            f'<g fill="none" stroke="{foreground}" stroke-width="{glyph_stroke}"'
            f'{transform}>{shapes[category]}</g>'
        )
    parts.extend(["</g>", "</svg>"])
    return "".join(parts)


def generate(directory):
    for name, digest in SOURCES.items():
        assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == digest, name
    source = (directory / "file-type.tsx").read_text()
    theme = colors((directory / "theme.css").read_text())
    block = between(source, "const FILE_TYPE_EXTENSIONS = {", "} satisfies")
    known = {}
    for quoted, bare, category, value in re.findall(
        r'\s+(?:"(\w+)"|(\w+)): \{ category: "(\w+)", color: ([^}]+) \}', block
    ):
        known[quoted or bare] = (category, value.strip())
    assert len(known) == 99
    families = dict(re.findall(
        r"  (\w+): ([^,]+),", between(source, "const CATEGORY_COLORS:", "\n}")
    ))
    assert len(families) == 14
    paths = {
        name: re.search(rf'const {name} =\s*"([^"]+)"', source).group(1)
        for name in ["PAGE_PATH", "FLAP_PATH", "FOLD_PATH"]
    }
    shapes = glyphs(source)
    types = {
        name: {
            "category": category,
            "compact": artwork(name, category, value, True, paths, shapes, theme),
            "regular": artwork(name, category, value, False, paths, shapes, theme),
        }
        for name, (category, value) in {**known, **{name: (name, value) for name, value in families.items()}}.items()
    }
    mime = dict(re.findall(
        r'"([^"]+)":\s*"([^"]+)"',
        between(source, "const MIME_TYPES:", "\n}"),
    ))
    assert len(types) == 113
    for name, entry in types.items():
        for edition in ["regular", "compact"]:
            ElementTree.fromstring(entry[edition])
    return {"types": types, "mime": mime}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    catalog = json.dumps(generate(arguments.directory), ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    path = arguments.directory / "catalog.json"
    if arguments.check:
        assert path.read_text() == catalog, "file type catalog differs from pinned sources"
    else:
        path.write_text(catalog)
    print("Verified 99 extensions, 14 categories, and 226 standalone Solid SVGs")


if __name__ == "__main__":
    main()
