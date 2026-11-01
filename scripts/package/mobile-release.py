"""Admit Mobile distribution builds and stage their signed artifacts."""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

import publication
import versions


def context(version, revision, environment):
    event = environment.get("GITHUB_EVENT_NAME")
    reference = environment.get("GITHUB_REF")
    if not re.fullmatch(r"[0-9a-f]{40}", revision) or environment.get("GITHUB_SHA") != revision:
        raise ValueError("The Mobile checkout must match the workflow's exact source")
    if event == "workflow_dispatch" and reference == "refs/heads/main":
        return
    if event in {"push", "workflow_dispatch"} and reference == f"refs/tags/v{version}":
        return
    raise ValueError("Dispatch Mobile packages from main or use the matching version tag")


def admit():
    version, code = versions.product(versions.ROOT)
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    context(version, revision, os.environ)
    if os.environ["GITHUB_REF"].startswith("refs/tags/"):
        publication.tagged_commit(f"v{version}")
    checked = publication.wait_ready(publication.GitHub(), os.environ["GITHUB_REPOSITORY"], revision, "mobile")
    values = {"version": version, "apple_version": versions.apple(version),
              "build_number": code, "revision": revision}
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        for key, value in values.items():
            output.write(f"{key}={value}\n")
    print(f"Verified Mobile source {revision} in full Checks run {checked}: {version}+{code}")


def private_file(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "wb") as output:
        output.write(data)
    path.chmod(0o600)


def android_stage():
    return Path(os.environ["RUNNER_TEMP"]) / "sailry-android"


def install_android():
    names = ("ANDROID_KEYSTORE_BASE64", "ANDROID_STORE_PASSWORD", "ANDROID_KEY_ALIAS", "ANDROID_KEY_PASSWORD")
    if not all(os.environ.get(name) for name in names):
        raise ValueError("Android distribution signing secrets are not configured")
    if not re.fullmatch(r"[0-9a-fA-F]{64}", os.environ.get("ANDROID_SIGNING_SHA256", "")):
        raise ValueError("The Android distribution certificate fingerprint is not configured")
    data = base64.b64decode(os.environ["ANDROID_KEYSTORE_BASE64"], validate=True)
    if not data:
        raise ValueError("The Android distribution keystore is empty")
    stage = android_stage()
    stage.mkdir(mode=0o700)
    try:
        key = stage / "release.jks"
        private_file(key, data)
        with Path(os.environ["GITHUB_ENV"]).open("a") as output:
            output.write(f"ANDROID_KEYSTORE_PATH={key}\nSAILRY_ANDROID_RELEASE=1\n")
    except BaseException:
        shutil.rmtree(stage)
        raise


def package(source, output, platform, version, code, revision):
    extension, target = {"android": ("apk", "android-arm64"), "ios": ("ipa", "ios")}[platform]
    if source.suffix != f".{extension}" or not source.is_file() or not source.stat().st_size:
        raise ValueError("The signed Mobile package is missing or empty")
    output.mkdir(parents=True, exist_ok=True)
    destination = output / f"Sailry-{version}-{target}.{extension}"
    if destination.exists():
        raise ValueError("The Mobile package output already exists")
    shutil.copyfile(source, destination)
    metadata = output / "release.json"
    metadata.write_text(json.dumps({"version": version, "build_number": code, "source_revision": revision,
                                    "platform": platform, "asset": destination.name}, indent=2) + "\n")
    with (output / "SHA256SUMS").open("w") as checksums:
        for path in (destination, metadata):
            with path.open("rb") as data:
                digest = hashlib.file_digest(data, "sha256").hexdigest()
            checksums.write(f"{digest}  {path.name}\n")
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("admit")
    commands.add_parser("install-android")
    commands.add_parser("cleanup-android")
    artifact = commands.add_parser("package")
    artifact.add_argument("--platform", choices=("android", "ios"), required=True)
    artifact.add_argument("--input", type=Path, required=True)
    artifact.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "admit":
        admit()
    elif args.command == "install-android":
        install_android()
    elif args.command == "cleanup-android":
        stage = android_stage()
        if stage.exists():
            shutil.rmtree(stage)
    else:
        version, code = versions.product(versions.ROOT)
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        context(version, revision, os.environ)
        print(package(args.input, args.output, args.platform, version, code, revision))


if __name__ == "__main__":
    main()
