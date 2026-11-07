#!/usr/bin/env python3
"""Draw the Windows installer wizard images from the Sailry mark with the standard library."""

import argparse
from array import array
from pathlib import Path
import re
import struct
import zlib
from xml.etree import ElementTree

ROOT = Path(__file__).resolve().parents[2]
MARK = ROOT / "assets/branding/sailry-mark.svg"
NAMESPACE = "{http://www.w3.org/2000/svg}"
BACKGROUND = (0x0F, 0x12, 0x17)
GLOW = (0x28, 0x74, 0xDA)
# Inno Setup's image areas at 100% through 250% display scaling; Setup picks the closest file.
WIZARD = ((202, 386), (269, 515), (336, 643), (403, 772), (430, 824), (498, 953), (534, 1022))
SMALL = (58, 77, 97, 116, 124, 143, 159)
SUBROWS = 8
CURVE_STEPS = 32


def color(value):
    if not isinstance(value, str) or re.fullmatch(r"#[0-9A-Fa-f]{6}", value) is None:
        raise ValueError("Mark paths must use a #RRGGBB fill")
    return tuple(int(value[index:index + 2], 16) for index in (1, 3, 5))


def cubic(start, first, second, end):
    points = []
    for step in range(1, CURVE_STEPS + 1):
        t = step / CURVE_STEPS
        u = 1 - t
        points.append(tuple(u ** 3 * start[axis] + 3 * u * u * t * first[axis]
                            + 3 * u * t * t * second[axis] + t ** 3 * end[axis] for axis in (0, 1)))
    return points


def rings(path):
    """Closed polygons for absolute M, L, C, S and Z path data."""
    if re.search(r"[^MLCSZ0-9.,\s+-]", path):
        raise ValueError("The mark uses an unsupported path command")
    tokens = re.findall(r"[MLCSZ]|[-+]?(?:\d+\.?\d*|\.\d+)", path)
    result, ring, current, control, command, index = [], [], (0.0, 0.0), None, None, 0

    def numbers(count):
        nonlocal index
        values = tokens[index:index + count]
        if len(values) != count or any(value in "MLCSZ" for value in values):
            raise ValueError("Mark path data is incomplete")
        index += count
        return [float(value) for value in values]

    while index < len(tokens):
        if tokens[index] in "MLCSZ":
            command = tokens[index]
            index += 1
            if command == "Z":
                if len(ring) > 2:
                    result.append(ring)
                ring, control = [], None
                continue
        elif command in (None, "Z"):
            raise ValueError("Mark path data has no command")
        if command == "M":
            current = tuple(numbers(2))
            ring, control, command = [current], None, "L"
        elif command == "L":
            current = tuple(numbers(2))
            ring.append(current)
            control = None
        else:
            if command == "C":
                x1, y1, x2, y2, x, y = numbers(6)
            else:
                x2, y2, x, y = numbers(4)
                x1, y1 = (2 * current[0] - control[0], 2 * current[1] - control[1]) if control else current
            ring.extend(cubic(current, (x1, y1), (x2, y2), (x, y)))
            control, current = (x2, y2), (x, y)
    if len(ring) > 2:
        result.append(ring)
    return result


def shapes(source):
    """Filled shapes in paint order, as (color, rings) in SVG user units."""
    found = []

    def visit(element, dx, dy):
        transform = element.get("transform")
        if transform is not None:
            match = re.fullmatch(r"\s*translate\(\s*([-+\d.]+)[\s,]+([-+\d.]+)\s*\)\s*", transform)
            if match is None:
                raise ValueError("The mark uses an unsupported transform")
            dx, dy = dx + float(match[1]), dy + float(match[2])
        if element.tag == f"{NAMESPACE}path":
            polygons = [[(x + dx, y + dy) for x, y in ring] for ring in rings(element.get("d", ""))]
            found.append((color(element.get("fill")), polygons))
        for child in element:
            visit(child, dx, dy)

    visit(ElementTree.fromstring(source), 0.0, 0.0)
    if not found:
        raise ValueError("The mark has no filled paths")
    return found


