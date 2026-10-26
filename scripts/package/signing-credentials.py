"""Import and remove ephemeral macOS distribution credentials."""

import base64
import os
from pathlib import Path
import subprocess
import sys
import uuid


def install():
    names = ('APPLE_CERTIFICATE_P12', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_API_PRIVATE_KEY')
    if not all(os.environ.get(name) for name in names):
        raise SystemExit('The macOS signing secrets are not configured')
    stage = Path(os.environ['RUNNER_TEMP'])
    certificate = stage / 'sailry-signing.p12'
    key = stage / 'sailry-notary.p8'
    certificate.write_bytes(base64.b64decode(os.environ['APPLE_CERTIFICATE_P12'], validate=True))
    key.write_text(os.environ['APPLE_API_PRIVATE_KEY'])
    certificate.chmod(0o600)
    key.chmod(0o600)
    keychain = stage / 'sailry-signing.keychain-db'
    password = uuid.uuid4().hex
    def security(*args):
        result = subprocess.run(['security', *map(str, args)], capture_output=True, text=True)
        if result.returncode:
            raise SystemExit('Temporary signing keychain setup failed')
    security('create-keychain', '-p', password, keychain)
    security('set-keychain-settings', '-lut', '21600', keychain)
    security('unlock-keychain', '-p', password, keychain)
    security('import', certificate, '-k', keychain, '-P', os.environ['APPLE_CERTIFICATE_PASSWORD'], '-T', '/usr/bin/codesign')
    security('set-key-partition-list', '-S', 'apple-tool:,apple:,codesign:', '-s', '-k', password, keychain)
    with open(os.environ['GITHUB_ENV'], 'a') as file:
        file.write(f'APPLE_SIGNING_KEYCHAIN={keychain}\nAPPLE_API_KEY_PATH={key}\n')
    certificate.unlink()


def cleanup():
    stage = Path(os.environ['RUNNER_TEMP'])
    keychain = stage / 'sailry-signing.keychain-db'
    if keychain.exists():
        subprocess.run(['security', 'delete-keychain', str(keychain)], check=True)
    for name in ('sailry-signing.p12', 'sailry-notary.p8'):
        (stage / name).unlink(missing_ok=True)


if __name__ == "__main__":
    if sys.argv[1:] == ["install"]:
        install()
    elif sys.argv[1:] == ["cleanup"]:
        cleanup()
    else:
        raise SystemExit("Usage: signing-credentials.py <install|cleanup>")
