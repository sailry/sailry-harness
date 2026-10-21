#!/usr/bin/env python3
"""Sign native code in a fresh macOS application and separate Host package."""

import argparse
from contextlib import contextmanager
import importlib.util
import os
import re
import shlex
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
MAGIC = {
    b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe",
    b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca", b"\xca\xfe\xba\xbf", b"\xbf\xba\xfe\xca",
}

runtime_spec = importlib.util.spec_from_file_location("onnx_runtime", Path(__file__).with_name("onnx-runtime.py"))
runtime = importlib.util.module_from_spec(runtime_spec)
runtime_spec.loader.exec_module(runtime)
llama_spec = importlib.util.spec_from_file_location("llama_runtime", Path(__file__).with_name("llama-runtime.py"))
llama_runtime = importlib.util.module_from_spec(llama_spec)
llama_spec.loader.exec_module(llama_runtime)
python_spec = importlib.util.spec_from_file_location("python_runtime", Path(__file__).with_name("python-runtime.py"))
python_runtime = importlib.util.module_from_spec(python_spec)
python_spec.loader.exec_module(python_runtime)

ocu_spec = importlib.util.spec_from_file_location("ocu_helper", Path(__file__).with_name("ocu-helper.py"))
ocu_helper = importlib.util.module_from_spec(ocu_spec)
ocu_spec.loader.exec_module(ocu_helper)


def run(*args):
    result = subprocess.run([str(arg) for arg in args], capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or
                           f"{args[0]} failed with exit code {result.returncode}")
    return result.stdout + result.stderr


@contextmanager
def keychain_scope(keychain, identity):
    if keychain is None:
        yield
        return
    original = shlex.split(run("security", "list-keychains", "-d", "user"))
    try:
        if str(keychain) not in original:
            run("security", "list-keychains", "-d", "user", "-s", *original, keychain)
        valid = run("security", "find-identity", "-v", "-p", "codesigning", keychain)
        if re.fullmatch(r"[0-9a-fA-F]{40}", identity):
            available = identity.upper() in re.findall(r"\b[0-9A-F]{40}\b", valid.upper())
        else:
            available = any(identity in name for name in re.findall(r'"([^"\n]+)"', valid))
        if not available:
            raise RuntimeError("The configured signing identity is unavailable in the temporary keychain")
        yield
    finally:
        failing = sys.exc_info()[0] is not None
        try:
            run("security", "list-keychains", "-d", "user", "-s", *original)
        except Exception:
            if not failing:
                raise
            print("The original keychain search list could not be restored", file=sys.stderr)


def mach_objects(directory):
    objects = []
    for path in directory.rglob("*"):
        if path.is_file() and not path.is_symlink():
            with path.open("rb") as file:
                if file.read(4) in MAGIC:
                    objects.append(path)
    return sorted(objects, key=lambda path: (-len(path.parts), str(path)))


def verify_dependencies(directory):
    def check(value, path):
        if value.startswith(("/System/Library/", "/usr/lib/", "@loader_path/",
                             "@executable_path/", "@rpath/")) or value in ("@loader_path", "@executable_path"):
            return
        raise RuntimeError(f"External native dependency in {path.relative_to(directory)}: {value}")

    for path in mach_objects(directory):
        commands = run("otool", "-l", path)
        for match in re.finditer(
                r"cmd LC_(?:LOAD_DYLIB|LOAD_WEAK_DYLIB|REEXPORT_DYLIB|LOAD_UPWARD_DYLIB)\s+"
                r"cmdsize \d+\s+name (.+?) \(offset \d+\)", commands):
            check(match.group(1), path)
        for match in re.finditer(r"cmd LC_RPATH\s+cmdsize \d+\s+path (.+?) \(offset \d+\)", commands):
            check(match.group(1), path)


def verify(path, team, deep=False):
    options = ["--deep"] if deep else []
    run("codesign", "--verify", "--strict", *options, path)
    metadata = run("codesign", "--display", "--verbose=4", path)
    if f"TeamIdentifier={team}\n" not in metadata:
        raise RuntimeError(f"Unexpected signing team: {path.name}")
    if "(runtime)" not in metadata or "Timestamp=" not in metadata:
        raise RuntimeError(f"Missing hardened runtime or secure timestamp: {path.name}")


def nested_apps(directory, options, team):
    for app in sorted(directory.rglob("*.app"), key=lambda path: -len(path.parts)):
        if app.is_dir() and not app.is_symlink():
            run("codesign", *options, "--preserve-metadata=entitlements", app)
            verify(app, team, deep=True)


def sign(app, host, target, identity, team, keychain=None):
    if app is None and host is None:
        raise RuntimeError("Select an application or Host package")
    if app is not None and not app.is_dir():
        raise RuntimeError("Expected a Sailry application package")
    if host is not None and not (host / "sailry-host").is_file():
        raise RuntimeError("Expected a Host executable")
    with keychain_scope(keychain, identity):
        options = ["--force", "--sign", identity, "--timestamp", "--options", "runtime"]
        if keychain:
            options.extend(["--keychain", str(keychain)])
        if app is not None:
            python_runtime.unlock(app / "Contents/MacOS")
            objects = mach_objects(app)
            if not objects:
                raise RuntimeError("The application contains no Mach-O code")
            for path in objects:
                metadata = ["--preserve-metadata=entitlements"] if path.is_relative_to(app / "Contents/Helpers") else []
                run("codesign", *options, *metadata, *python_runtime.signing_options(path, app / "Contents/MacOS"), path)
                verify(path, team)
            nested_apps(app, options, team)
            ocu_helper.finalize(app / "Contents/Helpers")
            llama_runtime.finalize(target, app / "Contents/MacOS")
            runtime.finalize(target, app / "Contents/MacOS", speech=True)
            python_runtime.finalize(target, app / "Contents/MacOS")
            run("codesign", *options, "--entitlements", ROOT / "apps/desktop/macos/entitlements.plist", app)
            verify(app, team, deep=True)
        if host is not None:
            # Host is a separate asset, never a deployment payload inside Desktop.
            python_runtime.unlock(host)
            for path in mach_objects(host):
                metadata = ["--preserve-metadata=entitlements"] if path.is_relative_to(host / "helpers") else []
                run("codesign", *options, *metadata, *python_runtime.signing_options(path, host), path)
                verify(path, team)
            nested_apps(host, options, team)
            ocu_helper.finalize(host / "helpers")
            llama_runtime.finalize(target, host)
            runtime.finalize(target, host, speech=False)
            python_runtime.finalize(target, host)
        print("Developer ID signing verified")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path)
    parser.add_argument("--host", type=Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--verify-dependencies", action="store_true")
    parser.add_argument("--keychain", type=Path, default=os.environ.get("APPLE_SIGNING_KEYCHAIN"))
    args = parser.parse_args()
    if args.app is None and args.host is None:
        parser.error("Select --app, --host or both")
    if args.verify_dependencies:
        for directory in (args.app, args.host):
            if directory is not None:
                verify_dependencies(directory)
        return
    identity = os.environ.get("APPLE_SIGNING_IDENTITY")
    team = os.environ.get("APPLE_TEAM_ID")
    if not identity or not team:
        parser.error("APPLE_SIGNING_IDENTITY and APPLE_TEAM_ID are required")
    sign(args.app, args.host, args.target, identity, team, args.keychain)


if __name__ == "__main__":
    main()
