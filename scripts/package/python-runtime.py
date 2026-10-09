#!/usr/bin/env python3
"""Package the pinned standalone Python without consulting installed interpreters."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tarfile
import tempfile
from urllib.request import urlopen

LOCK = Path(__file__).with_name("python-runtime.lock.json")
MANIFEST = "python-runtime.json"
MAX_EXPANDED = 512 * 1024 * 1024


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def specification(target):
    lock = json.loads(LOCK.read_text())
    if target not in lock["targets"]:
        raise ValueError(f"Unsupported Python target: {target}")
    return lock, lock["targets"][target]


def archive_bytes(asset, cache, source=None):
    archive = source or cache / asset["archive"]
    if archive.exists() or archive.is_symlink():
        if archive.is_symlink() or not archive.is_file():
            raise ValueError("Python archive is not an ordinary file")
        content = archive.read_bytes()
    elif source is not None:
        raise ValueError("Python archive is unavailable")
    else:
        with urlopen(asset["url"], timeout=120) as response:
            content = response.read(asset["size"] + 1)
    if len(content) != asset["size"] or hashlib.sha256(content).hexdigest() != asset["sha256"]:
        raise ValueError("Python archive checksum mismatch")
    if source is None and not archive.exists():
        cache.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=cache, delete=False) as pending:
            pending.write(content)
            name = Path(pending.name)
        os.replace(name, archive)
    return content


def member_path(name):
    path = PurePosixPath(name)
    if path.is_absolute() or "\\" in name or path.parts[:1] != ("python",) or any(
            part in (".", "..") for part in path.parts):
        raise ValueError("Python archive path is invalid")
    return path


def extract(content, directory):
    with tarfile.open(fileobj=io.BytesIO(content), mode="r:gz") as archive:
        members = archive.getmembers()
        total = 0
        names = set()
        for member in members:
            path = member_path(member.name)
            if str(path) in names or not (member.isdir() or member.isfile() or member.issym()):
                raise ValueError("Python archive entry is invalid")
            names.add(str(path))
            total += member.size
            if total > MAX_EXPANDED:
                raise ValueError("Python archive exceeds the installation limit")
            if member.issym():
                link = PurePosixPath(member.linkname)
                if link.is_absolute() or "\\" in member.linkname:
                    raise ValueError("Python archive link is invalid")
                depth = len(path.parent.parts)
                for part in link.parts:
                    depth += -1 if part == ".." else 0 if part == "." else 1
                    if depth < 1:
                        raise ValueError("Python archive link escapes its directory")
        # Links are published after regular files, so extraction never writes through one.
        for member in members:
            path = directory.joinpath(*member_path(member.name).parts)
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                path.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, path.open("xb") as output:
                    shutil.copyfileobj(source, output)
                os.chmod(path, 0o555 if member.mode & 0o111 else 0o444)
        for member in members:
            if member.issym():
                path = directory.joinpath(*member_path(member.name).parts)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.symlink_to(member.linkname)
                if not path.resolve().is_relative_to((directory / "python").resolve()) or not path.resolve().exists():
                    raise ValueError("Python archive link does not resolve inside its directory")


def finalize(target, output):
    lock, asset = specification(target)
    directory = output / "python"
    for path in list(directory.rglob("__pycache__")):
        if path.is_dir() and not path.is_symlink():
            os.chmod(path.parent, 0o755)
            remove(path)
    files = []
    for path in sorted(directory.rglob("*")):
        relative = path.relative_to(directory).as_posix()
        if "__pycache__" in path.relative_to(directory).parts:
            continue
        if path.is_symlink():
            if not path.resolve().is_relative_to(directory.resolve()):
                raise ValueError("Python package link escapes its directory")
            files.append({"path": relative, "link": os.readlink(path)})
        elif path.is_file() and relative != MANIFEST:
            files.append({"path": relative, "size": path.stat().st_size, "sha256": digest(path)})
    manifest = {"version": 1, "python_version": lock["python_version"], "target": target,
                "source": {"url": asset["url"], "sha256": asset["sha256"]}, "files": files}
    receipt = directory / MANIFEST
    if receipt.is_symlink():
        raise ValueError("Python package manifest is a symlink")
    if receipt.exists():
        os.chmod(receipt, 0o644)
    receipt.write_text(json.dumps(manifest, indent=2) + "\n")
    os.chmod(receipt, 0o444)
    for path in directory.rglob("*"):
        if not path.is_symlink():
            os.chmod(path, 0o555 if path.is_dir() or path.stat().st_mode & 0o111 else 0o444)
    os.chmod(directory, 0o555)


def remove(directory):
    for path in directory.rglob("*"):
        if not path.is_symlink():
            os.chmod(path, 0o755 if path.is_dir() else 0o644)
    os.chmod(directory, 0o755)
    shutil.rmtree(directory)


def prepare(target, output, cache, source=None):
    lock, asset = specification(target)
    output.mkdir(parents=True, exist_ok=True)
    destination = output / "python"
    if destination.is_symlink():
        raise ValueError("Python package destination is a symlink")
    if destination.exists():
        receipt = destination / MANIFEST
        if not receipt.is_file() or receipt.is_symlink():
            raise ValueError("Existing Python directory is not a managed package")
        current = json.loads(receipt.read_text())
        for file in current.get("files", []):
            name = file["path"]
            relative = PurePosixPath(name)
            if relative.is_absolute() or "\\" in name or any(part in (".", "..") for part in relative.parts) or not relative.parts:
                raise ValueError("Python receipt path is invalid")
        if current.get("target") == target and current.get("source", {}).get("sha256") == asset["sha256"]:
            valid = all((destination / file["path"]).is_symlink()
                        and os.readlink(destination / file["path"]) == file["link"]
                        if "link" in file else (destination / file["path"]).is_file()
                        and (destination / file["path"]).stat().st_size == file["size"]
                        and digest(destination / file["path"]) == file["sha256"]
                        for file in current["files"])
            if valid:
                return
    with tempfile.TemporaryDirectory(prefix=".python-", dir=output) as temporary:
        staged = Path(temporary)
        extract(archive_bytes(asset, cache, source), staged)
        library = staged / "python" / ("Lib" if "windows" in target else "lib/python3.12")
        if not library.is_dir():
            raise ValueError("Python standard library is unavailable")
        (library / "EXTERNALLY-MANAGED").write_text(
            "[externally-managed]\nError=Use a virtual environment to install dependencies\n")
        os.chmod(library / "EXTERNALLY-MANAGED", 0o444)
        executable = staged / "python" / ("python.exe" if "windows" in target else "bin/python3.12")
        if not executable.is_file():
            raise ValueError("Python executable is unavailable")
        finalize(target, staged)
        if destination.exists():
            remove(destination)
        # macOS requires owner-write on the directory being renamed.
        os.chmod(staged / "python", 0o755)
        os.replace(staged / "python", destination)
        os.chmod(destination, 0o555)


def unlock(output):
    directory = output / "python"
    if directory.is_symlink() or not (directory / MANIFEST).is_file():
        raise ValueError("Python signing requires a managed package")
    os.chmod(directory, 0o755)
    for path in directory.rglob("*"):
        if not path.is_symlink():
            os.chmod(path, 0o755 if path.is_dir() or path.stat().st_mode & 0o111 else 0o644)


def signing_options(path, output):
    # Only the child interpreter loads third-party wheels outside the application.
    if path == output / "python/bin/python3.12":
        return ["--entitlements", str(Path(__file__).with_name("python.entitlements.plist"))]
    return []


def sign(target, output, identity):
    if not target.endswith("-apple-darwin"):
        raise ValueError("Python code signing requires a macOS target")
    unlock(output)
    magic = {b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe",
             b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca", b"\xca\xfe\xba\xbf", b"\xbf\xba\xfe\xca"}
    for path in sorted((output / "python").rglob("*"), key=lambda path: (-len(path.parts), str(path))):
        if path.is_file() and not path.is_symlink():
            with path.open("rb") as stream:
                native = stream.read(4) in magic
            if native:
                subprocess.run(["codesign", "--force", "--timestamp=none", "--options", "runtime",
                                "--sign", identity, *signing_options(path, output), str(path)], check=True)
    finalize(target, output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path, default=Path(".runtime/python-cache"))
    parser.add_argument("--source", type=Path)
    parser.add_argument("--finalize", action="store_true")
    parser.add_argument("--sign")
    args = parser.parse_args()
    if args.sign:
        sign(args.target, args.output, args.sign)
    elif args.finalize:
        finalize(args.target, args.output)
    else:
        prepare(args.target, args.output, args.cache, args.source)


if __name__ == "__main__":
    main()
