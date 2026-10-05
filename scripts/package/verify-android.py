"""Verify APK identity, release metadata, native libraries and signing certificates."""

import argparse
import os
from pathlib import Path
import re
import subprocess
import zipfile


LIBRARIES = ("libapp.so", "libflutter.so", "libsailry_mobile_bridge.so",
             "libsherpa-onnx-c-api.so", "libonnxruntime.so")


def metadata(text, version, code, application):
    package = re.search(r"^package: (.+)$", text, re.MULTILINE)
    fields = dict(re.findall(r"(\w+)='([^']*)'", package.group(1))) if package else {}
    expected = {"name": application, "versionName": version, "versionCode": str(code)}
    if any(fields.get(key) != value for key, value in expected.items()):
        raise ValueError("APK application identity or release version does not match")
    if not re.search(r"^minSdkVersion:'24'$", text, re.MULTILINE):
        raise ValueError("APK must retain Android 7.0 as its minimum version")
    if not re.search(r"^targetSdkVersion:'36'$", text, re.MULTILINE):
        raise ValueError("APK must use the pinned Android target SDK")
    if re.search(r"^application-debuggable", text, re.MULTILINE):
        raise ValueError("Distribution APK must not be debuggable")


def certificate(text, expected):
    digests = re.findall(r"^Signer #[0-9]+ certificate SHA-256 digest: ([0-9a-fA-F]{64})$", text, re.MULTILINE)
    if not digests:
        raise ValueError("APK signing certificate is missing")
    if expected is not None and [value.lower() for value in digests] != [expected.lower()]:
        raise ValueError("APK is not signed with the configured distribution certificate")


def native(apk):
    with zipfile.ZipFile(apk) as archive:
        files = {item.filename: item.file_size for item in archive.infolist()}
    for library in LIBRARIES:
        if not files.get(f"lib/arm64-v8a/{library}"):
            raise ValueError(f"APK is missing {library}")
    if any(name.startswith("lib/") and not name.startswith("lib/arm64-v8a/") for name in files):
        raise ValueError("APK must contain only ARM64 native libraries")


def run(program, *args):
    return subprocess.check_output([str(program), *map(str, args)], text=True)


def verify(apk, version, code, application, tools, expected=None):
    native(apk)
    metadata(run(tools / "aapt2", "dump", "badging", apk), version, code, application)
    certificate(run(tools / "apksigner", "verify", "--print-certs", apk), expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("apk", type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--code", type=int, required=True)
    parser.add_argument("--application", default="com.sailry.sailry_mobile")
    parser.add_argument("--certificate")
    parser.add_argument("--tools", type=Path)
    args = parser.parse_args()
    if args.certificate is not None and not re.fullmatch(r"[0-9a-fA-F]{64}", args.certificate):
        raise ValueError("Expected a SHA-256 distribution certificate fingerprint")
    sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    if args.tools is None and not sdk:
        raise ValueError("Android SDK tools are unavailable")
    tools = args.tools or Path(sdk) / "build-tools/36.0.0"
    verify(args.apk, args.version, args.code, args.application, tools, args.certificate)
    print(f"Verified {args.version}+{args.code}: ARM64 APK, Android 7.0+, application identity and signing certificate")


if __name__ == "__main__":
    main()
