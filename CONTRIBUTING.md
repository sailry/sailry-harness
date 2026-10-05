# Contributing

Read [ARCHITECTURE.md](ARCHITECTURE.md) and [AGENTS.md](AGENTS.md) before changing
ownership, execution or UI boundaries. Work from the committed implementation,
preserve unrelated local changes and keep each task cohesive and reviewable.

## Native setup

Use Rust 1.98.0, Git, Python 3.12 or newer, Zig 0.16.0, `just`, and your platform's
native build tools. The Ghostty binding builds native code with Zig. macOS Desktop
requires Xcode Command Line Tools. Use committed lockfiles and `--locked` for Rust
commands; do not replace pinned dependencies with absolute local paths.

Clone with `git clone --recurse-submodules` or run
`git submodule update --init --recursive` before building. Official package changes
are committed and pushed in `sailry-plugins`; then commit the reviewed `plugins/`
gitlink update in this repository. Never copy plugin source into a second owner.

From the repository root:

```sh
git submodule update --init --recursive
cargo build --locked -p sailry-host
just desktop
```

`just desktop` prepares the Office runtime and builds an application bundle on
macOS. Its default profile is `.runtime/desktop`. Office preparation requires `uv`
0.10.9 and creates a pinned, relocatable Python runtime without installing packages
into your user Python environment. An available Apple Development signing identity
keeps the development app's OS permission identity stable; otherwise signing is ad hoc.

```sh
just desktop /absolute/private/desktop-profile
just host /absolute/private/host-profile
just preview
```

Direct Desktop and Host binaries default to `~/.sailry`; one running Node may own
a profile. Profiles contain Node storage, protected credentials, plugin packages,
plugin data, and attachments. Projects remain at their registered paths. Remote
execution data stays on the execution Node. Incompatible development profiles are
reported without conversion, reset, or deletion.

