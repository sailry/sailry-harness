# Contributing

Read [ARCHITECTURE.md](ARCHITECTURE.md) and [AGENTS.md](AGENTS.md) before changing
ownership, execution or UI boundaries. Work from the committed implementation,
preserve unrelated local changes and keep each task cohesive and reviewable.

Follow the [code of conduct](CODE_OF_CONDUCT.md). Report vulnerabilities through
[the private security reporting process](SECURITY.md), not public issues.

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

For local Rust iteration, `dev` and `test` leave workspace code unoptimized,
optimize stable dependencies at level 2, and retain level 3 for GPUI layout hot
paths. OCR preprocessing uses level 1. Release builds use LLVM with size
optimization (`opt-level = "z"`) and retain level 3 for those rendering and
layout dependencies.

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
private backup. For the current development baseline, existing owned Node storage
without `node.storage_generation` is reset along with its derived knowledge cache.
This reset creates an empty current database without migrating or backing up
development data. Current generation 1 storage retains its data. Unknown generation
values and other incompatible current definitions fail without resetting or
deleting data.

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
just ci fast        # Selected source and formatting checks
just ci affected    # Add affected lint, tests and production checks
just check          # Rust formatting and Clippy with warnings denied
just test-source    # Public guides, publication boundaries and source fixtures
just test-plugins   # Deterministic package JavaScript tests
just test-backend   # Shared Rust tests and package runtime contracts
just test-ui        # Desktop logic and service-contract tests
```

`just test-backend` and `just test-ui` use CI's nextest profile. Install the pinned runner with
`cargo install --locked cargo-nextest --version 0.9.143` if it is not available.
This selects nextest's execution settings, not Cargo's release build profile.
Standard test commands skip provider backoff waits against fixture servers;
the GDK's paused-clock tests verify retry timing. For focused direct Cargo runs,
set `GOOSE_PROVIDER_SKIP_BACKOFF=true` to use the same fixture behavior.
The backend check then builds the optional production test host and runs package
tests with that host explicitly selected, including raw-schema parameter matrices.

`just ci` checks changes outside `HEAD`, including staged, unstaged, untracked and
submodule changes. Include committed changes with an explicit baseline; inspect
the selected commands before running them with `--plan`:

```sh
just ci affected --base <base-commit> --plan
just ci affected --base <base-commit>
```

For a committed candidate, use an existing clean worktree with initialized,
matching submodules and explicit revisions:

```sh
just ci affected --candidate --base <base-commit> --head <candidate-commit>
```

The checkout must match the candidate. Equal base and head require `--all`;
`origin/main` after a push is therefore not a useful comparison baseline.
`--event pull_request` selects merge-base comparison. The runner does not switch
worktrees or stash changes. Local builds and tests share the repository's root
`target` directory; builds from another worktree select that same directory with
`CARGO_TARGET_DIR`. An explicit `--target-dir` selects the shared cache, and
`--jobs N` controls build concurrency. Local commands leave concurrency to Cargo
by default; CI defaults to two Cargo jobs. `CARGO_BUILD_JOBS` remains an explicit
override and is passed through to Cargo, including `default` and negative values.
The recipes and CI share command definitions; these entries do not install hooks.
Changes limited to the release workflows and covered publication helpers run
Source checks, including signing, release metadata and update-feed fixtures.
Product source, dependency, toolchain and build-input changes retain their
affected product checks; unknown paths select all checks.
Standalone Node integration-test changes select Shared Node. Exported fixture
modules still select Desktop, Host and Mobile FFI because those products import
the helpers directly.

Select checks from affected behavior and dependency boundaries. Reuse passing
results while their inputs remain unchanged. Explain which tests ran and any
coverage gap; do not describe a fixture or compile check as live acceptance.
Development profiles disable incremental state to avoid accumulating large
per-revision snapshots; ordinary dependency and build caches remain shared.
Other workspaces using this cache set `CARGO_INCREMENTAL=0` as well. Run the
affected tests first, not the complete platform matrix for each edit.
Designate one Cargo owner across local contributors and agents, including other
worktrees. Do not launch competing builds or create a per-task target directory.
Cargo's standard target-triple directories keep platform outputs separate within
the shared cache. Keep `PKG_CONFIG_PATH`, SDK, target and Rust flags stable between
commands, and honor the same explicit `CARGO_TARGET_DIR` when one is selected.
Reuse fixed-purpose paths such as `.runtime/worktrees/<purpose>`; do not append
run IDs, timestamps or UUIDs to create replacement caches or source checkouts.
An invalid reusable cache may be cleared and regenerated only after its owner
has stopped and its contents have been inspected. Cleanup must preserve plugin
and theme repositories, source worktrees, active resources, user profiles and
unrelated source.
Disposable test profiles and atomic staging files still need their existing
isolation and lifetime cleanup; they are not persistent build caches.

## Test conventions

The conventions follow the [pinned ADK contributor guide](https://github.com/dux-web/adk-rust/blob/5322153a79a80fbd77934f5d9313ab4da8f3fc73/CONTRIBUTING.md),
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
- Use neutral contract fixtures for reusable Core services; keep plugin business
  regressions in the owning package. Report pure JavaScript/mock checks separately
  from production plugin VM/SDK bridge checks, preserving local and remote coverage
  for affected service boundaries without requiring every suite for every change.
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
Mobile checks include Flutter analysis and controller logic tests, plus Dart FFI
contracts against isolated real Nodes. Android builds an ARM64 release
APK and checks its native libraries; iOS builds an unsigned application. Both
mobile builds check the Rust dependency boundary. Actions and toolchains are
pinned; Rust and Flutter use committed lockfiles. `Checks passed` requires every
job to succeed; skipped or cancelled jobs
do not establish acceptance. Signed release packaging builds exact version tags;
Mobile also accepts an explicitly selected, checked main commit.

Node integration features share eight compiled suites instead of linking a
binary per feature. Keep feature files focused and register new files in the
appropriate suite; the source check rejects missing or duplicate registrations.
Rust caches retain registry index metadata, compressed crate archives and Git
databases; Cargo recreates unpacked sources and checkouts. A single backend
producer fetches the locked workspace sources, shared by Checks and release jobs.
Target directories are not archived, to leave room within the repository's 10 GiB
cache budget. Cargo validates fingerprints and rebuilds changed inputs.
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
Checks uses four macOS jobs: lint, shared Node with Host lifecycle, Desktop, and
Mobile with conditional Dart FFI and iOS phases. Node-only changes run Dart FFI
contracts without rebuilding iOS.
Backend and Desktop share downloaded dependency sources, with backend as the
cache producer; Mobile reuses that source cache. Sccache caches eligible library
metadata. Downloaded speech and decision libraries share a target-specific cache
keyed by their pinned identities. Only successful jobs on `main` save complete
library archives; pull requests only restore them.
Rust compilation uses two Cargo jobs to bound memory on the standard macOS runner.
These CLI limits preserve the shared dependency cache identity. Backend tests run
four isolated processes concurrently to bound real-Node fixture contention.
When both are selected, Shared Node and macOS Host lifecycle tests use one Cargo
package graph and one JUnit report; Host lifecycle tests remain serialized.
Plugin contracts and the isolated database step reuse that same package graph,
avoiding a second compilation with different feature unification. Host-only
changes keep the dedicated lifecycle target. Production checks still verify the
shipped feature set separately from test-support builds.

CI does not supply model credentials, production profiles or OS permissions.
Live-service and device tests stay opt-in. Real GGUF goldens, cancellation and
ORT coexistence have an explicit [manual native lane](crates/decision/tests/README.md);
it uses public, hash-pinned fixtures without provider credentials. The ordinary Flutter suite skips its
bootstrap-dependent native fixture, which the explicit remote-Node step runs.
Database fixtures start isolated PostgreSQL/MySQL servers, including native
backend and Dart FFI coverage. The FFI step excludes only the MCP subprocess
entry point, which its parent fixtures launch themselves. CI APKs use disposable
debug certificates, not distribution keys. Native builds and a macOS host-side
Flutter run do not establish iOS or Android device acceptance.

## Release packages

[The version workflow](.github/workflows/release-version.yml) runs when a `v*`
tag is pushed and dispatches version publication on `main`. It also accepts manual
dispatch from `main` with an exact `tag` input. The tag must match every
application's version, such as `v0.1.0` for version `0.1.0`, and its source must be
an ancestor of the pinned main workflow commit. The entry generates notes and
publishes the version release before independently starting
[Desktop](.github/workflows/release.yml),
[Host](.github/workflows/release-host.yml), Mobile and the changelog. Version
publication does not wait for product Checks; every product build requires its
exact-source readiness gate, including strict lint and shared Node checks. Other
products' jobs can finish independently. The release can contain
only notes while these builds are running.

Desktop and Host recovery workflows select `main` and supply the exact version
tag as their `tag` input. Every product job checks out its selected source; the
workflow executes on main so compatible release compiler objects can be reused
across version tags instead of being isolated in each tag's cache scope. Cache
and progress tooling is sparsely checked out at the workflow's fixed commit, so
older candidate tags do not need to contain those helpers. Native product build
scripts and locked dependencies still come from the candidate. Branch pushes and
pull requests do not build releases.

Desktop appends both ad-hoc macOS packages, then starts
[formal signing](.github/workflows/release-desktop-formal.yml) for that same tag.
Formal signing verifies and reuses the native artifacts without rebuilding them;
failure does not withdraw the initial packages. Once both formal packages are
available, the catalog workflow attaches signed `update-v1.json` to that exact
release. Desktop discovers eligible versions through GitHub Releases, keeping
stable and preview selection separate; archive URLs always identify their exact
tag. Host appends its complete verified macOS and Linux asset set independently
of Desktop. Mobile packaging uses its independent workflow described below.
Repeated publication preserves an existing public release's metadata and assets;
an unpublished draft receives current tagged notes before publication. No
publisher replaces public tags or existing public asset bytes. Recover an
individual failed product through its own workflow.

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
filename before calculating checksums. Formal Desktop publication starts
the update-feed workflow, which downloads the published ZIPs and metadata,
verifies their version, target, digest and both publisher signatures, then
advances the root feeds with one non-forced compare-and-swap commit. The stable
feed accepts final versions only. The experience feed accepts both final and
prerelease versions, and a stable publication advances both feeds. GitHub does
not normally start another workflow for events created with `GITHUB_TOKEN`, so
the version entry explicitly dispatches product and changelog workflows, while
formal Desktop publication dispatches the verified feed handoff. Manual dispatch
can retry those follow-ups for a published version tag.

The independent [Mobile packages workflow](.github/workflows/release-mobile.yml)
accepts dispatch from `main`. Its optional `tag` input selects the exact source of
an already published version release. It waits for Mobile readiness at that exact
source before making signing credentials available.
A dispatch without a tag can instead supply `source_commit`, the full
40-character SHA of an ancestor of the workflow commit; leaving both inputs empty
builds the workflow's source. A tag and source override cannot be combined. This
allows a newer packaging workflow to use an already checked product commit
without treating the tooling commit as checked. Product build scripts, locked
dependencies and versions come from the selected source. Distribution helpers
and cache actions are sparsely checked out at `github.workflow_sha`; package
metadata records product and workflow revisions separately.
Manual dispatch selects `both`, `android` or `ios`; disabling `upload_testflight`
preserves the signed IPA without uploading it. The version entry selects both
platforms and uploads the iOS package for TestFlight. Android produces a signed
ARM64 APK. Both jobs retain packages, source metadata and SHA-256 checksums as
Actions artifacts for 30 days. Tagged builds independently append each platform's
package, metadata and checksum to the published version release; builds selected
without a tag retain only Actions artifacts. An IPA packaged before a TestFlight
upload failure remains available for download. These builds do not establish
real-device acceptance.

Mobile distribution uses these additional repository secrets:

- `ANDROID_KEYSTORE_BASE64`: base64-encoded Android distribution keystore
- `ANDROID_STORE_PASSWORD`, `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD`
- `IOS_CERTIFICATE_P12`: base64-encoded Apple Distribution identity, including
  its private key, exported as an Apple-compatible PKCS#12 archive
- `IOS_CERTIFICATE_PASSWORD`: the iOS certificate export password
- `IOS_PROVISIONING_PROFILE`: base64-encoded App Store provisioning profile for
  `com.sailry.sailryMobile`, bound to that Distribution certificate and team

Set `ANDROID_SIGNING_SHA256` to the distribution certificate's SHA-256
fingerprint. iOS reuses `APPLE_TEAM_ID`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID`
and `APPLE_API_PRIVATE_KEY` for its team and upload authorization; the API key
must have App Store Connect upload access. The macOS Developer ID certificate
cannot replace the iOS Apple Distribution identity. Signing files live only in
the runner's temporary directories and are removed even when packaging fails.

