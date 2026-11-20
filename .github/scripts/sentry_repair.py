import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


SHA = r"[0-9a-f]{40}"
TAG = r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
TEMPLATE = "queue-stopping"
PACKAGE = "github.com/sailry/sailry-harness/internal/desktop/live"
TEST = "TestStoppingQueueGuidance"
PROBE = Path("internal/desktop/live/conversation_stopping_guidance_test.go")
ASSERTION = "STOPPING_QUEUE_GUIDANCE: SendQueued issued guide_queued_turn after the recipient began stopping"
PATCH = Path(".github/scripts/sentry-candidates/queue-stopping.patch")


def fields(name, event):
    if name == "repository_dispatch" and event.get("action") == "sentry-repair":
        data = event.get("client_payload")
    elif name == "workflow_dispatch":
        data = event.get("inputs")
    else:
        raise ValueError("unsupported event")
    if not isinstance(data, dict) or set(data) != {"fingerprint", "template", "target"}:
        raise ValueError("expected only fingerprint, template and target")
    if not all(isinstance(v, str) for v in data.values()):
        raise ValueError("expected string fields")
    if not re.fullmatch(r"[0-9a-f]{64}", data["fingerprint"]) or data["template"] != TEMPLATE:
        raise ValueError("invalid fingerprint or unsupported template")
    if not re.fullmatch(f"(?:{SHA}|{TAG})", data["target"]):
        raise ValueError("expected a stable release tag or exact lowercase commit SHA")
    return data


def git(*args, cwd=None):
    return subprocess.check_output(["git", *args], cwd=cwd, text=True).strip()


def append(path, line):
    with open(os.environ[path], "a", encoding="utf-8") as stream:
        print(line, file=stream)


def note(line):
    print(line, flush=True)
    append("GITHUB_STEP_SUMMARY", line)


def resolve():
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    data = fields(os.environ["GITHUB_EVENT_NAME"], event)
    target = data["target"]
    if re.fullmatch(TAG, target):
        release = json.loads(subprocess.check_output([
            "gh", "release", "view", target, "--repo", os.environ["GITHUB_REPOSITORY"],
            "--json", "isDraft,tagName",
        ]))
        if release != {"isDraft": False, "tagName": target}:
            raise ValueError("not a published release tag")
        target = git("rev-parse", "--verify", f"refs/tags/{target}^{{commit}}")
    elif git("cat-file", "-t", target) != "commit":
        raise ValueError("target is not a commit")
    # A GitHub clone may contain objects from other refs, including PR heads.
    git("merge-base", "--is-ancestor", target, os.environ["GITHUB_SHA"])
    append("GITHUB_OUTPUT", "fingerprint=" + data["fingerprint"])
    append("GITHUB_OUTPUT", "target=" + target)
    note(f"Group `{data['fingerprint']}`, `{TEMPLATE}`, affected `{target}`, base `{os.environ['GITHUB_SHA']}`.")


def classify(code, rows):
    pkg = [r for r in rows if r.get("Package") == PACKAGE]
    tests = [r for r in pkg if r.get("Test") == TEST]
    actions = [r["Action"] for r in tests if r["Action"] != "output"]
    ended = [r["Action"] for r in pkg if "Test" not in r and r["Action"] in ("pass", "fail", "skip")]
    if code == 0 and actions == ["run", "pass"] and ended == ["pass"]:
        return "green"
    output = [r["Output"].strip() for r in tests if r["Action"] == "output"]
    messages = [s for s in output if not s.startswith(("=== RUN", "--- FAIL:"))]
    marker = re.escape(PROBE.name) + r":\d+: " + re.escape(ASSERTION)
    if code == 1 and actions == ["run", "fail"] and ended == ["fail"] and len(messages) == 1 and re.fullmatch(marker, messages[0]):
        return "red"
    return "inconclusive"


