#!/usr/bin/env python3
"""Notarize signed Desktop and Host packages; staple Desktop before distribution."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def run(*args):
    result = subprocess.run([str(arg) for arg in args], capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or
                           f"{args[0]} failed with exit code {result.returncode}")
    return result.stdout


def submit(archive, credentials):
    output = run("xcrun", "notarytool", "submit", archive, *credentials,
                 "--wait", "--timeout", "45m", "--output-format", "json")
    result = json.loads(output)
    if result.get("status") != "Accepted":
        submission = result.get("id")
        if submission:
            log = json.loads(run("xcrun", "notarytool", "log", submission, *credentials))
            for issue in log.get("issues") or []:
                print(f"Notarization issue: {issue.get('path', '')}: {issue.get('message', '')}")
        raise RuntimeError(f"Notarization did not succeed: {result.get('status', 'unknown')}")
    print(f"Notarization accepted: {archive.name} ({result['id']})")


def notarize(app, host, credentials):
    with tempfile.TemporaryDirectory(prefix="sailry-notary-") as directory:
        stage = Path(directory)
        for item in (app, host):
            archive = stage / f"{item.name}.zip"
            run("ditto", "-c", "-k", "--keepParent", item, archive)
            submit(archive, credentials)
    # Tickets cannot be stapled to a ZIP or standalone executable. The final
    # Desktop ZIP is created by package-macos.sh only after stapling its app.
    run("xcrun", "stapler", "staple", app)
    run("xcrun", "stapler", "validate", app)
    run("codesign", "--verify", "--deep", "--strict", app)
    run("spctl", "--assess", "--type", "execute", "--verbose=2", app)
    print("Desktop ticket and Gatekeeper assessment verified")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--host", type=Path, required=True)
    args = parser.parse_args()
    names = ("APPLE_API_KEY_PATH", "APPLE_API_KEY_ID", "APPLE_API_ISSUER_ID")
    values = [os.environ.get(name) for name in names]
    if not all(values):
        parser.error("APPLE_API_KEY_PATH, APPLE_API_KEY_ID and APPLE_API_ISSUER_ID are required")
    if not Path(values[0]).is_file():
        parser.error("The notarization API key file does not exist")
    notarize(args.app, args.host, ["--key", values[0], "--key-id", values[1], "--issuer", values[2]])


if __name__ == "__main__":
    main()