The iOS marketing version uses the numeric core of the shared product version:
`0.1.0-alpha.9+9` becomes `0.1.0` with build number `9`. Keep product versions
aligned and increase the Mobile build number before uploading a distinct build;
Apple will reject an already accepted version/build pair. After Apple processes
an upload, complete any export-compliance questions and assign the build to a
TestFlight group in App Store Connect. External testing may require Beta App
Review. The workflow does not infer encryption declarations or invite testers.

Apple silicon and Intel runners build native Desktop DMGs, updater ZIPs and
standalone macOS Host archives. Desktop contains its local Node, not remote Host
binaries or a Python runtime. Independent Linux amd64 and arm64 runners each
build one Host architecture with glibc 2.34 targeting. The locked ARM64 Decision
libraries additionally require glibc 2.38; the compatibility check verifies their
exact hashes before applying that ceiling, without changing Host or ORT limits.
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

GitHub publications are ordinary releases, including versions with a SemVer preview
suffix. Update channels follow the semantic version rather than GitHub's Pre-release
flag; preview versions remain in the experience channel and do not replace Latest.
Older preview releases retain their existing GitHub flag.

The release title is `Sailry Harness <version>`. Release notes use the exact
version's section in `CHANGELOG.md`; when that section is absent, they are
generated from the immutable tag's Git commits since the previous version tag.
Version publication starts the separate
[changelog workflow](.github/workflows/changelog.yml), which prepends the same
entry to [`CHANGELOG.md`](CHANGELOG.md) on the default branch and preserves existing
entries. Draft creation does not update the public changelog. Changelog updates
need `contents: write` and permission to update that file on the default branch.
A failed changelog update is reported as a failed workflow, even if the release
was already published; inspect that outcome before retrying.

