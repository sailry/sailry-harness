"""Stage isolated iOS distribution signing and verify exported App Store packages."""

import argparse
import base64
import datetime
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import secrets
import shlex
import shutil
import subprocess
import tempfile
import uuid

import versions


BUNDLE = "com.sailry.sailryMobile"
PROJECT = versions.ROOT / "apps/mobile/ios/Runner.xcodeproj/project.pbxproj"


def stage():
    return Path(os.environ["RUNNER_TEMP"]) / "sailry-ios"


def profiles():
    return Path.home() / "Library/Developer/Xcode/UserData/Provisioning Profiles"


def run(program, *arguments):
    result = subprocess.run([program, *map(str, arguments)], capture_output=True)
    if result.returncode:
        # Do not include command arguments: security receives the export password.
        raise RuntimeError(f"The iOS distribution {Path(program).name} command failed")
    return result.stdout


def private_file(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "wb") as output:
        output.write(data)
    path.chmod(0o600)


def profile_details(profile, team, now=None):
    expiry = profile.get("ExpirationDate")
    now = now or datetime.datetime.now(datetime.timezone.utc)
    if not isinstance(expiry, datetime.datetime):
        raise ValueError("The iOS provisioning profile has no expiration date")
    if expiry.tzinfo is None:
        expiry = expiry.replace(tzinfo=datetime.timezone.utc)
    entitlements = profile.get("Entitlements", {})
    if (expiry <= now or team not in profile.get("TeamIdentifier", [])
            or entitlements.get("application-identifier") != f"{team}.{BUNDLE}"
            or entitlements.get("com.apple.developer.team-identifier") != team):
        raise ValueError("The iOS provisioning profile is expired or belongs to another app or team")
    if (entitlements.get("get-task-allow") is not False or "ProvisionedDevices" in profile
            or profile.get("ProvisionsAllDevices") is True or "iOS" not in profile.get("Platform", [])):
        raise ValueError("iOS distribution requires an App Store provisioning profile")
    identifier = profile.get("UUID")
    try:
        if not isinstance(identifier, str) or str(uuid.UUID(identifier)) != identifier.lower():
            raise ValueError
    except (ValueError, AttributeError):
        raise ValueError("The iOS provisioning profile has an invalid UUID") from None
    certificates = profile.get("DeveloperCertificates", [])
    if not certificates or any(not isinstance(value, bytes) or not value for value in certificates):
        raise ValueError("The iOS provisioning profile has no signing certificates")
    return identifier, {hashlib.sha1(value).hexdigest().upper() for value in certificates}


def configure_project(project, team, identity, identifier):
    objects = project["objects"]
    targets = [value for value in objects.values()
               if value.get("isa") == "PBXNativeTarget" and value.get("productType") == "com.apple.product-type.application"]
    if len(targets) != 1 or targets[0].get("name") != "Runner":
        raise ValueError("The iOS project must contain the Sailry Runner application")
    configurations = objects[targets[0]["buildConfigurationList"]]["buildConfigurations"]
    releases = [objects[key] for key in configurations if objects[key].get("name") == "Release"]
    if len(releases) != 1 or releases[0]["buildSettings"].get("PRODUCT_BUNDLE_IDENTIFIER") != BUNDLE:
        raise ValueError("The iOS Release target has a different application identifier")
    releases[0]["buildSettings"].update({
        "CODE_SIGN_STYLE": "Manual", "CODE_SIGN_IDENTITY": identity,
        "CODE_SIGN_IDENTITY[sdk=iphoneos*]": identity, "DEVELOPMENT_TEAM": team,
        "PROVISIONING_PROFILE_SPECIFIER": identifier,
    })


