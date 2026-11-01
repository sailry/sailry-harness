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
    requested = environment.get("SOURCE_COMMIT", "")
    workflow = environment.get("WORKFLOW_REVISION", "")
    if not re.fullmatch(r"[0-9a-f]{40}", workflow) or workflow != environment.get("GITHUB_SHA"):
        raise ValueError("The Mobile workflow tooling must match its trusted event commit")
    if requested and not re.fullmatch(r"[0-9a-f]{40}", requested):
        raise ValueError("The Mobile source input must be a full commit SHA")
    if not re.fullmatch(r"[0-9a-f]{40}", revision) or revision != (requested or environment.get("GITHUB_SHA")):
        raise ValueError("The Mobile checkout must match the selected source commit")
    if event == "workflow_dispatch" and reference == "refs/heads/main":
        return workflow
    if not requested and event in {"push", "workflow_dispatch"} and reference == f"refs/tags/v{version}":
        return workflow
    raise ValueError("Dispatch Mobile packages from main or use the matching version tag")


def candidate(source, tooling, environment):
    version, code = versions.product(source)
    def head(root):
        return subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    revision = head(source)
    workflow = context(version, revision, environment)
    if head(tooling) != workflow:
        raise ValueError("The Mobile tooling checkout must match the pinned workflow commit")
    if environment.get("SOURCE_COMMIT"):
        result = subprocess.run(["git", "-C", str(tooling), "merge-base", "--is-ancestor", revision, workflow],
                                capture_output=True)
        if result.returncode:
            raise ValueError("The selected Mobile source must be an ancestor of the main workflow commit")
    return {"version": version, "apple_version": versions.apple(version), "build_number": code,
            "revision": revision, "workflow_revision": workflow}


def admit(source=None, tooling=None):
    source = source or versions.ROOT
    values = candidate(source, tooling or versions.ROOT, os.environ)
    version, code, revision = values["version"], values["build_number"], values["revision"]
    if os.environ["GITHUB_REF"].startswith("refs/tags/"):
        publication.tagged_commit(f"v{version}", source)
    checked = publication.wait_ready(publication.GitHub(), os.environ["GITHUB_REPOSITORY"], revision, "mobile")
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        for key, value in values.items():
            output.write(f"{key}={value}\n")
    print(f"Verified Mobile product source {revision} in full Checks run {checked}: {version}+{code}; "
          f"workflow tooling {values['workflow_revision']}")


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


def package(path, output, platform, source=None, tooling=None):
    values = candidate(source or versions.ROOT, tooling or versions.ROOT, os.environ)
    version = values["version"]
    extension, target = {"android": ("apk", "android-arm64"), "ios": ("ipa", "ios")}[platform]
    if path.suffix != f".{extension}" or not path.is_file() or not path.stat().st_size:
        raise ValueError("The signed Mobile package is missing or empty")
    output.mkdir(parents=True, exist_ok=True)
    destination = output / f"Sailry-{version}-{target}.{extension}"
    if destination.exists():
        raise ValueError("The Mobile package output already exists")
    shutil.copyfile(path, destination)
    metadata = output / "release.json"
    metadata.write_text(json.dumps({"version": version, "build_number": values["build_number"],
                                    "source_revision": values["revision"], "workflow_revision": values["workflow_revision"],
                                    "platform": platform, "asset": destination.name}, indent=2) + "\n")
    with (output / "SHA256SUMS").open("w") as checksums:
        for path in (destination, metadata):
            with path.open("rb") as data:
                digest = hashlib.file_digest(data, "sha256").hexdigest()
            checksums.write(f"{digest}  {path.name}\n")
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, help="Exact product checkout when using separate workflow tooling")
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
        admit(args.source)
    elif args.command == "install-android":
        install_android()
    elif args.command == "cleanup-android":
        stage = android_stage()
        if stage.exists():
            shutil.rmtree(stage)
    else:
        print(package(args.input, args.output, args.platform, args.source))


if __name__ == "__main__":
    main()
