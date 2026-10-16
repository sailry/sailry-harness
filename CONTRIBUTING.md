# Contributing

Read [ARCHITECTURE.md](ARCHITECTURE.md) and [AGENTS.md](AGENTS.md) before changing
ownership, execution or UI boundaries. Work from the committed implementation,
preserve unrelated local changes and keep each task cohesive and reviewable.

## Native setup

Use Rust 1.98.0, Git, Python 3.12 or newer, Zig 0.16.0, `just`, and your platform's
native build tools. The Ghostty binding builds native code with Zig. macOS Desktop
requires Xcode Command Line Tools. Use committed lockfiles and `--locked` for Rust
commands; do not replace pinned dependencies with absolute local paths.
Linux Desktop also requires Speech Dispatcher headers and libraries plus libclang
(`libspeechd-dev` and `libclang-dev` on Debian or Ubuntu). Host does not require
these desktop speech dependencies.
Python recipes select an available Python 3.12 or newer without changing `PATH`.
Set `SAILRY_PYTHON` to an explicit executable when needed; an unsupported explicit
interpreter fails rather than silently selecting another one.

Clone with `git clone --recurse-submodules` or run
`git submodule update --init --recursive` before building. Official plugins live
in `sailry-plugins` under `plugins/`; official themes live in `sailry-themes` under
`themes/`. Commit each package repository separately, then commit its reviewed
Gitlink here. When publishing, push the referenced package commit first. Never
copy package source into a second owner. Package maintenance and generated index
checks are documented in each submodule's README and AGENTS files.

From the repository root:

```sh
git submodule update --init --recursive
cargo build --locked -p sailry-host
just desktop
```

`just desktop` builds an application bundle on macOS with the pinned Python
runtime. Plugin dependencies are prepared lazily. It uses the same `~/.sailry` profile as the installed
app. `SAILRY_DESKTOP_DATA_DIR` or an explicit recipe argument selects another
profile; the argument takes precedence.
An available Apple Development signing identity
keeps the development app's OS permission identity stable; otherwise signing is ad hoc.

```sh
just desktop /absolute/private/desktop-profile
just host /absolute/private/host-profile
just preview
```

