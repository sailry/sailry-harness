"""Shared verification for version-tagged GitHub release workflows."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import re
import stat
import subprocess
import tempfile
import time
from urllib.parse import quote
import zipfile

import versions

_reuse_spec = importlib.util.spec_from_file_location("_sailry_ci_reuse", Path(__file__).resolve().parents[1] / "ci_reuse.py")
ci_reuse = importlib.util.module_from_spec(_reuse_spec)
_reuse_spec.loader.exec_module(ci_reuse)

PREREQUISITES = {
    "desktop": ("Source", "Rust strict lint", "Test build", "Shared Node (1/2)", "Shared Node (2/2)", "Desktop"),
    "host": ("Source", "Rust strict lint", "Test build", "Shared Node (1/2)", "Shared Node (2/2)", "Host"),
    "mobile": ("Source", "Rust strict lint", "Test build", "Shared Node (1/2)", "Shared Node (2/2)", "Mobile", "Mobile Android"),
}


# Product Checks remain valid when only the reviewed signing tool and its
# provenance checks change. Workflow changes require their own product Checks.
SIGNING_FIX_FILES = frozenset({
    "scripts/package/dmg.py",
    "scripts/package/notarize-macos.py",
    "scripts/package/sign-macos.py",
    "scripts/package/signing-credentials.py",
    "scripts/package/publication.py",
    "scripts/tests/test_macos_package.py",
    "scripts/tests/test_publish_packages.py",
})


def checked_source(commit, root=None):
    root = root or versions.ROOT
    def git(*arguments, check=True):
        result = subprocess.run(["git", "-C", str(root), *arguments], capture_output=True)
        if check and result.returncode:
            raise ValueError("The release source could not be verified")
        return result
    if git("rev-parse", "HEAD").stdout.decode().strip() != commit:
        return None
    header = git("cat-file", "-p", commit).stdout.split(b"\n\n", 1)[0]
    parents = [line.split(b" ", 1)[1].decode() for line in header.splitlines() if line.startswith(b"parent ")]
    if len(parents) != 1:
        return None
    parent = parents[0]
    if git("cat-file", "-e", parent + "^{commit}", check=False).returncode:
        git("fetch", "--quiet", "--no-tags", "--depth=1", "origin", parent)
    changes = git("diff", "--raw", "-z", "--no-renames", "--no-ext-diff", "--no-textconv", "--ignore-submodules=none", parent, commit, "--").stdout.split(b"\0")
    if changes == [b""]:
        return None
    for index in range(0, len(changes) - 1, 2):
        fields = changes[index].split()
        path = changes[index + 1].decode()
        if (path not in SIGNING_FIX_FILES or fields[-1] != b"M"
                or fields[0] != b":" + fields[1]
                or fields[1] not in {b"100644", b"100755"}):
            return None
    counts = git("diff", "--numstat", "-z", "--no-renames", "--no-ext-diff", "--no-textconv", "--ignore-submodules=none", parent, commit, "--").stdout
    if any(row.split(b"\t", 2)[:2] == [b"-", b"-"] for row in counts.split(b"\0") if row):
        return None
    return parent


class GitHub:
    def build_log(self, repository, run_id, job_name):
        result = subprocess.run(["gh", "api", f"repos/{repository}/actions/runs/{run_id}/logs"],
                                capture_output=True)
        if result.returncode or not result.stdout.startswith(b"PK\x03\x04"):
            raise RuntimeError("The original native build archive could not be downloaded")
        try:
            with zipfile.ZipFile(io.BytesIO(result.stdout)) as archive:
                matches = [entry for entry in archive.infolist()
                           if re.fullmatch(r"[0-9]+_" + re.escape(job_name) + r"\.txt", entry.filename)
                           and not entry.is_dir()]
                if len(matches) != 1:
                    raise ValueError("The original native target log is missing or ambiguous")
                return archive.read(matches[0]).decode("utf-8")
        except (zipfile.BadZipFile, UnicodeDecodeError, RuntimeError):
            raise RuntimeError("The original native target log archive is invalid") from None

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
    page = 1
    while metadata is None:
        releases = client.api("GET", f"repos/{repository}/releases?per_page=100&page={page}")
        metadata = next((entry for entry in releases if entry.get("tag_name") == tag), None)
        if metadata is not None or len(releases) < 100:
            break
        page += 1
    if remote_commit(client, repository, tag) != commit:
        raise ValueError("The remote release tag differs from the verified commit")
    if metadata is None:
        if published:
            raise ValueError("Publish this version release before appending packages or running follow-ups")
        return None
    preview = versions.parse(tag[1:]).group("preview") is not None
    if metadata.get("tag_name") != tag or (metadata.get("prerelease") and not preview):
        raise ValueError("The release tag or channel does not match the application version")
    if published and (metadata.get("draft") is not False or not metadata.get("published_at")):
        raise ValueError("A published version release is required")
    return metadata


def digest(path):
    with path.open("rb") as source:
        return "sha256:" + hashlib.file_digest(source, "sha256").hexdigest()


def append(client, repository, tag, metadata, assets):
    if metadata.get("immutable"):
        raise ValueError("Release assets cannot be appended to an immutable release")
    existing = {asset["name"]: asset for asset in metadata.get("assets", [])}
    missing = []
    for path in assets:
        asset = existing.get(path.name)
        if asset is None:
            missing.append(path)
            continue
        if asset.get("state") != "uploaded" or asset.get("size") != path.stat().st_size:
            raise ValueError(f"Release asset collision: {path.name}; existing assets cannot be replaced")
        actual = asset.get("digest") or client.digest(repository, asset)
        if actual != digest(path):
            raise ValueError(f"Release asset collision: {path.name}; existing assets cannot be replaced")
    if missing:
        client.upload(tag, missing)


def job_results(client, repository, commit, run_id, component, jobs):
    fresh = {"Source", f"{component.title()} ready"}
    required = {*PREREQUISITES[component], *fresh}
    jobs = [job for job in jobs if job.get("name") in required]
    successful = {job["name"] for job in jobs if job.get("status") == "completed"
                  and job.get("conclusion") == "success"}
    skipped = {job["name"] for job in jobs if job.get("status") == "completed"
               and job.get("conclusion") == "skipped"}
    unfinished = {job["name"] for job in jobs if job.get("status") != "completed"}
    if (skipped & fresh or any(job.get("status") == "completed"
            and job.get("conclusion") not in {"success", "skipped"} for job in jobs)):
        return successful, True
    reusable = required - fresh - successful - unfinished
    if reusable and (fresh <= successful or skipped & reusable):
        successful |= reusable & ci_reuse.verified_jobs(client, repository, commit, run_id)
    return successful - unfinished, bool(skipped - successful)


def ready_run(client, repository, commit, component):
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
            successful, blocked = job_results(client, repository, commit, run["id"], component, jobs)
            if not blocked and required.issubset(successful):
                return run["id"]
        if len(runs) < 100:
            break
        page += 1
    return None


def recovered_run(client, repository, commit, component, root=None):
    root = root or ci_reuse.ROOT
    revision = ci_reuse.git(root, "rev-parse", "HEAD").decode().strip()
    if revision == commit:
        return None
    if ci_reuse.git(root, "rev-parse", "--is-shallow-repository").strip() == b"true":
        ci_reuse.git(root, "fetch", "--quiet", "--no-tags", "--no-recurse-submodules",
                     "--unshallow", "origin", revision)
    try:
        ci_reuse.equivalent_inputs(root, commit, revision)
    except ValueError as error:
        print(f"Recovered Checks cannot verify product equivalence: {error}", flush=True)
        return None
    run_id = ready_run(client, repository, revision, component)
    if run_id is None:
        revision = checked_source(revision, root=root)
        if revision is not None:
            run_id = ready_run(client, repository, revision, component)
    if run_id is None:
        print("Recovered Checks have no successful gate at the verified tooling source", flush=True)
        return None
    metadata = {"id": run_id, "head_sha": revision}
    artifact = ci_reuse.find_artifact(client, repository, metadata, "checks-reuse", optional=True)
    if artifact is None:
        return None
    proof = ci_reuse.read_proof(repository, artifact)
    if not isinstance(proof, dict) or proof.get("source_sha") != commit:
        return None
    ci_reuse.validate_proof(client, repository, revision, run_id, proof, root)
    print(f"Verified recovered {component.title()} Checks for unchanged product source {commit} in run {run_id}")
    return run_id


def readiness(client, repository, commit, component):
    run = ready_run(client, repository, commit, component)
    if run is not None:
        return run
    source = checked_source(commit)
    if source is not None:
        run = ready_run(client, repository, source, component)
        if run is not None:
            print(f"Reused product Checks for unchanged source {source}; only signing tools differ")
            return run
    run = recovered_run(client, repository, commit, component)
    if run is not None:
        return run
    raise ValueError(f"The release product needs a successful {component.title()} ready gate from main Checks")


def wait_ready(client, repository, commit, component):
    deadline = time.monotonic() + 5 * 60 * 60
    while True:
        ready = ready_run(client, repository, commit, component)
        if ready is not None:
            checked = client.api("GET", f"repos/{repository}/actions/runs/{ready}")
            if (checked.get("id") != ready or checked.get("head_sha") != commit
                    or checked.get("head_branch") != "main"
                    or checked.get("event") not in {"push", "workflow_dispatch"}):
                raise ValueError("The ready Checks run does not match the release source")
            return ready
        recovered = recovered_run(client, repository, commit, component)
        if recovered is not None:
            return recovered
        payload = client.api("GET", f"repos/{repository}/actions/workflows/ci.yml/runs?head_sha={commit}&per_page=100")
        runs = [run for run in payload["workflow_runs"]
                if run.get("head_sha") == commit and run.get("head_branch") == "main"
                and run.get("event") in {"push", "workflow_dispatch"}]
        pending = False
        for run in runs:
            if run.get("status") == "completed":
                continue
            jobs = client.api("GET", f"repos/{repository}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")["jobs"]
            _, blocked = job_results(client, repository, commit, run["id"], component, jobs)
            pending |= not blocked
        if runs and not pending:
            raise ValueError(f"The exact-source Checks ended without successful {component.title()} readiness")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("The exact-source release gate did not become ready within five hours")
        print(f"Waiting for {component.title()} readiness at exact source {commit}", flush=True)
        time.sleep(min(60, remaining))


def native_binary(path, target):
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or not metadata.st_mode & 0o111:
        raise ValueError("The restored native executable is missing or unsafe")
    with path.open("rb") as executable:
        header = executable.read(8)
    architecture = {"aarch64-apple-darwin": 0x100000C, "x86_64-apple-darwin": 0x1000007}
    if (header[:4] != b"\xcf\xfa\xed\xfe" or len(header) != 8
            or int.from_bytes(header[4:], "little") != architecture.get(target)):
        raise ValueError("The restored native executable has a different architecture")


def event(metadata, repository):
    return {"action": "published", "repository": {"full_name": repository}, "release": metadata}