def test(tree, probe):
    # Overlay the trusted probe even when that file does not exist at the tag.
    overlay = tree.parent / "overlay.json"
    overlay.write_text(json.dumps({"Replace": {str(tree / PROBE): str(probe)}}))
    env = dict(os.environ, GOTOOLCHAIN="go1.27.3", CGO_ENABLED="0")
    env.pop("GOROOT", None)
    result = subprocess.run([
        "go", "test", "-json", "-count=1", "-overlay", str(overlay), "-run", f"^{TEST}$", "./internal/desktop/live",
    ], cwd=tree, env=env, capture_output=True, text=True)
    try:
        state = classify(result.returncode, [json.loads(line) for line in result.stdout.splitlines()])
    except (ValueError, KeyError):
        state = "inconclusive"
    if state == "inconclusive":
        # Keep enough of the runner's diagnostics to distinguish setup/build
        # errors from other assertions. Never interpret them as Actions commands.
        print("Go diagnostics:", repr((result.stderr + result.stdout)[-3500:]))
    return state


def checkout(path, commit):
    git("worktree", "add", "--detach", str(path), commit)
    git("-c", "protocol.allow=never", "-c", "protocol.https.allow=always", "submodule", "update", "--init", "--recursive", cwd=path)


def matches_index(path, tree):
    # An unstaged source change or an untracked Go file could make the tested
    # code differ from what the isolated publisher reconstructs from the index.
    return (git("write-tree", cwd=path) == tree
            and not git("diff", "--name-only", "--ignore-submodules=none", cwd=path)
            and not git("ls-files", "--others", cwd=path))


def verify():
    base, target = os.environ["GITHUB_SHA"], os.environ["TARGET"]
    patch, probe = PATCH.absolute(), PROBE.resolve(strict=True)
    with tempfile.TemporaryDirectory(dir=os.environ["RUNNER_TEMP"]) as temp:
        root = Path(temp)
        affected, main = root / "affected", root / "main"
        checkout(affected, target)
        state = test(affected, probe)
        note(f"Affected commit: **{state}** (`{TEST}`).")
        if state != "red":
            note("not_reproduced: probe passes on the affected revision; no draft." if state == "green" else "inconclusive: no matching assertion on the affected revision; no draft.")
            return
        if target == base:
            main = affected
        else:
            checkout(main, base)
            state = test(main, probe)
        note(f"Default branch at verification: **{state}**.")
        if state != "red":
            note("already_fixed: probe passes on the default branch; no draft." if state == "green" else "inconclusive: default branch could not be verified; no draft.")
            return
        if not patch.is_file():
            note("no_candidate: no reviewed patch for this template; no draft.")
            return
        if patch.is_symlink() or patch.resolve() != patch:
            raise ValueError("candidate must be a regular file in the trusted checkout")
        base_tree = git("rev-parse", "HEAD^{tree}", cwd=main)
        if not matches_index(main, base_tree):
            raise ValueError("baseline probe changed the checkout; candidate not verified")
        try:
            git("apply", "--index", str(patch), cwd=main)
        except subprocess.CalledProcessError:
            note("candidate_conflict: reviewed patch no longer applies cleanly; no draft.")
            return
        allowed = {"internal/desktop/live/conversation.go", "internal/desktop/live/conversation_actions.go"}
        if not set(git("diff", "--cached", "--name-only", cwd=main).splitlines()) <= allowed:
            raise ValueError("queue candidate changed files outside its reviewed scope")
        tree = git("write-tree", cwd=main)
        if tree == base_tree:
            raise ValueError("candidate has no changes")
        state = test(main, probe)
        note(f"Reviewed candidate: **{state}** (`{tree}`).")
        if state == "green":
            if not matches_index(main, tree):
                raise ValueError("green probe changed the candidate tree; no draft")
            append("GITHUB_OUTPUT", "tree=" + tree)
            note("Eligible for a draft only if repository publication is enabled.")
        else:
            note("Candidate did not verify; no draft.")


if __name__ == "__main__":
    {"resolve": resolve, "verify": verify}[sys.argv[1]]()
