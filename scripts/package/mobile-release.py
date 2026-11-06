"""Admit Mobile builds and append verified signed artifacts to published releases."""

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
    tag = environment.get("RELEASE_TAG", "")
    workflow = environment.get("WORKFLOW_REVISION", "")
    if not re.fullmatch(r"[0-9a-f]{40}", workflow) or workflow != environment.get("GITHUB_SHA"):
        raise ValueError("The Mobile workflow tooling must match its trusted event commit")
    if requested and not re.fullmatch(r"[0-9a-f]{40}", requested):
        raise ValueError("The Mobile source input must be a full commit SHA")
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("The Mobile checkout must be an exact source commit")
    if tag and (requested or event != "workflow_dispatch" or reference != "refs/heads/main"):
        raise ValueError("Dispatch a Mobile tag from main without a source override")
    if tag:
        if tag != f"v{version}":
            raise ValueError("The Mobile release tag must match the product version")
        return workflow, tag
    if revision != (requested or environment.get("GITHUB_SHA")):
        raise ValueError("The Mobile checkout must match the selected source commit")
    if event == "workflow_dispatch" and reference == "refs/heads/main":
        return workflow, ""
    if not requested and event == "workflow_dispatch" and reference == f"refs/tags/v{version}":
        return workflow, f"v{version}"
    raise ValueError("Dispatch Mobile packages from main or use the matching version tag")


def candidate(source, tooling, environment):
    version, code = versions.product(source)
    def head(root):
        return subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    revision = head(source)
    workflow, tag = context(version, revision, environment)
    if head(tooling) != workflow:
        raise ValueError("The Mobile tooling checkout must match the pinned workflow commit")
    if environment.get("SOURCE_COMMIT"):
        result = subprocess.run(["git", "-C", str(tooling), "merge-base", "--is-ancestor", revision, workflow],
                                capture_output=True)
        if result.returncode:
            raise ValueError("The selected Mobile source must be an ancestor of the main workflow commit")
    if tag:
        publication.tagged_commit(tag, source)
    return {"version": version, "apple_version": versions.apple(version), "build_number": code,
            "revision": revision, "workflow_revision": workflow, "tag": tag}


def admit(source=None, tooling=None):
    source = source or versions.ROOT
    values = candidate(source, tooling or versions.ROOT, os.environ)
    version, code, revision = values["version"], values["build_number"], values["revision"]
    client = publication.GitHub()
    repository = os.environ["GITHUB_REPOSITORY"]
    if values["tag"]:
        publication.release(client, repository, values["tag"], revision, published=True)
    checked = publication.wait_ready(client, repository, revision, "mobile", tag=values["tag"] or None)
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        for key, value in values.items():
            output.write(f"{key}={value}\n")
    print(f"Verified Mobile product source {revision} in Checks run {checked}: {version}+{code}; "
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


TARGETS = {"android": ("apk", "android-arm64"), "ios": ("ipa", "ios")}


def filenames(platform):
    """Unversioned names let `releases/latest/download/<name>` serve as a fixed download link."""
    extension, target = TARGETS[platform]
    name = f"Sailry-{target}"
    return f"{name}.{extension}", f"{name}.json", f"{name}.sha256"


def manifest(values, platform):
    return {"version": values["version"], "build_number": values["build_number"], "tag": values["tag"],
            "source_revision": values["revision"], "workflow_revision": values["workflow_revision"],
            "platform": platform, "asset": filenames(platform)[0]}


def checksums(paths):
    rows = []
    for path in paths:
        with path.open("rb") as data:
            rows.append(f"{hashlib.file_digest(data, 'sha256').hexdigest()}  {path.name}\n")
    return "".join(rows)


def package(path, output, platform, source=None, tooling=None):
    values = candidate(source or versions.ROOT, tooling or versions.ROOT, os.environ)
    archive, metadata_name, checksum_name = filenames(platform)
    extension = Path(archive).suffix
    if path.suffix != extension or not path.is_file() or not path.stat().st_size:
        raise ValueError("The signed Mobile package is missing or empty")
    output.mkdir(parents=True, exist_ok=True)
    destination = output / archive
    if destination.exists():
        raise ValueError("The Mobile package output already exists")
    shutil.copyfile(path, destination)
    metadata = output / metadata_name
    metadata.write_text(json.dumps(manifest(values, platform), indent=2) + "\n")
    (output / checksum_name).write_text(checksums((destination, metadata)))
    return destination


def verify_package(directory, platform, source, tooling):
    values = candidate(source, tooling, os.environ)
    names = filenames(platform)
    paths = [directory / name for name in names]
    if ({path.name for path in directory.iterdir()} != set(names)
            or any(not path.is_file() or path.is_symlink() or not path.stat().st_size for path in paths)):
        raise ValueError("The complete signed Mobile package, provenance and checksums are required")
    if json.loads(paths[1].read_text()) != manifest(values, platform):
        raise ValueError("The Mobile package provenance does not match the selected source and workflow")
    if paths[2].read_text() != checksums(paths[:2]):
        raise ValueError("The Mobile package checksums do not match the preserved files")
    return values, paths


def publish(directory, platform, source=None, tooling=None, client=None):
    values, paths = verify_package(directory, platform, source or versions.ROOT, tooling or versions.ROOT)
    if not values["tag"]:
        raise ValueError("Only tagged Mobile builds append assets to a published release")
    client = client or publication.GitHub()
    repository = os.environ["GITHUB_REPOSITORY"]
    metadata = publication.release(client, repository, values["tag"], values["revision"], published=True)
    publication.append(client, repository, values["tag"], metadata, paths)
    print(f"Appended verified {platform} assets for {values['tag']} at product source {values['revision']}; "
          f"workflow tooling {values['workflow_revision']}")


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
    publication_command = commands.add_parser("publish")
    publication_command.add_argument("--platform", choices=("android", "ios"), required=True)
    publication_command.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "admit":
        admit(args.source)
    elif args.command == "install-android":
        install_android()
    elif args.command == "cleanup-android":
        stage = android_stage()
        if stage.exists():
            shutil.rmtree(stage)
    elif args.command == "package":
        print(package(args.input, args.output, args.platform, args.source))
    else:
        publish(args.directory, args.platform, args.source)


if __name__ == "__main__":
    main()
