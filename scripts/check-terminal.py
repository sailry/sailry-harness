#!/usr/bin/env python3
"""Interactive terminal acceptance probe using only Python's standard library."""
import argparse
import base64
import os
import re
import select
import sys
import struct
from pathlib import Path
import termios
import tty


def write(text):
    sys.stdout.write(text)
    sys.stdout.flush()


def display():
    print("ANSI: expect distinct single, double, curly, dotted and dashed underlines")
    for code, label in [("4", "single"), ("4:2", "double"), ("4:3", "curly"),
                        ("4:4", "dotted"), ("4:5", "dashed"), ("53", "overline"), ("5", "blink")]:
        write(f"\x1b[{code}m{label}\x1b[0m  ")
    print("\nWide characters: 中文 🙂 e\u0301")
    print("Selection: double-click a word, triple-click a line, Option-drag a rectangle")
    print("Search for TERMINAL_PROBE with Cmd+F / Ctrl+F, then use next/previous")
    print("TERMINAL_PROBE first\nTERMINAL_PROBE second")
    write("\x1b]8;;https://ghostty.org\x1b\\Cmd/Ctrl-click this Ghostty link\x1b]8;;\x1b\\\n")
    print("Graphics: expect a red rectangle; scroll away and back to check retention")
    payload = base64.b64encode(bytes([255, 0, 0, 255]) * 64 * 32).decode()
    for offset in range(0, len(payload), 4096):
        chunk = payload[offset:offset + 4096]
        more = int(offset + 4096 < len(payload))
        header = "a=T,f=32,s=64,v=32,i=764,c=12,r=3,q=2," if offset == 0 else ""
        write(f"\x1b_G{header}m={more};{chunk}\x1b\\")
    write("\n")
    print("Clipboard: this probe leaves the system clipboard unchanged unless --clipboard is used")


def image(path):
    data = Path(path).expanduser().read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR" or len(data) < 33:
        raise ValueError("--image requires a PNG file")
    width, height = struct.unpack(">II", data[16:24])
    if not width or not height or width * height * 4 > 5 * 1024 * 1024:
        raise ValueError("PNG dimensions must fit within 5 MiB of decoded RGBA pixels")
    if len(data) > 8 * 1024 * 1024:
        raise ValueError("PNG file exceeds 8 MiB")
    print(f"PNG: {width} x {height}; check color, transparency and aspect ratio")
    columns = min(80, max(1, os.get_terminal_size().columns - 2))
    payload = base64.b64encode(data).decode()
    for offset in range(0, len(payload), 4096):
        chunk = payload[offset:offset + 4096]
        more = int(offset + 4096 < len(payload))
        header = f"a=T,f=100,i=765,c={columns},q=2," if offset == 0 else ""
        write(f"\x1b_G{header}m={more};{chunk}\x1b\\")
    write("\n")


def keyboard():
    descriptor = sys.stdin.fileno()
    previous = termios.tcgetattr(descriptor)
    print("Keyboard: press/hold/release letters, Shift+Enter, left/right modifiers and keypad keys")
    print("Switch window focus and paste text. Press Escape to finish")
    try:
        tty.setraw(descriptor)
        write("\x1b[>31u\x1b[?1004h\x1b[?2004h\x1b[?u")
        pending = b""
        while True:
            if not select.select([descriptor], [], [], 0.05)[0]:
                continue
            data = os.read(descriptor, 4096)
            if not data:
                break
            write(repr(data) + "\r\n")
            pending = (pending + data)[-4096:]
            if re.search(rb"\x1b\[27(?:;[0-9:]*)?u", pending) or data == b"\x03":
                break
    finally:
        write("\x1b[<u\x1b[?1004l\x1b[?2004l\r\n")
        termios.tcsetattr(descriptor, termios.TCSADRAIN, previous)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keyboard", action="store_true", help="capture negotiated Kitty key events")
    parser.add_argument("--clipboard", action="store_true", help="write TERMINAL_PROBE to the clipboard via OSC 52")
    parser.add_argument("--image", metavar="PNG", help="display a PNG using Kitty graphics")
    options = parser.parse_args()
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        parser.error("run this probe inside the terminal being tested")
    display()
    if options.image:
        try:
            image(options.image)
        except (OSError, ValueError) as error:
            parser.error(str(error))
    if options.clipboard:
        write("\x1b]52;c;" + base64.b64encode(b"TERMINAL_PROBE").decode() + "\x07")
        print("Clipboard now contains TERMINAL_PROBE if OSC 52 is available")
    if options.keyboard:
        keyboard()


if __name__ == "__main__":
    main()