Direct Desktop and Host binaries default to `~/.sailry`; one running Node may own
a profile. Close the installed app before running `just desktop` with that profile.
Profiles contain Node configuration and plaintext credentials, plugin packages,
plugin data, and attachments. Credentials stay out of public events, logs and UI
caches. Projects remain at their registered paths. Remote
execution data stays on the execution Node. Startup restores missing schema
objects and storage defaults after validating an isolated snapshot and saving a
private backup. Incompatible definitions fail without resetting or deleting data.

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
just test-ui        # Desktop logic and service-contract tests
```

`just test-ui` uses CI's nextest profile. Install the pinned runner with
`cargo install --locked cargo-nextest --version 0.9.143` if it is not available.

Select checks from affected behavior and dependency boundaries. Reuse passing
results while their inputs remain unchanged. Explain which tests ran and any
coverage gap; do not describe a fixture or compile check as live acceptance.
Keep Cargo's default incremental compilation enabled for local development;
the CI-only environment overrides are not local build settings. Run the affected
tests first, not the complete platform matrix for each edit.
When several contributors or agents share a checkout, designate one Cargo owner
per target directory. Use a separate worktree and `CARGO_TARGET_DIR` for independent
builds; do not launch competing builds against the same target or change its build
environment between them. Keep `PKG_CONFIG_PATH`, SDK, target and Rust flags stable
for a build lane. Do not delete another lane's artifacts to resolve a lock wait.

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
- Cover data mappings exhaustively in unit tests; use representative integration
  workflows for each distinct behavior instead of repeating identical setup for
  every preset. Preserve local and remote paths and meaningful failure cases.
- Inject offline catalogs and services in ordinary fixtures. External-service
  acceptance must be explicit and opt-in, not an incidental UI setup dependency.
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
- Use simulated-window, rendered-layout, mouse or keyboard interaction checks
  only as temporary development and debugging aids. Remove them afterward rather
  than committing permanent tests or adding CI, release or other standardized
  gates. GPUI Kit owns framework-control interaction coverage. Preserve static
  checks and ordinary logic, protocol, persistence and local/remote service
  integration; controller logic should be tested without mounting a window.

Real-service, device, OS interaction and performance acceptance must state their
actual prerequisites and results separately. Do not substitute automatic OS,
screenshot, pixel or end-to-end gates for removed UI interaction tests. Add or run
those checks only when explicitly requested.

## Continuous integration

[GitHub Actions](.github/workflows/ci.yml) runs automatically on pushes and pull
requests. Source checks, native tests and platform builds use the same workflow;
manual dispatch reruns it without changing the required coverage. A newer run for
the same ref cancels superseded work. Reuse verified results while their relevant
inputs remain unchanged.

Native validation runs strict Rust checks, backend local/remote integration,
Desktop logic and service-contract tests, and production-feature checks on Apple
silicon with an explicit Xcode SDK. Intel packaging and compilation run in the
release workflow.
Mobile checks include Flutter analysis and widget tests, native Dart FFI contracts
and Flutter against an isolated real remote Node. Android builds an ARM64 release
APK and checks its native libraries; iOS builds an unsigned application. Both
mobile builds check the Rust dependency boundary. Actions and toolchains are
pinned; Rust and Flutter use committed lockfiles. `Checks passed` requires every
job to succeed; skipped or cancelled jobs
do not establish acceptance. Signed release packaging builds exact version tags.

Node integration features share eight compiled suites instead of linking a
binary per feature. Keep feature files focused and register new files in the
appropriate suite; the source check rejects missing or duplicate registrations.
Rust caches retain compressed registry archives and Git databases; Cargo recreates
unpacked sources and checkouts. A single backend producer fetches the locked
workspace sources, shared by Checks and release jobs. Target directories are not
archived, to leave room within the repository's 10 GiB cache budget. Cargo
validates fingerprints and rebuilds changed inputs.
The pinned sccache compiler cache reuses Rust library compilation by content;
test executables still need linking. Main produces compiler objects; PRs read
them. SDKs, targets and profile flags remain part of compiler identity; release
and test objects are not interchangeable. Cache statistics and repository usage
are recorded in each run, with a warning at 9 GiB. GitHub evicts least recently
used entries at the repository limit; this budget cannot keep every platform and
profile hot simultaneously. `SCCACHE_CACHE_SIZE` does not cap remote storage.
Checks omit debug information and preserve the workspace's optimization settings,
including the GPUI hot-path overrides. Development and release profiles are unchanged.
Compilation and execution are separate steps; compilation timings are uploaded
for the backend and Desktop jobs. The pinned nextest runner executes each Desktop
and backend test in its own process without compiling a binary per test.
Desktop runs two tests concurrently;
failures are not retried. Hung tests fail after five 60-second timeout periods.
Unit tests precede production-feature checks; the lifecycle suite builds its own
required application executable. JUnit reports remain separate and include
individual test durations.
Checks uses three macOS jobs: lint with backend, Desktop, and Mobile with iOS.
Backend and Desktop share downloaded dependency sources, with backend as the
cache producer; Mobile reuses that source cache. Sccache caches eligible library
metadata. Downloaded speech libraries are cached
separately and saved after preparation, before subsequent checks or target cleanup.
Rust compilation uses one Cargo job to bound memory on the standard macOS runner.
These CLI limits preserve the shared dependency cache identity. Backend tests run
four isolated processes concurrently to bound real-Node fixture contention.
The isolated database
step selects the already-built database suite using the same backend package
graph, avoiding a second compilation with different feature unification.

CI does not supply model credentials, production profiles or OS permissions.
Live-service and device tests stay opt-in. The ordinary Flutter suite skips its
bootstrap-dependent native fixture, which the explicit remote-Node step runs.
Database fixtures start isolated PostgreSQL/MySQL servers, including native
backend and Dart FFI coverage. The FFI step excludes only the MCP subprocess
entry point, which its parent fixtures launch themselves. CI APKs use disposable
debug certificates, not distribution keys. Native builds and a macOS host-side
Flutter run do not establish iOS or Android device acceptance.

## Release packages

[The release workflow](.github/workflows/release.yml) runs when a `v*` tag is
pushed, and dispatches its build on `main`. Manual dispatch of Desktop or
[Host](.github/workflows/release-host.yml) selects `main` and supplies the exact
version tag as its `tag` input. Every job checks out that tag; the workflow
executes on main so compatible release compiler objects can be reused across
version tags instead of being isolated in each tag's cache scope. Cache and
progress tooling is sparsely checked out at the workflow's fixed commit, so
older candidate tags do not need to contain those helpers. Product packaging
scripts still come from the candidate. The tag
must match every application's version, such as `v0.1.0` for version `0.1.0`;
branch pushes and pull requests do not build releases.
Before packaging, each product requires its successful readiness gate from full
main Checks at the tag's exact commit. Desktop publishes automatically after
both signed macOS packages pass, then starts Host packaging, update feeds and
the changelog for the same tag. Host appends its verified macOS and Linux assets
to that published release and can be retried independently. Mobile publication
is not part of this workflow. Published tags and existing public assets are never
replaced. Before reusing an unpublished candidate draft, remove unsupported
attachments; Desktop refreshes its title and notes from the final tagged source.

Configure these repository secrets through GitHub's encrypted secret storage:

- `APPLE_CERTIFICATE_P12`: base64-encoded Developer ID Application certificate
  with its private key
- `APPLE_CERTIFICATE_PASSWORD`: the certificate export password
- `APPLE_API_PRIVATE_KEY`: the App Store Connect API private key contents
- `SAILRY_UPDATE_SIGNING_KEY`: the base64-encoded raw 32-byte Ed25519 publisher
  seed whose public key is present in `release/update-public-keys.txt`

Configure repository variables `APPLE_SIGNING_IDENTITY`, `APPLE_TEAM_ID`,
`APPLE_API_KEY_ID` and `APPLE_API_ISSUER_ID`. The signing identity is the Developer
ID certificate's SHA-1 fingerprint. Keep credentials out of source, artifacts and
logs; the workflow imports them into a temporary keychain and removes its key
files after packaging. The Apple account must have accepted current agreements
and the API key must have notarization access.

The packaging workflow signs the final notarized updater ZIP with its published
filename before calculating checksums. Desktop publication starts
the update-feed workflow, which downloads the published ZIPs and metadata,
verifies their version, target, digest and both publisher signatures, then
advances the root feeds with one non-forced compare-and-swap commit. The stable
feed accepts final versions only. The experience feed accepts both final and
prerelease versions, and a stable publication advances both feeds. GitHub does
not normally start another workflow for events created with `GITHUB_TOKEN`, so
Desktop publication explicitly dispatches the same verified feed and changelog
handoffs. Manual dispatch can retry those follow-ups for a published version tag.

Apple silicon and Intel runners build native Desktop DMGs, updater ZIPs and
standalone macOS Host archives. Desktop contains its local Node, not remote Host
binaries or a Python runtime. Independent Linux amd64 and arm64 runners each
build one Host architecture with glibc 2.28 targeting.
Linux link inputs are checksum-pinned in
[`linux-sysroot.lock.json`](scripts/package/linux-sysroot.lock.json); the build
does not install cross-compilation packages into the host system.

For local verification, build native Desktop and Host, then package into a
fresh `dist/` directory:

```sh
cargo build --locked --release -p sailry-desktop -p sailry-host
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
carry stapled tickets. The DMG is separately signed, notarized, stapled and checked
before distribution. The Desktop publisher attaches only macOS Desktop assets.
The Host publisher waits for the macOS and Linux Host packages, then
attaches their verified assets to the same published release. Neither publisher
overwrites public tags or existing assets. Verify both products' completed
packaging workflows before reporting the full Desktop and Host release complete.

