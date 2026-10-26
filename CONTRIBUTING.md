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
`--jobs N` controls build concurrency.
The recipes and CI share command definitions; these entries do not install hooks.

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
do not establish acceptance. Signed release packaging builds exact version tags.

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
Rust compilation uses one Cargo job to bound memory on the standard macOS runner.
These CLI limits preserve the shared dependency cache identity. Backend tests run
four isolated processes concurrently to bound real-Node fixture contention.
The isolated database
step selects the already-built database suite using the same backend package
graph, avoiding a second compilation with different feature unification.

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
Initial Desktop publication waits for full main Checks to complete successfully
at the tag's exact commit, then validates Desktop readiness. Other products retain
their own readiness gates. Desktop publishes both ad-hoc macOS
packages first, then starts [formal signing](.github/workflows/release-desktop-formal.yml),
Host packaging and the changelog independently for that same tag. Formal signing
reuses the native artifacts without rebuilding them; failure does not withdraw
the initial packages. Once both formal packages are available, the catalog
workflow attaches signed `update-v1.json` to that exact release. Desktop discovers
eligible versions through GitHub Releases, keeping stable and preview selection
separate; archive URLs always identify their exact tag. Host appends its verified
macOS and Linux assets independently. Mobile publication is not part of this
workflow. Published tags and existing public asset bytes are never replaced.
Before reusing an unpublished candidate draft, remove unsupported attachments;
Desktop refreshes its title and notes from the final tagged source.

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
Desktop publication starts the separate
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
existing sessions retain theirs. Without `--expected-revision`, the Node captures
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

ACP editors can launch `sailry acp --data-dir /absolute/profile` over stdio on
macOS or Linux while that Host is running. The adapter opens registered project
worktrees and creates, loads, resumes, lists or forks the Node's own sessions.
Models, credentials, configuration revisions, tools and approvals remain in that
Node. Editors receive canonical text, tool results and once-only approval
requests, including delegated calls. Disconnecting stdio ends observation;
`session/cancel` and `session/close` explicitly stop the current Node turn.

Text and resource links are supported. Configure additional workspaces and MCP
servers in Sailry; the adapter does not install or authorize them. Questions
requiring Sailry's richer input UI remain available there. Sailry identifiers and
status travel in `_meta.sailry`. Clients may supply `request_id` and
`expected_revision` in that metadata to retain exact admission identity and
configuration on a retry; uncertain submissions are never replayed automatically.

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