def install():
    names = ("IOS_CERTIFICATE_P12", "IOS_CERTIFICATE_PASSWORD", "IOS_PROVISIONING_PROFILE", "APPLE_TEAM_ID")
    if not all(os.environ.get(name) for name in names):
        raise ValueError("The iOS distribution signing secrets are not configured")
    directory = stage()
    directory.mkdir(mode=0o700)
    try:
        certificate = directory / "distribution.p12"
        supplied_profile = directory / "distribution.mobileprovision"
        private_file(certificate, base64.b64decode(os.environ["IOS_CERTIFICATE_P12"], validate=True))
        private_file(supplied_profile, base64.b64decode(os.environ["IOS_PROVISIONING_PROFILE"], validate=True))
        team = os.environ["APPLE_TEAM_ID"]
        profile = plistlib.loads(run("security", "cms", "-D", "-i", supplied_profile))
        identifier, fingerprints = profile_details(profile, team)
        keychain = directory / "distribution.keychain-db"
        password = secrets.token_urlsafe(32)
        original = shlex.split(run("security", "list-keychains", "-d", "user").decode())
        private_file(directory / "keychains.json", json.dumps(original).encode())
        run("security", "create-keychain", "-p", password, keychain)
        run("security", "set-keychain-settings", "-lut", "21600", keychain)
        run("security", "unlock-keychain", "-p", password, keychain)
        run("security", "import", certificate, "-k", keychain, "-P", os.environ["IOS_CERTIFICATE_PASSWORD"],
            "-T", "/usr/bin/codesign")
        run("security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s", "-k", password, keychain)
        run("security", "list-keychains", "-d", "user", "-s", keychain, *original)
        identities = re.findall(r'\b([0-9A-Fa-f]{40}) "Apple Distribution:[^"]+"',
                                run("security", "find-identity", "-v", "-p", "codesigning", keychain).decode())
        if len(identities) != 1 or identities[0].upper() not in fingerprints:
            raise ValueError("The iOS distribution identity does not match the provisioning profile")
        identity = identities[0].upper()
        profiles().mkdir(parents=True, exist_ok=True)
        installed = profiles() / f"{identifier}.mobileprovision"
        if installed.exists():
            if installed.read_bytes() != supplied_profile.read_bytes():
                raise ValueError("A different iOS provisioning profile already uses this UUID")
        else:
            private_file(installed, supplied_profile.read_bytes())
            private_file(directory / "installed-profile", identifier.encode())
        project = plistlib.loads(run("plutil", "-convert", "xml1", "-o", "-", PROJECT))
        configure_project(project, team, identity, identifier)
        private_file(directory / "project.pbxproj", PROJECT.read_bytes())
        PROJECT.write_bytes(plistlib.dumps(project, fmt=plistlib.FMT_XML, sort_keys=False))
        options = directory / "ExportOptions.plist"
        private_file(options, plistlib.dumps({
            "method": "app-store-connect", "destination": "export", "teamID": team,
            "signingStyle": "manual", "signingCertificate": identity,
            "provisioningProfiles": {BUNDLE: identifier}, "manageAppVersionAndBuildNumber": False,
            "stripSwiftSymbols": True, "uploadSymbols": True,
        }))
        with Path(os.environ["GITHUB_ENV"]).open("a") as output:
            output.write(f"IOS_SIGNING_IDENTITY={identity}\nIOS_PROFILE_UUID={identifier}\nIOS_EXPORT_OPTIONS={options}\n")
        certificate.unlink()
        print("Installed the matching iOS Distribution identity and App Store profile")
    except BaseException:
        cleanup()
        raise


def cleanup():
    directory = stage()
    if not directory.exists():
        return
    failed = False
    if (directory / "project.pbxproj").exists():
        PROJECT.write_bytes((directory / "project.pbxproj").read_bytes())
    if (directory / "installed-profile").exists():
        identifier = (directory / "installed-profile").read_text()
        if str(uuid.UUID(identifier)) != identifier.lower():
            raise ValueError("The staged iOS profile UUID is invalid")
        (profiles() / f"{identifier}.mobileprovision").unlink(missing_ok=True)
    if (directory / "keychains.json").exists():
        original = json.loads((directory / "keychains.json").read_text())
        result = subprocess.run(["security", "list-keychains", "-d", "user", "-s", *original], capture_output=True)
        failed |= result.returncode != 0
    keychain = directory / "distribution.keychain-db"
    if keychain.exists():
        result = subprocess.run(["security", "delete-keychain", str(keychain)], capture_output=True)
        failed |= result.returncode != 0
    shutil.rmtree(directory)
    if failed:
        raise RuntimeError("Temporary iOS signing cleanup failed")


def ipa(path):
    candidates = list(path.glob("*.ipa")) if path.is_dir() else [path]
    if len(candidates) != 1 or not candidates[0].is_file() or candidates[0].suffix != ".ipa":
        raise ValueError("Expected exactly one exported iOS IPA")
    return candidates[0].resolve()


