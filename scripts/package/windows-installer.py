#!/usr/bin/env python3
"""Compile the per-user Windows installer from a staged portable Desktop bundle."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
from urllib.request import urlopen

sys.path.insert(0, str(Path(__file__).parent))
import versions

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "windows-installer.iss"
LOCK = HERE / "inno-setup.lock.json"
TARGETS = ("x86_64-pc-windows-msvc",)
_art_spec = importlib.util.spec_from_file_location("installer_art", HERE / "installer-art.py")
art = importlib.util.module_from_spec(_art_spec)
_art_spec.loader.exec_module(art)


def assets(target):
    """Published Windows Desktop files that the checksum file covers."""
    return f"Sailry-{target}.zip", f"Sailry-{target}-setup.exe"


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def release(bundle, target):
    """The staged bundle's product version and its numeric core for Windows version resources."""
    if target not in TARGETS:
        raise ValueError(f"Unsupported Windows installer target: {target}")
    build = json.loads((bundle / "build.json").read_text(encoding="utf-8"))
    if build.get("target") != target or not (bundle / "sailry-desktop.exe").is_file():
        raise ValueError("The staged bundle does not match the installer target")
    version = build.get("application_version")
    if not isinstance(version, str):
        raise ValueError("The staged bundle has no application version")
    return version, versions.apple(version)


def compiler(cache, lock=None):
    """The pinned Inno Setup compiler, installed privately for the current user."""
    lock = lock or json.loads(LOCK.read_text())
    directory = cache / f"inno-setup-{lock['version']}"
    executable = directory / "ISCC.exe"
    if executable.is_file():
        return executable
    installer = cache / lock["archive"]
    if not installer.is_file() or installer.stat().st_size != lock["size"] or digest(installer) != lock["sha256"]:
        with urlopen(lock["url"], timeout=120) as response:
            data = response.read(lock["size"] + 1)
        if len(data) != lock["size"] or hashlib.sha256(data).hexdigest() != lock["sha256"]:
            raise ValueError("Inno Setup installer checksum mismatch")
        cache.mkdir(parents=True, exist_ok=True)
        installer.write_bytes(data)
    subprocess.run([str(installer), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-",
                    "/CURRENTUSER", "/NOICONS", f"/DIR={directory}"], check=True, timeout=600)
    if not executable.is_file():
        raise ValueError("Inno Setup did not install its command-line compiler")
    return executable


def command(executable, bundle, output, target, version, core, icon, images, sign_tool=None):
    """ISCC arguments; a sign tool definition such as `name=command` also signs the uninstaller."""
    argv = [str(executable), "/Qp", f"/O{output}", f"/F{Path(assets(target)[1]).stem}",
            f"/DAppVersion={version}", f"/DVersionCore={core}", f"/DBundle={bundle}",
            f"/DIconFile={icon}", f"/DArtDir={images}"]
    if sign_tool:
        name, separator, tool = sign_tool.partition("=")
        if not separator or not name.isidentifier() or not tool:
            raise ValueError("A sign tool must be given as name=command")
        argv += [f"/S{sign_tool}", f"/DSignTool={name}"]
    return argv + [str(SCRIPT)]


def build(source, bundle, target, output, cache, sign_tool=None):
    if platform.system() != "Windows":
        raise ValueError("The Windows installer must be compiled on Windows")
    source, bundle, output = source.resolve(), bundle.resolve(), output.resolve()
    version, core = release(bundle, target)
    executable = compiler(cache.resolve())
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="sailry-installer-") as images:
        art.write(source / "assets/branding/sailry-mark.svg", images)
        subprocess.run(command(executable, bundle, output, target, version, core,
                               source / "assets/branding/sailry.ico", images, sign_tool), check=True)
    setup = output / assets(target)[1]
    if not setup.is_file():
        raise ValueError("The Windows installer was not produced")
    return setup


def checksums(directory, target):
    """Write the release checksum file after any signing, so it covers the published bytes."""
    lines = []
    for name in assets(target):
        path = directory / name
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"Windows release file is missing: {name}")
        lines.append(f"{digest(path)}  {name}\n")
    result = directory / f"SHA256SUMS-desktop-{target}"
    result.write_text("".join(lines), encoding="utf-8", newline="\n")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    compile_parser = commands.add_parser("build", help="Compile the installer from a staged bundle")
    compile_parser.add_argument("--source", type=Path, required=True, help="Product checkout with brand assets")
    compile_parser.add_argument("--bundle", type=Path, required=True)
    compile_parser.add_argument("--target", choices=TARGETS, required=True)
    compile_parser.add_argument("--output", type=Path, required=True)
    compile_parser.add_argument("--cache", type=Path, required=True)
    compile_parser.add_argument("--sign-tool")
    checksum_parser = commands.add_parser("checksums", help="Record checksums of the Windows release files")
    checksum_parser.add_argument("--directory", type=Path, required=True)
    checksum_parser.add_argument("--target", choices=TARGETS, required=True)
    arguments = parser.parse_args()
    if arguments.command == "build":
        print(build(arguments.source, arguments.bundle, arguments.target, arguments.output,
                    arguments.cache, arguments.sign_tool))
    else:
        print(checksums(arguments.directory, arguments.target))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from None
