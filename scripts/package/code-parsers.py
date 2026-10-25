#!/usr/bin/env python3
"""Prepare verified offline language-pack queries and application parser resources."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tempfile
from urllib.request import urlopen

LOCK = Path(__file__).with_name("code-parsers.lock.json")
MANIFEST = "code-parsers.json"
FILE_LIMIT = 128 * 1024 * 1024
TOTAL_LIMIT = 1024 * 1024 * 1024


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def ordinary(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"Parser resource must be an ordinary file: {path}")


def atomic(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_symlink():
        raise ValueError(f"Parser resource destination is a symlink: {path}")
    pending = path.with_name(path.name + ".pending")
    try:
        with pending.open("wb") as stream:
            stream.write(content)
        os.replace(pending, path)
    finally:
        pending.unlink(missing_ok=True)


def fetch(lock, asset, cache):
    path = cache / asset["archive"]
    if path.exists() or path.is_symlink():
        ordinary(path)
    else:
        limit = asset.get("size", 1024 * 1024)
        with urlopen(lock["source"] + asset["archive"], timeout=120) as response:
            content = response.read(limit + 1)
        if len(content) > limit or hashlib.sha256(content).hexdigest() != asset["sha256"]:
            raise ValueError("Official parser resource checksum mismatch")
        atomic(path, content)
    if ("size" in asset and path.stat().st_size != asset["size"]) or digest(path) != asset["sha256"]:
        raise ValueError(f"Cached parser resource checksum mismatch: {path.name}")
    return path


def source_bundle(lock, cache):
    path = fetch(lock, lock["queries"], cache)
    atomic(path.with_name(path.name + ".sha256"),
           (lock["queries"]["sha256"] + "  " + path.name + "\n").encode())
    return path


def bundle_url(path):
    # The pinned build.rs reads everything after file:// as a native path,
    # without URL decoding or Windows drive normalization.
    return "file://" + str(path)


def libraries(archive, suffix):
    """Read one verified, flat archive; decoding remains a build-time operation."""
    result = {}
    try:
        stream = tarfile.open(archive, mode="r:zst")
    except (tarfile.CompressionError, ValueError):
        # Python 3.12/3.13 use the platform archive tool (libarchive, or GNU tar + zstd).
        # The fixed upstream checksum is verified before this extraction.
        with tempfile.TemporaryDirectory(prefix="sailry-parsers-") as temporary:
            subprocess.run(["tar", "-xf", str(archive), "-C", temporary], check=True)
            root = Path(temporary)
            total = 0
            for path in root.iterdir():
                ordinary(path)
                size = path.stat().st_size
                if size > FILE_LIMIT or total + size > TOTAL_LIMIT:
                    raise ValueError("Parser archive exceeds the extraction limit")
                result[path.name] = path.read_bytes()
                total += size
        return validate(result, suffix)
    with stream:
        total = 0
        for member in stream:
            if member.isdir() and member.name in {".", "./"}:
                continue
            name = PurePosixPath(member.name).name
            if not member.isfile() or member.name.removeprefix("./") != name:
                raise ValueError("Parser archive contains an invalid entry")
            if member.size > FILE_LIMIT or total + member.size > TOTAL_LIMIT:
                raise ValueError("Parser archive exceeds the extraction limit")
            if name in result:
                raise ValueError("Parser archive contains duplicate entries")
            with stream.extractfile(member) as file:
                result[name] = file.read(member.size + 1)
            if len(result[name]) != member.size:
                raise ValueError("Parser archive contains an incomplete file")
            total += member.size
    return validate(result, suffix)


def validate(files, suffix):
    pattern = re.compile(r"(?:lib)?tree_sitter_[A-Za-z0-9_]+\." + re.escape(suffix))
    if len(files) != 371 or any(pattern.fullmatch(name) is None for name in files):
        raise ValueError("Parser archive does not contain the 371 pinned grammars")
    return files


def staged(target, output, lock, checksums=True):
    ordinary(output / MANIFEST)
    manifest = json.loads((output / MANIFEST).read_text())
    if (manifest.get("version") != 1 or manifest.get("target") != target
            or manifest.get("language_pack") != lock["language_pack"]
            or manifest.get("tree_sitter") != lock["tree_sitter"]
            or manifest.get("source_sha256") != lock["queries"]["sha256"]
            or manifest.get("archive_sha256") != lock["targets"][target]["sha256"]
            or len(manifest.get("languages", [])) != 371):
        raise ValueError("Prepared parsers do not match the pinned release")
    records = manifest.get("files", [])
    names = {record["name"] for record in records}
    if len(records) != 371 or len(names) != 371:
        raise ValueError("Prepared parser file inventory is incomplete")
    validate(dict.fromkeys(names), extension(target))
    total = 0
    for record in records:
        path = output / record["name"]
        ordinary(path)
        size = path.stat().st_size
        if size > FILE_LIMIT or total + size > TOTAL_LIMIT:
            raise ValueError("Prepared parsers exceed the resource limit")
        if checksums and (size != record["size"] or digest(path) != record["sha256"]):
            raise ValueError(f"Prepared parser checksum mismatch: {path.name}")
        total += size
    return manifest


def extension(target):
    return "dylib" if "apple" in target else "dll" if "windows" in target else "so"


def finalize(target, output):
    lock = json.loads(LOCK.read_text())
    manifest = staged(target, output, lock, checksums=False)
    for record in manifest["files"]:
        path = output / record["name"]
        record.update(size=path.stat().st_size, sha256=digest(path))
    atomic(output / MANIFEST, (json.dumps(manifest, indent=2) + "\n").encode())


def prepare(target, output, cache, source_only=False):
    lock = json.loads(LOCK.read_text())
    source = source_bundle(lock, cache)
    if source_only:
        return bundle_url(source)
    if target not in lock["targets"]:
        raise ValueError(f"Unsupported parser target: {target}")
    if (output / MANIFEST).exists():
        staged(target, output, lock)
        return bundle_url(source)
    metadata = json.loads(fetch(lock, lock["manifest"], cache).read_text())
    languages = metadata["groups"]["all"]
    if metadata["version"] != lock["language_pack"] or len(languages) != 371:
        raise ValueError("Official parser manifest does not match the pinned release")
    asset = lock["targets"][target]
    archive = fetch(lock, asset, cache)
    suffix = extension(target)
    files = libraries(archive, suffix)
    output.mkdir(parents=True, exist_ok=True)
    installed = []
    for name, content in sorted(files.items()):
        atomic(output / name, content)
        installed.append({"name": name, "size": len(content), "sha256": hashlib.sha256(content).hexdigest()})
    manifest = {"version": 1, "language_pack": lock["language_pack"],
                "tree_sitter": lock["tree_sitter"], "source_sha256": lock["queries"]["sha256"],
                "archive_sha256": asset["sha256"], "target": target,
                "languages": languages, "files": installed}
    atomic(output / MANIFEST, (json.dumps(manifest, indent=2) + "\n").encode())
    return bundle_url(source)


def sign(target, output, identity):
    if "apple" not in target:
        raise ValueError("Parser signing requires a macOS target")
    lock = json.loads(LOCK.read_text())
    manifest = staged(target, output, lock)
    for record in manifest["files"]:
        subprocess.run(["codesign", "--force", "--timestamp=none", "--sign", identity,
                        str(output / record["name"])], check=True, stdout=subprocess.DEVNULL)
    finalize(target, output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--cache", type=Path, default=Path(".runtime/code-index/resources"))
    parser.add_argument("--source-only", action="store_true")
    parser.add_argument("--sign")
    parser.add_argument("--finalize", action="store_true")
    args = parser.parse_args()
    if not args.source_only and (args.target is None or args.output is None):
        parser.error("--target and --output are required when preparing parser libraries")
    if args.source_only and (args.sign or args.finalize):
        parser.error("Signing requires prepared parser libraries")
    output = args.output.absolute() if args.output else None
    if args.finalize:
        finalize(args.target, output)
        return
    source = prepare(args.target, output, args.cache.absolute(), args.source_only)
    if args.sign:
        sign(args.target, output, args.sign)
    print(source)


if __name__ == "__main__":
    main()
