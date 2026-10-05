"""Check publication boundaries and export one committed public source snapshot."""

import argparse
import io
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parents[1]
GUIDES = {"README.md", "ARCHITECTURE.md", "CONTRIBUTING.md", "AGENTS.md"}
REQUIRED = GUIDES | {"LICENSE", "NOTICE", ".gitignore", "Cargo.lock"}
INTERNAL = {
    "docs", "plan", "plans", "notes", "reports", "internal", "output",
    ".planning", ".internal", ".migration-backups", ".sailry", ".secrets",
    ".codux", ".codex", ".claude", ".cursor", ".idea", ".vscode",
}
GENERATED = {".git", "node_modules", ".runtime", ".preview", ".cache", "target", "dist", "__pycache__"}
PRIVATE_FILES = {"CLAUDE.md", "GEMINI.md", "scripts/snapshot-migration.sh", ".cargo/config.local.toml"}
LINK = re.compile(r"\[[^\]\n]*\]\(([^\s)]+)(?:\s+\"[^\"]*\")?\)")


def git(root, *args, input=None):
    return subprocess.run(
        ["git", "-C", str(root), *args], input=input, capture_output=True, check=True,
    ).stdout


def source_paths(root, revision=None):
    output = (git(root, "ls-tree", "-rz", "--name-only", revision) if revision
              else git(root, "ls-files", "-z"))
    paths = {path.decode("utf-8") for path in output.split(b"\0") if path}
    for path, commit in gitlinks(root, revision):
        module = root / path
        if not (module / ".git").exists():
            raise ValueError("Submodules are missing; run git submodule update --init --recursive")
        paths.update(f"{path}/{child}" for child in source_paths(module, commit))
    return paths


def gitlinks(root, revision=None):
    output = (git(root, "ls-tree", "-rz", revision) if revision
              else git(root, "ls-files", "--stage", "-z"))
    links = []
    for entry in output.split(b"\0"):
        if not entry:
            continue
        metadata, path = entry.split(b"\t", 1)
        fields = metadata.decode().split()
        if fields[0] == "160000":
            links.append((path.decode("utf-8"), fields[2] if revision else fields[1]))
    return links


def validate_paths(paths):
    errors = []
    for name in sorted(paths):
        path = PurePosixPath(name)
        env = path.name.startswith(".env") and not (
            path.name == ".env.example" or path.name.endswith(".example")
        )
        if (path.is_absolute() or ".." in path.parts or path.parts[0] in INTERNAL
                or GENERATED.intersection(path.parts) or name in PRIVATE_FILES
                or path.name == ".npmrc" or env
                or path.suffix in {".pem", ".key", ".p12", ".pfx", ".jks", ".keystore"}):
            errors.append(f"Non-public tracked path: {name}")
    for name in sorted(REQUIRED - paths):
        errors.append(f"Missing public source file: {name}")
    return errors


def document_paths(paths):
    # Skills and vendor fixtures can contain illustrative links, not guide links.
    return sorted(name for name in paths if name in GUIDES
                  or name.startswith("sdk/") and name.endswith(".md") or (
                      name.endswith(".md")
                      and PurePosixPath(name).name in {"README.md", "SDK.md"}
                      and not name.startswith("vendor/")))


def check_documents(root, paths):
    errors = []
    for name in document_paths(paths):
        text = (root / name).read_text(encoding="utf-8")
        if re.search(r"/Volumes/|/Users/|[\u3400-\u9fff]", text):
            errors.append(f"Non-public or non-English guide text: {name}")
        # Examples inside code fences are not documentation navigation.
        text = re.sub(r"```.*?```", "", text, flags=re.S)
        text = re.sub(r"`[^`\n]*`", "", text)
        for link in LINK.findall(text):
            parsed = urlsplit(link.strip("<>"))
            if parsed.scheme or parsed.netloc or not parsed.path:
                continue
            target = ((root / name).parent / unquote(parsed.path)).resolve()
            if not target.is_relative_to(root.resolve()):
                errors.append(f"Guide link escapes repository: {name}: {link}")
                continue
            relative = target.relative_to(root.resolve()).as_posix()
            tracked = relative in paths or any(p.startswith(relative + "/") for p in paths)
            if not target.exists() or not tracked:
                errors.append(f"Guide link has no public source target: {name}: {link}")
    return errors


def check_source(root):
    paths = source_paths(root)
    errors = validate_paths(paths)
    ignored = subprocess.run(
        ["git", "-C", str(root), "check-ignore", "--no-index", "-z", "--stdin"],
        input="".join(name + "\0" for name in sorted(paths)).encode(), capture_output=True,
    )
    if ignored.returncode not in {0, 1}:
        raise RuntimeError("Unable to check tracked source against ignore rules")
    errors.extend("Ignored file is tracked: " + path.decode()
                  for path in ignored.stdout.split(b"\0") if path)
    errors.extend(check_documents(root, paths))
    if errors:
        raise ValueError("\n".join(errors))
    return paths


def export_source(root, destination):
    # Refuse unsafe committed input before creating any destination.
    revision = git(root, "rev-parse", "HEAD").decode().strip()
    errors = validate_paths(source_paths(root, revision))
    if errors:
        raise ValueError("\n".join(errors))
    archive = git(root, "archive", "--format=tar", revision)
    author = {key: git(root, "config", key).decode().strip()
              for key in ("user.name", "user.email")}
    destination.mkdir()  # Existing files/directories are never overwritten.
    with tarfile.open(fileobj=io.BytesIO(archive)) as source:
        source.extractall(destination, filter="data")
    git(destination, "init", "--initial-branch=main", "--template=")
    for key, value in author.items():
        git(destination, "config", key, value)
    # Preserve dependency ownership and the exact reviewed gitlinks, not a
    # second tracked copy of their source or whichever remote HEAD is newest.
    links = gitlinks(root, revision)
    for path, commit in links:
        git(destination, "update-index", "--add", "--cacheinfo", "160000", commit, path)
    if links:
        git(destination, "submodule", "update", "--init", "--recursive")
    git(destination, "add", "--all")
    check_source(destination)
    git(destination, "commit", "-m", "Initial public source")
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--export", type=Path, metavar="NEW_DIRECTORY")
    args = parser.parse_args()
    try:
        if args.export:
            destination = export_source(ROOT, args.export.absolute())
            print(f"Created a new public repository: {destination}")
        else:
            paths = check_source(ROOT)
            print(f"Checked {len(paths)} tracked public source files")
    except (ValueError, OSError, subprocess.CalledProcessError, tarfile.TarError) as error:
        parser.exit(1, f"Public source check failed: {error}\n")


if __name__ == "__main__":
    main()