def verify(path):
    path = ipa(path)
    version, code = versions.product(versions.ROOT)
    with tempfile.TemporaryDirectory(dir=stage()) as temporary:
        directory = Path(temporary)
        run("ditto", "-x", "-k", path, directory)
        applications = list((directory / "Payload").glob("*.app"))
        if len(applications) != 1:
            raise ValueError("The IPA must contain exactly one application")
        app = applications[0]
        information = plistlib.loads((app / "Info.plist").read_bytes())
        expected = {"CFBundleIdentifier": BUNDLE, "CFBundleShortVersionString": versions.apple(version),
                    "CFBundleVersion": str(code)}
        if any(information.get(key) != value for key, value in expected.items()):
            raise ValueError("The IPA application identity or build version does not match")
        run("codesign", "--verify", "--deep", "--strict", app)
        prefix = directory / "certificate"
        run("codesign", "--display", f"--extract-certificates={prefix}", app)
        identity = hashlib.sha1(Path(f"{prefix}0").read_bytes()).hexdigest().upper()
        if identity != os.environ["IOS_SIGNING_IDENTITY"]:
            raise ValueError("The IPA does not use the captured Distribution identity")
        profile = plistlib.loads(run("security", "cms", "-D", "-i", app / "embedded.mobileprovision"))
        identifier, fingerprints = profile_details(profile, os.environ["APPLE_TEAM_ID"])
        if identifier != os.environ["IOS_PROFILE_UUID"] or identity not in fingerprints:
            raise ValueError("The IPA does not use the captured App Store profile")
    with Path(os.environ["GITHUB_ENV"]).open("a") as output:
        output.write(f"IOS_IPA_PATH={path}\n")
    print(f"Verified iOS {versions.apple(version)} ({code}) application identity and distribution signing")


def upload(path):
    names = ("APPLE_API_PRIVATE_KEY", "APPLE_API_KEY_ID", "APPLE_API_ISSUER_ID")
    if not all(os.environ.get(name) for name in names):
        raise ValueError("The App Store Connect upload authorization is not configured")
    identifier = os.environ["APPLE_API_KEY_ID"]
    if not re.fullmatch(r"[A-Za-z0-9]+", identifier):
        raise ValueError("The App Store Connect API key ID is invalid")
    directory = stage() / "private_keys"
    directory.mkdir(mode=0o700)
    key = directory / f"AuthKey_{identifier}.p8"
    private_file(key, os.environ["APPLE_API_PRIVATE_KEY"].encode())
    environment = {**os.environ, "API_PRIVATE_KEYS_DIR": str(directory)}
    environment.pop("APPLE_API_PRIVATE_KEY", None)
    try:
        help_result = subprocess.run(["xcrun", "altool", "--help"], capture_output=True, text=True)
        help_text = help_result.stdout + help_result.stderr
        # Xcode's bundled altool exposes either the current package command or the
        # documented upload-app spelling. Select its supported CLI explicitly.
        if "--upload-package" in help_text:
            command = ["xcrun", "altool", "--upload-package", str(ipa(path)), "--wait",
                       "--api-key", identifier, "--api-issuer", os.environ["APPLE_API_ISSUER_ID"]]
        else:
            command = ["xcrun", "altool", "--upload-app", "-f", str(ipa(path)), "-t", "ios",
                       "--apiKey", identifier, "--apiIssuer", os.environ["APPLE_API_ISSUER_ID"]]
        result = subprocess.run(command, env=environment)
        if result.returncode:
            raise RuntimeError("The App Store Connect upload failed; inspect its delivery status before retrying")
    finally:
        key.unlink(missing_ok=True)
        directory.rmdir()
    print("Apple accepted the upload; TestFlight processing and export compliance remain in App Store Connect")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("install", "cleanup", "verify", "upload"))
    parser.add_argument("ipa", type=Path, nargs="?")
    args = parser.parse_args()
    if args.command in {"verify", "upload"}:
        if args.ipa is None:
            parser.error("An exported IPA path is required")
        (verify if args.command == "verify" else upload)(args.ipa)
    else:
        (install if args.command == "install" else cleanup)()


if __name__ == "__main__":
    main()
