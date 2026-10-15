"""Shared verification for version-tagged GitHub release workflows."""

import hashlib
import json
import subprocess
import tempfile
from urllib.parse import quote

import versions

PREREQUISITES = {
    "desktop": ("Source", "Shared Node", "Desktop"),
    "host": ("Source", "Shared Node", "Host"),
    "mobile": ("Source", "Shared Node", "Mobile, shared bridge and iOS",
               "Mobile Dart FFI contracts", "Mobile Android"),
}


class GitHub:
    def api(self, method, endpoint, payload=None, missing=False):
        command = ["gh", "api", "--method", method, endpoint]
        if payload is not None:
            command.extend(["--input", "-"])
        result = subprocess.run(command, input=json.dumps(payload) if payload is not None else None,
                                capture_output=True, text=True)
        if result.returncode:
            if missing and "HTTP 404" in result.stderr:
                return None
            raise RuntimeError("GitHub release API request failed")
        return json.loads(result.stdout) if result.stdout.strip() else None

    def upload(self, tag, assets):
        result = subprocess.run(["gh", "release", "upload", tag, *(str(asset) for asset in assets)],
                                capture_output=True, text=True)
        if result.returncode:
            raise RuntimeError("Release asset upload failed; retry with the same workflow artifacts")

    def digest(self, repository, asset):
        with tempfile.TemporaryFile() as output:
            result = subprocess.run(["gh", "api", "-H", "Accept: application/octet-stream",
                                     f"repos/{repository}/releases/assets/{asset['id']}"],
                                    stdout=output, stderr=subprocess.PIPE)
            if result.returncode:
                raise RuntimeError("Existing release asset could not be verified")
            output.seek(0)
            return "sha256:" + hashlib.file_digest(output, "sha256").hexdigest()


def tagged_commit(tag, root=None):
    root = root or versions.ROOT
    version, _ = versions.product(root)
    if tag != f"v{version}":
        raise ValueError("The release tag must match every application version")
    def revision(value):
        return subprocess.check_output(["git", "-C", str(root), "rev-parse", "--verify", value], text=True).strip()
    commit = revision(f"refs/tags/{tag}^{{commit}}")
    if revision("HEAD") != commit:
        raise ValueError("The checkout must be the release tag's exact commit")
    return commit


def remote_commit(client, repository, tag):
    return client.api("GET", f"repos/{repository}/commits/{quote(tag, safe='')}")["sha"]


def release(client, repository, tag, commit, published=False):
    metadata = client.api("GET", f"repos/{repository}/releases/tags/{quote(tag, safe='')}", missing=True)
    if remote_commit(client, repository, tag) != commit:
        raise ValueError("The remote release tag differs from the verified commit")
    if metadata is None:
        if published:
            raise ValueError("Desktop must publish this release before Host or publication follow-ups run")
        return None
    preview = versions.parse(tag[1:]).group("preview") is not None
    if metadata.get("tag_name") != tag or bool(metadata.get("prerelease")) != preview:
        raise ValueError("The release tag or channel does not match the application version")
    if published and (metadata.get("draft") is not False or not metadata.get("published_at")):
        raise ValueError("A published Desktop release is required")
    return metadata


def readiness(client, repository, commit, component):
    gate = f"{component.title()} ready"
    required = {*PREREQUISITES[component], gate}
    page = 1
    while True:
        runs = client.api("GET", f"repos/{repository}/actions/workflows/ci.yml/runs?head_sha={commit}&per_page=100&page={page}")["workflow_runs"]
        for run in runs:
            if (run.get("head_sha") != commit or run.get("head_branch") != "main"
                    or run.get("event") not in {"push", "workflow_dispatch"}):
                continue
            jobs = client.api("GET", f"repos/{repository}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")["jobs"]
            successful = {job.get("name") for job in jobs if job.get("status") == "completed"
                          and job.get("conclusion") == "success"}
            if required.issubset(successful):
                return run["id"]
        if len(runs) < 100:
            break
        page += 1
    raise ValueError(f"The exact release commit needs a successful {gate} gate from full main Checks")


def event(metadata, repository):
    return {"action": "published", "repository": {"full_name": repository}, "release": metadata}
