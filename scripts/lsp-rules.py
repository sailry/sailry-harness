"""Refresh original pinned Helix and Mason snapshots after validating every asset."""
import datetime
import hashlib
import io
import json
import os
from pathlib import Path
import re
import sys
import subprocess
import tempfile
import tomllib
import urllib.parse
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DESTINATION = ROOT / "crates/node-runtime/src/code/upstream"
LIMIT = 16 * 1024 * 1024


def download(url):
    if urllib.parse.urlparse(url).scheme != "https":
        raise ValueError("snapshot downloads require HTTPS")
    request = urllib.request.Request(url, headers={"User-Agent": "Sailry-rules/1"})
    with urllib.request.urlopen(request, timeout=30) as response:
        if urllib.parse.urlparse(response.url).scheme != "https":
            raise ValueError("snapshot redirect requires HTTPS")
        data = response.read(LIMIT + 1)
        if len(data) > LIMIT:
            raise ValueError("snapshot exceeds the download limit")
        return data


def digest(data):
    return hashlib.sha256(data).hexdigest()


def validate(languages, registry):
    helix = tomllib.loads(languages.decode("utf-8"))
    if not isinstance(helix.get("language"), list) or not isinstance(helix.get("language-server"), dict):
        raise ValueError("Helix snapshot contains no language rules")
    with zipfile.ZipFile(io.BytesIO(registry)) as archive:
        info = archive.getinfo("registry.json")
        if info.file_size > LIMIT:
            raise ValueError("Mason registry exceeds the expanded limit")
        mason = json.loads(archive.read(info))
    if not isinstance(mason, list) or not any(isinstance(item, dict) and isinstance(item.get("bin"), dict) for item in mason):
        raise ValueError("Mason snapshot contains no executable packages")


def refresh(commit, tag):
    if not re.fullmatch(r"[0-9a-f]{40}", commit) or not re.fullmatch(r"[A-Za-z0-9._-]{1,128}", tag):
        raise ValueError("provide a full Helix commit and a literal Mason release tag")
    helix_url = f"https://raw.githubusercontent.com/helix-editor/helix/{commit}/languages.toml"
    mason_url = f"https://github.com/mason-org/mason-registry/releases/download/{tag}/registry.json.zip"
    languages = download(helix_url)
    registry = download(mason_url)
    checksums = download(mason_url.rsplit("/", 1)[0] + "/checksums.txt")
    expected = next((line.split()[0] for line in checksums.decode().splitlines() if len(line.split()) == 2 and line.split()[1].lstrip("*") == "registry.json.zip"), None)
    if expected != digest(registry):
        raise ValueError("Mason release checksum does not match registry.json.zip")
    validate(languages, registry)
    reference = json.loads(download(f"https://api.github.com/repos/mason-org/mason-registry/git/ref/tags/{tag}"))["object"]
    if reference["type"] == "tag":
        reference = json.loads(download(reference["url"]))["object"]
    if reference["type"] != "commit" or not re.fullmatch(r"[0-9a-f]{40}", reference["sha"]):
        raise ValueError("Mason tag does not resolve to an immutable commit")
    licenses = {
        "helix": download(f"https://raw.githubusercontent.com/helix-editor/helix/{commit}/LICENSE"),
        "mason": download(f"https://raw.githubusercontent.com/mason-org/mason-registry/{reference['sha']}/LICENSE"),
    }
    source = {
        "version": 1,
        "retrieved": datetime.datetime.now(datetime.timezone.utc).date().isoformat(),
        "helix": {"repository": "https://github.com/helix-editor/helix", "commit": commit, "url": helix_url, "sha256": digest(languages), "license": "MPL-2.0", "license_sha256": digest(licenses["helix"])},
        "mason": {"repository": "https://github.com/mason-org/mason-registry", "tag": tag, "commit": reference["sha"], "url": mason_url, "sha256": digest(registry), "license": "Apache-2.0", "license_sha256": digest(licenses["mason"])},
    }
    with tempfile.TemporaryDirectory(prefix="lsp-rules-", dir=DESTINATION.parent) as temporary:
        candidate = Path(temporary) / "upstream"
        candidate.mkdir()
        for name, data in {"languages.toml": languages, "registry.json.zip": registry, "checksums.txt": checksums, "helix-LICENSE": licenses["helix"], "mason-LICENSE": licenses["mason"], "SOURCE.json": (json.dumps(source, indent=2) + "\n").encode()}.items():
            (candidate / name).write_bytes(data)
        environment = dict(os.environ)
        environment["SAILRY_RULES_CANDIDATE"] = str(candidate)
        subprocess.run(["cargo", "test", "--locked", "-p", "sailry-node-runtime", "--lib", "code::rules::tests::parses_candidate", "--", "--ignored", "--exact"], cwd=ROOT, env=environment, check=True)
        previous = Path(temporary) / "previous"
        if DESTINATION.exists():
            os.rename(DESTINATION, previous)
        try:
            os.rename(candidate, DESTINATION)
        except OSError:
            if previous.exists():
                os.rename(previous, DESTINATION)
            raise
        notice = ROOT / "NOTICE"
        text = notice.read_text()
        marker = "Helix language rules and Mason package registry"
        text = re.sub(r"\n?Helix language rules and Mason package registry\n(?:[^\n]+\n){3}", "", text).rstrip() + "\n\n"
        text += f"{marker}\nHelix: https://github.com/helix-editor/helix at {commit} (MPL-2.0)\nMason: https://github.com/mason-org/mason-registry at {tag}, {reference['sha']} (Apache-2.0)\nOriginal snapshots, checksums, provenance and license copies: crates/node-runtime/src/code/upstream/\n"
        notice.write_text(text)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("Usage: lsp-rules.py <helix-commit> <mason-release-tag>")
    try:
        refresh(*sys.argv[1:])
    except (OSError, ValueError, KeyError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
        raise SystemExit(f"Snapshot update failed: {error}") from error
