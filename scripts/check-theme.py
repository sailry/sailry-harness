#!/usr/bin/env python3
"""Audit explicit theme colors and optional artwork; delegate schema to Desktop."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys


def rgba(value):
    """Only solid hex colors can be measured without the Kit color resolver."""
    if not isinstance(value, str) or not re.fullmatch(r"#[\da-fA-F]{6}(?:[\da-fA-F]{2})?", value):
        return None
    channels = [int(value[index:index + 2], 16) / 255 for index in range(1, len(value), 2)]
    return channels if len(channels) == 4 else channels + [1.0]


def composite(overlay, background):
    return [overlay[index] * overlay[3] + background[index] * (1 - overlay[3])
            for index in range(3)] + [1.0]


def luminance(color):
    linear = [value / 12.92 if value <= 0.04045 else ((value + 0.055) / 1.055) ** 2.4
              for value in color[:3]]
    return sum(value * weight for value, weight in zip(linear, (0.2126, 0.7152, 0.0722)))


def contrast(first, second):
    low, high = sorted((luminance(first), luminance(second)))
    return (high + 0.05) / (low + 0.05)


class Audit:
    def __init__(self):
        self.errors = []
        self.skipped = set()
        self.measurements = 0

    def pair(self, label, foreground, background, base, minimum=4.5, opacity=1):
        foreground, background = rgba(foreground), rgba(background)
        if foreground is None or background is None:
            self.skipped.add(label)
            return
        background[3] *= opacity
        background = composite(background, base)
        ratio = contrast(composite(foreground, background), background)
        self.measurements += 1
        if ratio < minimum:
            self.errors.append(f"{label}: {ratio:.2f}:1 contrast, requires {minimum:g}:1")

    def variant(self, mode, variant):
        colors = variant["theme"].get("colors", {})
        background = rgba(colors.get("background"))
        if background is None:
            self.skipped.add(f"{mode}: base background")
            return
        base = composite(background, [1, 1, 1, 1] if mode == "light" else [0, 0, 0, 1])
        for key in ("foreground", "muted.foreground"):
            self.pair(f"{mode}: {key}", colors.get(key), colors.get("background"), base)
        modal = rgba(colors.get("popover.background"))
        modal = composite(modal, base) if modal else base
        for prefix in ("tab", "tab.active", "button", "button.primary", "button.secondary"):
            states = ("background",) if prefix.startswith("tab") else (
                "background", "hover.background", "active.background")
            for state in states:
                # Kit's normal buttons mix their fill with transparent at 20%.
                opacity = 0.8 if prefix.startswith("button") and state == "background" else 1
                self.pair(f"{mode}: {prefix}.{state}", colors.get(prefix + ".foreground"),
                          colors.get(prefix + "." + state), modal, opacity=opacity)
        for key, card in variant.get("cards", {}).items():
            for state in ("background", "hover", "active"):
                for text in ("foreground", "detail"):
                    self.pair(f"{mode}: card.{key}.{state}.{text}", card.get(text), card.get(state), base)
        for key, message in variant.get("messages", {}).items():
            self.pair(f"{mode}: message.{key}", message.get("foreground"),
                      message.get("background", colors.get("background")), base)
        composer = rgba(colors.get("muted.background"))
        composer = composite(composer, base) if composer else base
        for key in ("send", "stop"):
            action = variant.get("composer", {}).get(key)
            if action:
                for state in ("background", "hover", "active"):
                    label = f"{mode}: {key}.{state}"
                    self.pair(label + ".glyph", action.get("foreground"), action.get(state), composer)
                    self.pair(label + ".surface", action.get(state), colors.get("muted.background"), base, 3)
        tracks = {
            "unchecked": colors.get("switch.background"),
            "checked": colors.get("primary.background"),
        }
        for state, track in tracks.items():
            self.pair(f"{mode}: {state} switch thumb", colors.get("switch.thumb.background"), track, base, 3)
            self.pair(f"{mode}: {state} switch track", track, colors.get("group_box.background"), base, 3)


def artwork(root, manifest, audit):
    """Decode referenced images and measure text over the default panel tint."""
    from PIL import Image

    paths = {asset["path"] for mode in ("light", "dark")
             for asset in manifest[mode].get("assets", {}).values()}
    if len(paths) > 32:
        raise ValueError("more than 32 unique images")
    encoded = (root / "theme.json").stat().st_size
    decoded = 0
    for relative in sorted(paths):
        parts = Path(relative).parts
        if not parts or parts[0] != "assets" or any(part in (".", "..") for part in parts):
            raise ValueError(f"resource outside assets: {relative}")
        path = root
        for part in parts:
            path = path / part
            if path.is_symlink():
                raise ValueError(f"symbolic resource: {relative}")
        size = path.stat().st_size
        if size > 8 * 1024 * 1024:
            raise ValueError(f"resource exceeds 8 MiB: {relative}")
        encoded += size
        with Image.open(path) as image:
            if image.format not in ("PNG", "JPEG", "WEBP", "GIF") or max(image.size) > 4096:
                raise ValueError(f"unsupported image: {relative}")
            decoded += image.width * image.height * 4
            if decoded > 32 * 1024 * 1024:
                raise ValueError("decoded images exceed 32 MiB")
            image.verify()
        with Image.open(path) as image:
            image.load()
    if encoded > 32 * 1024 * 1024:
        raise ValueError("encoded package exceeds 32 MiB")
    for mode in ("light", "dark"):
        variant = manifest[mode]
        colors = variant["theme"].get("colors", {})
        base, foreground = rgba(colors.get("background")), rgba(colors.get("foreground"))
        wallpaper = variant.get("assets", {}).get("background.window")
        if not wallpaper or base is None or foreground is None:
            continue
        backdrop = composite(base, [1, 1, 1, 1] if mode == "light" else [0, 0, 0, 1])
        with Image.open(root / wallpaper["path"]) as image:
            image = image.convert("RGBA")
            image.thumbnail((160, 100))
            ratios = []
            for pixel in image.get_flattened_data():
                art = [channel / 255 for channel in pixel]
                art[3] *= wallpaper.get("opacity", 1)
                surface = composite(base[:3] + [base[3] * 0.835], composite(art, backdrop))
                ratios.append(contrast(composite(foreground, surface), surface))
            minimum = min(ratios)
            audit.measurements += 1
            if minimum < 4.5:
                audit.errors.append(f"{mode}: default panel over wallpaper: {minimum:.2f}:1, requires 4.5:1")
    return f"{len(paths)} images, {encoded / 1024:.0f} KiB encoded, {decoded / 1024**2:.1f} MiB decoded"


def inspect(root, desktop=None, images=False):
    if desktop:
        result = subprocess.run([str(desktop), "--check-theme", str(root)],
                                capture_output=True, text=True, check=False)
        if result.returncode:
            raise ValueError(result.stderr.strip() or "Desktop theme validation failed")
        print(result.stdout.strip())
    manifest_path = root / "theme.json"
    if root.is_symlink() or manifest_path.is_symlink():
        raise ValueError("theme directory and manifest cannot be symbolic links")
    if manifest_path.stat().st_size > 256 * 1024:
        raise ValueError("manifest exceeds 256 KiB")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    audit = Audit()
    for mode in ("light", "dark"):
        audit.variant(mode, manifest[mode])
    if images:
        print(artwork(root, manifest, audit))
    return audit


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("themes", nargs="+", type=Path, help="theme directories")
    parser.add_argument("--desktop", type=Path, help="existing Desktop binary for authoritative import validation")
    parser.add_argument("--artwork", action="store_true", help="decode images; requires scripts/theme-requirements.txt")
    args = parser.parse_args(argv)
    failed = False
    if args.desktop is None:
        print("Palette/artwork audit only; import schema is not checked (use --desktop)")
    for root in args.themes:
        print(f"Checking {root}")
        try:
            audit = inspect(root, args.desktop.resolve() if args.desktop else None, args.artwork)
            for error in audit.errors:
                print(f"ERROR: {error}")
            print(f"{audit.measurements} contrast checks, {len(audit.skipped)} pairs omitted or not solid hex")
            failed |= bool(audit.errors)
        except (OSError, ValueError, KeyError, TypeError, AttributeError, ImportError) as error:
            print(f"ERROR: {error}")
            failed = True
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
