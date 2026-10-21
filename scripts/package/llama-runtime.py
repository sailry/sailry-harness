#!/usr/bin/env python3
"""Stage the pinned official llama.cpp runtime beside Desktop and Host."""
import argparse
import hashlib
import importlib.util
import io
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile
import zipfile

LOCK = Path(__file__).with_name("llama-runtime.lock.json")
MANIFEST = "llama-runtime.json"

# Both native runtimes share verified downloads and atomic ordinary-file publication.
SPEC = importlib.util.spec_from_file_location("onnx_runtime", Path(__file__).with_name("onnx-runtime.py"))
NATIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NATIVE)


def specification(target):
    lock = json.loads(LOCK.read_text())
    if target not in lock["targets"]:
        raise ValueError(f"Unsupported llama.cpp target: {target}")
    asset = lock["targets"][target]
    return lock, asset, asset["files"]


def extract(data, asset, files):
    result = {}
    archive = zipfile.ZipFile(io.BytesIO(data)) if asset["archive"].endswith(".zip") else tarfile.open(
        fileobj=io.BytesIO(data), mode="r:gz")
    with archive:
        members = archive.infolist() if isinstance(archive, zipfile.ZipFile) else archive.getmembers()
        for file in files:
            name = file["name"]
            if name != PurePosixPath(name).name or "\\" in name:
                raise ValueError("Native runtime output name is invalid")
            selected = [member for member in members if (
                member.filename if isinstance(archive, zipfile.ZipFile) else member.name) == file["member"]]
            if len(selected) != 1:
                raise ValueError(f"Native runtime archive entry is ambiguous: {name}")
            member = selected[0]
            if isinstance(archive, zipfile.ZipFile):
                mode = member.external_attr >> 16
                if member.is_dir() or mode & 0o170000 not in (0, 0o100000) or member.file_size != file["size"]:
                    raise ValueError(f"Native runtime archive entry is invalid: {name}")
                stream = archive.open(member)
            else:
                if not member.isfile() or member.size != file["size"]:
                    raise ValueError(f"Native runtime archive entry is invalid: {name}")
                stream = archive.extractfile(member)
            with stream:
                content = stream.read(file["size"] + 1)
            if len(content) != file["size"] or hashlib.sha256(content).hexdigest() != file["sha256"]:
                raise ValueError(f"Native runtime file checksum mismatch: {name}")
            result[name] = content
    return result


def finalize(target, output):
    lock, _, files = specification(target)
    installed = []
    for file in files:
        path = output / file["name"]
        NATIVE.ordinary(path)
        installed.append({"name": file["name"], "size": path.stat().st_size,
                          "sha256": NATIVE.digest(path)})
    content = {"version": 1, "target": target, "build": lock["build"],
               "files": installed}
    directory = output
    if target.endswith("-apple-darwin") and output.name == "MacOS" and output.parent.name == "Contents":
        directory = output.parent / "Resources"
    NATIVE.atomic(directory / MANIFEST, (json.dumps(content, indent=2) + "\n").encode())
    if directory != output:
        (output / MANIFEST).unlink(missing_ok=True)


def prepare(target, output, cache, source=None, cargo=False):
    lock, asset, files = specification(target)
    content = {}
    if source is not None:
        for file in files:
            path = source / file["name"]
            if path.exists() or path.is_symlink():
                NATIVE.verify(path, file)
                content[file["name"]] = path.read_bytes()
    missing = [file for file in files if file["name"] not in content]
    if missing:
        content.update(extract(NATIVE.archive_bytes(lock, asset, cache), asset, missing))
    directories = [output]
    if cargo:
        directories.extend([output / "deps", output / "examples"])
    for directory in directories:
        directory.mkdir(parents=True, exist_ok=True)
        for file in files:
            NATIVE.atomic(directory / file["name"], content[file["name"]])
        finalize(target, directory)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--cargo", action="store_true")
    parser.add_argument("--finalize", action="store_true")
    parser.add_argument("--sign")
    args = parser.parse_args()
    output = args.output.absolute()
    if not args.finalize and args.sign is None:
        cache = args.cache.absolute() if args.cache else output.parent / "llama-prebuilt"
        prepare(args.target, output, cache, args.source.absolute() if args.source else None, args.cargo)
    if args.sign is not None:
        if not args.target.endswith("-apple-darwin"):
            parser.error("Signing is supported only for macOS")
        for file in specification(args.target)[2]:
            path = output / file["name"]
            NATIVE.ordinary(path)
            subprocess.run(["codesign", "--force", "--timestamp=none", "--sign", args.sign, str(path)], check=True)
    if args.finalize or args.sign is not None:
        finalize(args.target, output)


if __name__ == "__main__":
    main()