Host is a long-running headless service, not an interactive agent CLI. Desktop or
Mobile controls it through the shared protocol. See [Host deployment](ARCHITECTURE.md#deployment-and-operations)
and the [pairing service guide](services/pairing-relay/README.md).

`just preview` uses identified example data without starting a Node or opening a
business profile. It cannot be combined with profile or relay options.

## Checks

Plugin JavaScript tests require Node.js 22 or newer; source checks use Python's
standard library. Pairing-service dependencies and checks have their own
[guide](services/pairing-relay/README.md#run-and-verify).

```sh
just check          # Rust formatting and Clippy with warnings denied
just test-source    # Public guides, publication boundaries and source fixtures
just test-plugins   # Deterministic package JavaScript tests
just test-backend   # Protocol, Link, Client, Node and Host tests
just test-ui        # Desktop behavior and Kit interaction tests
```

Select checks from affected behavior and dependency boundaries. Reuse passing
results while their inputs remain unchanged. Explain which tests ran and any
coverage gap; do not describe a fixture or compile check as live acceptance.

## Test conventions

The conventions follow the [pinned ADK contributor guide](https://github.com/dux-web/adk-rust/blob/b3e360bbf49ca39d49cccba182c0f7cb4d251970/CONTRIBUTING.md),
adapted to Sailry's ownership and local/remote boundaries.

- Put small Rust unit tests in nearby `#[cfg(test)]` modules. Put cross-module
  integration tests and substantial fixtures in focused test modules or the
  owning crate's `tests/` directory.
- Group tests by behavior. Use concise names distinguishing the scenario,
  rather than repeating the full crate and feature name.
- Cover successful and meaningful failure paths for changed public behavior.
  A regression test must fail without its fix. Compare complete structured
  outcomes when stable; proving only that code did not panic is insufficient.
- Keep ordinary tests deterministic and self-contained. Use isolated temporary
  profiles, injected clocks or bounded waits, and clean up spawned workers.
  Do not depend on ignored output directories, user projects or private profiles.
- Test conflicts, reconnect and original-request recovery where a changed command
  can cause side effects. Exercise shared behavior through local and remote paths,
  not merely a mocked transport.
- Make external prerequisites explicit. Rust live-service/device tests use
  `#[ignore = "requires …"]` with a focused opt-in command. Do not silently return
  success when credentials, services or devices are missing. An explicitly gated
  subprocess fixture is not an independently passing acceptance test.
- Keep test credentials synthetic, use loopback fixtures by default and never
  log secrets. Preserve intentional Unicode and malformed-input fixtures.
- Use property tests for meaningful invariants when existing test tools fit.
  Do not add a framework or arbitrary coverage targets to inflate test counts.
- Keep source and developer diagnostics in English. Product copy belongs in
  i18n resources; test operation feedback through the toast boundary.

Real-service, device, OS interaction and performance acceptance must state their
actual prerequisites and results separately. Screenshots are not a default gate.

## Continuous integration

[GitHub Actions](.github/workflows/ci.yml) checks concise names, workflow and script syntax,
public-source fixtures, packaged JavaScript tests and the pairing service.
Both macOS architectures run strict Rust checks, backend local/remote integration,
desktop Kit interactions and binary builds with an explicit Xcode SDK.
Mobile checks include Flutter analysis and widget tests, native Dart FFI contracts
and Flutter against an isolated real remote Node. Android builds an ARM64 release
APK and checks its native libraries; iOS builds an unsigned application. Both
mobile builds check the Rust dependency boundary. Actions and toolchains are
pinned; Rust and Flutter use committed lockfiles. `Checks passed` requires every
job to succeed, including both Desktop architectures.

CI does not supply model credentials, production profiles or OS permissions.
Live-service and device tests stay opt-in. The ordinary Flutter suite skips its
bootstrap-dependent native fixture, which the explicit remote-Node step runs.
Database fixtures start isolated PostgreSQL/MySQL servers, including native
backend and Dart FFI coverage. The FFI step excludes only the MCP subprocess
entry point, which its parent fixtures launch themselves. CI APKs use disposable
debug certificates, not distribution keys. Native builds and a macOS host-side
Flutter run do not establish iOS or Android device acceptance.

## macOS releases

[The release workflow](.github/workflows/release.yml) runs only when a `v*` tag is
pushed. The tag must match `apps/desktop/Cargo.toml`, such as `v0.1.0` for version
`0.1.0`; branch pushes, pull requests and manual dispatch do not build releases.
Publish a tag only when you intend to make a formal release. iOS and App Store
publication are not part of this workflow.

Configure these repository secrets through GitHub's encrypted secret storage:

- `APPLE_CERTIFICATE_P12`: base64-encoded Developer ID Application certificate
  with its private key
- `APPLE_CERTIFICATE_PASSWORD`: the certificate export password
- `APPLE_API_PRIVATE_KEY`: the App Store Connect API private key contents

Configure repository variables `APPLE_SIGNING_IDENTITY`, `APPLE_TEAM_ID`,
`APPLE_API_KEY_ID` and `APPLE_API_ISSUER_ID`. The signing identity is the Developer
ID certificate's SHA-1 fingerprint. Keep credentials out of source, artifacts and
logs; the workflow imports them into a temporary keychain and removes its key
files after packaging. The Apple account must have accepted current agreements
and the API key must have notarization access.

Apple silicon and Intel runners build separate native Desktop/Host packages.
Each Desktop bundle includes both Linux Host architectures and pinned Office
runtimes. Linux link inputs are checksum-pinned in
[`linux-sysroot.lock.json`](scripts/package/linux-sysroot.lock.json); the build
does not install cross-compilation packages into the host system.

For local verification, build Desktop and the bundled Hosts, then package into a
fresh `dist/` directory:

```sh
cargo build --locked --release -p sailry-desktop -p sailry-host
bash scripts/build-hosts.sh release
SAILRY_SIGNING=developer-id SAILRY_NOTARIZE=1 bash scripts/package-macos.sh release
```

The local command requires the signing variables above plus `APPLE_API_KEY_PATH`,
pointing to a private API key file. `APPLE_SIGNING_KEYCHAIN` optionally selects a
keychain containing the imported identity. Never put those private values in a
checked-in environment file. Without distribution options, the packaging command
creates an ad hoc development package, not a notarized release.

Distribution signs nested native code inside out with hardened runtime and secure
timestamps, submits Desktop and Host for notarization, staples Desktop's ticket,
and verifies Gatekeeper before creating archives. Standalone executables cannot
carry stapled tickets. Both native builds must succeed before GitHub publishes
their ZIP/TAR packages and checksums.

Release notes are generated from the immutable tag's Git commits since the
previous version tag. The same entry is prepended to [`CHANGELOG.md`](CHANGELOG.md)
on the default branch automatically after publication; existing entries are
preserved. Publishing needs `contents: write` and permission to update that file
on the default branch. A failed changelog update is reported as a failed workflow,
even if the release was already published; inspect that outcome before retrying.

## Public source and history

Internal `docs/`, plans, local outputs, profiles and credentials are ignored and
must not be tracked. Public guides, `AGENTS.md`, package Skills, fixtures, licenses
and lockfiles remain source. Ignore rules alone do not remove old Git history.

Keep private development history private. After committing the reviewed public
tree, create a new local repository from that one committed snapshot:

```sh
python3 scripts/public-source.py --export /absolute/new/public-sailry
```

The destination must not exist. The exporter checks the committed snapshot,
copies tracked source only and creates one new root commit. It does not copy
private history, uncommitted changes or ignored files, and does not set a remote
or push. Review the new repository before selecting a public remote. This is a
publication boundary check, not a complete secret or license audit.

## Licensing

Sailry-owned contributions use [Apache-2.0](LICENSE). Preserve third-party
copyright, source provenance and license notices. The root license does not
relicense vendored source, imported shell scripts, fonts, icons or other resources.
Include applicable notices when packaging or redistributing those materials.