To build and package one Linux Host locally, install the pinned Zig 0.16.0,
cargo-zigbuild 0.23.4 tools plus LLVM ar/strip, then run:

```sh
rustup target add x86_64-unknown-linux-gnu
bash scripts/build-host.sh release x86_64-unknown-linux-gnu
bash scripts/package-host.sh release x86_64-unknown-linux-gnu
```

Use `aarch64-unknown-linux-gnu` for arm64. Native Linux runners additionally execute
`scripts/check-host.py` against the packaged binary to verify headless startup,
exclusive profile ownership and shutdown. Building on macOS does not establish
Linux runtime acceptance. After packaging, Linux runners also exercise the real
systemd installer, service controls and profile-preserving update with an isolated
home and exact package bytes. Installer fixtures replace downloads, not the
service manager or Host process; pairing acceptance remains separate.

The release title is `Sailry Harness <version>`. Release notes use the exact
version's section in `CHANGELOG.md`; when that section is absent, they are
generated from the immutable tag's Git commits since the previous version tag.
Desktop publication starts the separate
[changelog workflow](.github/workflows/changelog.yml), which prepends the same
entry to [`CHANGELOG.md`](CHANGELOG.md) on the default branch and preserves existing
entries. Draft creation does not update the public changelog. Changelog updates
need `contents: write` and permission to update that file on the default branch.
A failed changelog update is reported as a failed workflow, even if the release
was already published; inspect that outcome before retrying.

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
