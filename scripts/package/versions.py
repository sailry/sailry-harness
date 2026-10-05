"""Keep product versions aligned while mapping native distribution metadata."""

import argparse
from pathlib import Path
import re
import tomllib


NUMBER = r"(?:0|[1-9][0-9]*)"
PREVIEW = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
VERSION = re.compile(rf"(?P<core>{NUMBER}\.{NUMBER}\.{NUMBER})(?:-(?P<preview>{PREVIEW}(?:\.{PREVIEW})*))?")
ROOT = Path(__file__).resolve().parents[2]


def parse(value):
    match = VERSION.fullmatch(value)
    if match is None:
        raise ValueError("Expected a product version such as 0.1.0-alpha.1")
    return match


def apple(value):
    return parse(value).group("core")


def release_flags(value):
    return ["--prerelease", "--latest=false"] if parse(value).group("preview") else ["--latest"]


def product(root):
    versions = []
    for app in ("desktop", "host"):
        with (root / f"apps/{app}/Cargo.toml").open("rb") as file:
            versions.append(tomllib.load(file)["package"]["version"])
    mobile = re.search(r"^version:\s*(\S+)\s*$", (root / "apps/mobile/pubspec.yaml").read_text(), re.MULTILINE)
    if mobile is None:
        raise ValueError("Mobile must declare its application version")
    name, separator, code = mobile.group(1).rpartition("+")
    if not separator or not re.fullmatch(r"[1-9][0-9]*", code):
        raise ValueError("Mobile must declare a positive Android build number")
    versions.append(name)
    if len(set(versions)) != 1:
        raise ValueError("Desktop, Host and Mobile application versions must match")
    parse(name)
    return name, int(code)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--tag")
    mode.add_argument("--apple", nargs="?", const="")
    mode.add_argument("--android-code", action="store_true")
    args = parser.parse_args()
    version, code = product(ROOT)
    if args.tag is not None and args.tag != f"v{version}":
        raise ValueError("The release tag must match every application version")
    print(code if args.android_code else apple(args.apple or version) if args.apple is not None else version)


if __name__ == "__main__":
    main()