def placed(mark, width, height, size, center):
    """Scale the mark so its larger side is `size` pixels, centered on `center`."""
    points = [point for _, polygons in mark for ring in polygons for point in ring]
    left, right = min(x for x, _ in points), max(x for x, _ in points)
    top, bottom = min(y for _, y in points), max(y for _, y in points)
    scale = size / max(right - left, bottom - top)
    middle = ((left + right) / 2, (top + bottom) / 2)
    return [(fill, [[((x - middle[0]) * scale + center[0], (y - middle[1]) * scale + center[1]) for x, y in ring]
                     for ring in polygons]) for fill, polygons in mark]


def coverage(polygons, width, height):
    """Even-odd coverage per pixel: exact horizontally, sampled on sub-rows vertically."""
    grid = array("d", bytes(8 * width * height))
    edges = [(x0, y0, x1, y1) for ring in polygons
             for (x0, y0), (x1, y1) in zip(ring, ring[1:] + ring[:1]) if y0 != y1]
    ys = [y for ring in polygons for _, y in ring]
    weight = 1 / SUBROWS
    for row in range(max(0, int(min(ys))), min(height, int(max(ys)) + 1)):
        offset = row * width
        for sub in range(SUBROWS):
            y = row + (sub + 0.5) * weight
            crossings = sorted(x0 + (y - y0) * (x1 - x0) / (y1 - y0)
                               for x0, y0, x1, y1 in edges if (y0 <= y) != (y1 <= y))
            for left, right in zip(crossings[::2], crossings[1::2]):
                left, right = max(0.0, left), min(float(width), right)
                if right <= left:
                    continue
                first, last = int(left), int(right)
                if first == last:
                    grid[offset + first] += (right - left) * weight
                    continue
                grid[offset + first] += (first + 1 - left) * weight
                for column in range(first + 1, last):
                    grid[offset + column] += weight
                if last < width:
                    grid[offset + last] += (right - last) * weight
    return grid


def encode(width, height, pixel, alpha):
    raw = bytearray()
    for y in range(height):
        raw.append(0)
        for x in range(width):
            raw.extend(pixel(x, y))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 6 if alpha else 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")


def wizard(mark, width, height):
    """The opaque side image: the mark over a faint brand glow on the dark brand background."""
    center = (width / 2, height * 0.42)
    layers = [(fill, coverage(polygons, width, height))
              for fill, polygons in placed(mark, width, height, width * 0.58, center)]
    radius = width * 0.62

    def pixel(x, y):
        distance = (((x + 0.5 - center[0]) ** 2 + (y + 0.5 - center[1]) ** 2) ** 0.5) / radius
        glow = 0.2 * max(0.0, 1 - distance) ** 2
        value = [base + (accent - base) * glow for base, accent in zip(BACKGROUND, GLOW)]
        for fill, grid in layers:
            amount = min(1.0, grid[y * width + x])
            value = [current + (target - current) * amount for current, target in zip(value, fill)]
        return bytes(round(channel) for channel in value)

    return encode(width, height, pixel, alpha=False)


def small(mark, size):
    """The transparent corner image, legible on both light and dark wizard styles."""
    layers = [(fill, coverage(polygons, size, size))
              for fill, polygons in placed(mark, size, size, size * 0.84, (size / 2, size / 2))]

    def pixel(x, y):
        value, opacity = [0.0, 0.0, 0.0], 0.0
        for fill, grid in layers:
            amount = min(1.0, grid[y * size + x])
            total = amount + opacity * (1 - amount)
            if total:
                value = [(target * amount + current * opacity * (1 - amount)) / total
                         for current, target in zip(value, fill)]
            opacity = total
        return bytes([*(round(channel) for channel in value), round(opacity * 255)])

    return encode(size, size, pixel, alpha=True)


def write(source, output):
    """Write every wizard image size into `output` and return the paths."""
    mark = shapes(Path(source).read_text(encoding="utf-8"))
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    paths = []
    for width, height in WIZARD:
        path = output / f"wizard-{width}x{height}.png"
        path.write_bytes(wizard(mark, width, height))
        paths.append(path)
    for size in SMALL:
        path = output / f"small-{size}.png"
        path.write_bytes(small(mark, size))
        paths.append(path)
    return paths


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mark", type=Path, default=MARK)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    for path in write(arguments.mark, arguments.output):
        print(path)


if __name__ == "__main__":
    main()
