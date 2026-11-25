import json
import re
import subprocess
import sys


MAX_RELEASE_PAGES = 10
MAX_PAGE_BYTES = 8 * 1024 * 1024
PAGE_SIZE = 100


repository, archive, checksum, current = sys.argv[1:]
number = r"(0|[1-9][0-9]*)"
version_pattern = rf"{number}\.{number}\.{number}"
current_match = re.fullmatch(
    rf"Sailry Host {version_pattern}(?:-([0-9A-Za-z.-]+))?", current
)
if current_match is None:
    sys.exit("Installed Host has an invalid release version")
current_version = tuple(map(int, current_match.groups()[:3]))
selected = None
selected_tag = None
try:
    for page in range(1, MAX_RELEASE_PAGES + 1):
        with subprocess.Popen(
            [
                "curl", "--fail", "--silent", "--show-error", "--location",
                "--proto", "=https", "--tlsv1.2",
                f"https://api.github.com/repos/{repository.removeprefix('https://github.com/')}/releases?per_page={PAGE_SIZE}&page={page}",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
        ) as response:
            try:
                body = response.stdout.read(MAX_PAGE_BYTES + 1)
                if len(body) > MAX_PAGE_BYTES:
                    raise ValueError("release metadata exceeds its 8 MiB limit")
                status = response.wait()
                if status != 0:
                    raise subprocess.CalledProcessError(status, response.args)
            finally:
                if response.poll() is None:
                    response.kill()
        releases = json.loads(body)
        if not isinstance(releases, list):
            raise ValueError("release response is not a list")
        for release in releases:
            if not isinstance(release, dict):
                raise ValueError("release response contains an invalid entry")
            if release.get("draft") is not False or release.get("prerelease") is not False:
                continue
            tag = release.get("tag_name")
            match = re.fullmatch(rf"v{version_pattern}", tag) if isinstance(tag, str) else None
            assets = release.get("assets")
            if match is None or not isinstance(assets, list):
                continue
            names = {
                asset.get("name") for asset in assets
                if isinstance(asset, dict) and asset.get("state") == "uploaded"
            }
            if not {archive, checksum}.issubset(names):
                continue
            version = tuple(map(int, match.groups()))
            if selected is None or version > selected:
                selected, selected_tag = version, tag
        if len(releases) < PAGE_SIZE:
            break
    else:
        raise ValueError("release discovery exceeds its page limit")
except (OSError, ValueError, subprocess.CalledProcessError) as error:
    sys.exit(f"Cannot select a Host release: {error}")

if selected is None:
    sys.exit("No stable Host release is available for this target")
if selected < current_version:
    sys.exit(f"Refusing to downgrade {current} to {selected_tag}")
if selected > current_version or current_match.group(4) is not None:
    print(selected_tag[1:])
