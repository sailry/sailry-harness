#!/usr/bin/env python3
"""Stage the pinned shared ONNX Runtime beside Desktop and Host executables."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import tarfile
import tempfile
from urllib.request import urlopen

LOCK = Path(__file__).with_name("onnx-runtime.lock.json")
MANIFEST = "onnx-runtime.json"


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def specification(target, speech, link=False):
    lock = json.loads(LOCK.read_text())
    if target not in lock["targets"]:
        raise ValueError(f"Unsupported native runtime target: {target}")
    asset = lock["targets"][target]
    files = [file for file in asset["files"]
             if file["role"] == "runtime"
             or speech and file["role"] == "speech"
             or speech and link and file["role"] == "link"]
    return lock, asset, files


def ordinary(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"Native runtime input is not an ordinary file: {path}")


def verify(path, file):
    ordinary(path)
    if path.stat().st_size != file["size"] or digest(path) != file["sha256"]:
        raise ValueError(f"Native runtime file checksum mismatch: {path.name}")


def atomic(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_symlink():
        raise ValueError(f"Native runtime destination is a symlink: {path}")
    pending = None
    try:
        with tempfile.NamedTemporaryFile(prefix=".onnx-", dir=path.parent, delete=False) as temporary:
            pending = Path(temporary.name)
            temporary.write(content)
            temporary.flush()
            os.chmod(pending, 0o644)
        os.replace(pending, path)
    finally:
        if pending is not None:
            pending.unlink(missing_ok=True)


def archive_bytes(lock, asset, cache):
    archive = cache / asset["archive"]
    if archive.exists() or archive.is_symlink():
        ordinary(archive)
        data = archive.read_bytes()
    else:
        with urlopen(lock["source"] + asset["archive"], timeout=120) as response:
            data = response.read(asset["size"] + 1)
        if len(data) != asset["size"] or hashlib.sha256(data).hexdigest() != asset["sha256"]:
            raise ValueError("Native runtime archive checksum mismatch")
        atomic(archive, data)
    if len(data) != asset["size"] or hashlib.sha256(data).hexdigest() != asset["sha256"]:
        raise ValueError("Native runtime archive checksum mismatch")
    return data


def extract(data, files):
    result = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:bz2") as archive:
        members = archive.getmembers()
        for file in files:
            name = file["name"]
            if name != PurePosixPath(name).name or "\\" in name:
                raise ValueError("Native runtime output name is invalid")
            selected = [member for member in members if member.name == file["member"]]
            if len(selected) != 1 or not selected[0].isfile() or selected[0].size != file["size"]:
                raise ValueError(f"Native runtime archive entry is invalid: {name}")
            with archive.extractfile(selected[0]) as stream:
                content = stream.read(file["size"] + 1)
            if len(content) != file["size"] or hashlib.sha256(content).hexdigest() != file["sha256"]:
                raise ValueError(f"Native runtime file checksum mismatch: {name}")
            result[name] = content
    return result


def manifest(target, output, speech):
    lock, asset, files = specification(target, speech)
    installed = []
    for file in files:
        path = output / file["name"]
        ordinary(path)
        installed.append({"name": file["name"], "size": path.stat().st_size,
                          "source_sha256": file["sha256"], "sha256": digest(path)})
    return {"version": 1, "target": target, "runtime_version": lock["runtime_version"],
            "sherpa_version": lock["sherpa_version"] if speech else None,
            "source": {"url": lock["source"] + asset["archive"], "sha256": asset["sha256"]},
            "files": installed}


def finalize(target, output, speech):
    # Signing changes Mach-O bytes. The app seal or release archive protects this final manifest.
    content = manifest(target, output, speech)
    atomic(output / MANIFEST, (json.dumps(content, indent=2) + "\n").encode())


def prepare(target, output, cache, speech=False, source=None, cargo=False, link=False):
    lock, asset, files = specification(target, speech, link)
    content = {}
    if source is not None:
        for file in files:
            path = source / file["name"]
            if path.exists() or path.is_symlink():
                verify(path, file)
                content[file["name"]] = path.read_bytes()
    missing = [file for file in files if file["name"] not in content]
    if missing:
        content.update(extract(archive_bytes(lock, asset, cache), missing))
    directories = [output]
    if cargo:
        directories.extend([output / "deps", output / "examples"])
    for directory in directories:
        for file in files:
            atomic(directory / file["name"], content[file["name"]])
        finalize(target, directory, speech)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--speech", action="store_true")
    parser.add_argument("--link", action="store_true")
    parser.add_argument("--cargo", action="store_true")
    parser.add_argument("--finalize", action="store_true")
    args = parser.parse_args()
    output = args.output.absolute()
    if args.finalize:
        finalize(args.target, output, args.speech)
    else:
        cache = args.cache.absolute() if args.cache else output.parent / "sherpa-onnx-prebuilt"
        source = args.source.absolute() if args.source else None
        prepare(args.target, output, cache, args.speech, source, args.cargo, args.link)


if __name__ == "__main__":
    main()