## Host automation

For installation and pairing, see [Install](README.md#install).
Run `sailry help` for available commands. Service controls are `start`, `stop`,
`restart` and `status`; `version` reports the installed release.

`sailry share` displays a private pairing PIN while it runs. Press Ctrl-C to
cancel. `sailry update` installs the newest published release, including previews;
use `--version 0.1.0-alpha.1` to select one. The installer does not replace
`~/.sailry-host` and retains the previous program files in a reported backup
directory. One fixed backup is kept beside the program directory; failed updates
preserve the existing backup. Failed updates restore the previous program files,
managed command, service definition and activation state before reporting failure,
so the current installer can be retried. If recovery itself fails, the installer
reports the remaining recovery files instead of deleting them.

For an existing Linux root installation in `~/.local/lib/sailry`, use Desktop's
Host update or the current release's verified installer from [Install](README.md#install):
`bash install-host.sh --update --version <version>`. This moves program files to
`/usr/local/libexec/sailry` and preserves the profile; later updates can use `sailry update`.

Submit a noninteractive task to the already running Host with a registered
worktree ID or an existing session ID:

```sh
sailry submit --worktree "$WORKTREE_ID" --prompt-file task.txt --request-id "$REQUEST_ID"
sailry submit --session "$SESSION_ID" --expected-revision "$REVISION" --stdin < task.txt
sailry wait --session "$SESSION_ID" --turn "$TURN_ID"
```

Submission uses the Node's current defaults for a new session. An optional
`--config` JSON file supplies a complete session configuration for a new task;
existing sessions retain theirs. New tasks can select `--execution model` for direct
model execution or `--execution team --team "$TEAM_ID"` for a team. Omitting execution
retains Default team behavior. `--team-revision` pins the exact team revision;
without it, fresh admission resolves the current team while replay retains the original
capture. An optional `--association` JSON file contains `{ "namespace": "automation",
"kind": "job", "id": "daily-review" }` for later lookup through the shared session
protocol. Execution, team, association and configuration options require `--worktree`.
Model execution cannot select a team. Without `--expected-revision`, the Node captures
the session's current configuration on first admission; the same request ID and
input recover that turn even if the session configuration later changes. An
explicit revision must match for a new admission. `--no-wait` returns after durable
admission.
Each command writes one JSON result containing the Node, request, session, turn,
frozen revision, `state`, answer text, and pending approvals or questions. Keep the same
request ID, input and supplied revision option when retrying an uncertain
submission. `wait` observes the original turn without submitting it again. Ctrl-C
and `--timeout` end observation while execution continues; approvals remain
available in Desktop or Mobile.

Exit codes are `0` for admission or completion, `1` for failed or stopped work,
`3` for pending approval, input or paused work, `4` for an uncertain result,
`124` for timeout, and `130` for cancelled observation. Read the JSON `state` and
`error` to distinguish admission failures from execution and observation results.
Task management uses the private Unix control socket on macOS and Linux.

For CI on an already configured Host, invoke `scripts/ci-task.sh`. Set
`SAILRY_REQUEST_ID` to an ID retained across retries and exactly one of
`SAILRY_WORKTREE` or `SAILRY_SESSION`; supply the prompt on stdin or through
`SAILRY_PROMPT_FILE`. Optional variables are `SAILRY_PROFILE`, `SAILRY_TIMEOUT`,
`SAILRY_EXPECTED_REVISION` for an existing session, `SAILRY_CONFIG` for a new task,
and `SAILRY_COMMAND` for the installed management executable. The script forwards
the JSON and exit code without starting a service or approving tools.

## Public source and history

Only the explicitly listed public guides in `docs/` are tracked; other internal
documents, plans, local outputs, profiles and credentials remain ignored and must not be tracked.
Public guides, `AGENTS.md`, package Skills, fixtures, licenses
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

## Contributor License Agreement

Contributions require signing the [Sailry CLA](CLA.md). It grants the project
the right to relicense contributions, including under commercial terms. Sign it
in your first pull request as described in that file; pull requests are not
merged without a signed CLA.

## Licensing

Sailry-owned contributions use [AGPL-3.0-only](LICENSE). The plugin SDK
interface files listed in [NOTICE](NOTICE), including [sdk/](sdk/), use
[Apache-2.0](sdk/LICENSE). Preserve third-party copyright, source provenance
and license notices. The root license does not
relicense vendored source, imported shell scripts, fonts, icons or other resources.
Include applicable notices when packaging or redistributing those materials.
