# Contributing

Read [ARCHITECTURE.md](ARCHITECTURE.md) and [AGENTS.md](AGENTS.md) before changing
code. The Go module and repository are `github.com/sailry/sailry-harness`.

## Scope

The checkout is self-contained: locale sources live in `locales/`, plugin SDK
declarations in `sdk/`, and official packages in the pinned submodules. Setup,
generation, builds and tests do not read another project directory.

Keep changes within the requested scope. For fixes, include a reproduction and
evidence of the corrected behavior. Keep local commits cohesive with imperative
English subjects. Agents commit locally and push or open pull requests only when
requested.

## Setup

Requirements:

- Go 1.27.3 and Git.
- Linux Git workflows use the system Bubblewrap with `--bind-fd` and
  `--ro-bind-fd` support (0.10 or a distribution backport), and a kernel
  with `openat2` (Linux 5.6 or later) and working user namespaces.
- The MyGo and other module versions pinned in `go.mod`, with their platform
  runtime dependencies. Do not edit module-cache sources.
- Native builds use the Rust toolchain and dependency locks under `native/`.
  Mobile uses the Flutter version in `apps/mobile/.flutter-version` and the
  target's Android NDK or Apple SDK. The TypeScript pairing service uses its
  own Node.js package tooling.

Use this environment before every Go command:

```sh
cd /path/to/sailry-harness
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
```

Initialize the reviewed submodule snapshots:

```sh
git submodule update --init --recursive --checkout
```

Run the same command after switching to a revision with different submodule
pins. `git submodule status` prefixes a mismatched checkout with `+`.

Official plugins (`plugins/`) and themes (`themes/`) are separate repositories.
Their README and AGENTS files cover package maintenance. Commit package changes
there first, then the reviewed Gitlink here; publishing is a separate, explicitly
authorized action. Never replace a pin with an unreviewed remote tip.

Generate the desktop catalogs from the local source catalogs:

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
go generate ./internal/desktop/i18n
```

Each key is defined in one source file per language. Edit its existing definition
when changing copy; the generator rejects duplicate keys.

The model metadata snapshot is maintained by `.github/workflows/model-catalog.yml`
every six hours and can also be refreshed manually:

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
go run ./cmd/update-model-catalog
```

Only validated content changes are committed by the workflow. Releases embed
the snapshot in their source commit rather than downloading mutable data while
packaging. This does not update the reviewed request-policy catalog in
`internal/node/agent/model/data`.

The desktop entry point is:

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
export SAILRY_HELPERS_DIR="$PWD/.runtime/native/$(go env GOOS)-$(go env GOARCH)/helpers"
go run ./cmd/sailry-desktop --data-dir .runtime/profiles/desktop
```

The helper directory points source runs at the staged computer-use helper
because `go run` places its executable in a temporary directory. Store tests
and source runs need the FTS-enabled Turso library. `just native` prepares
the Turso library using its pinned Rust toolchain. Source builds
check `.runtime/native/<goos>-<goarch>/` from the working directory and the
source executable's ancestor directories. `SAILRY_TURSO_LIB` selects an
explicit source-build library and skips its build in `just native`.
The Desktop recipes run `just native` and supply the helper path automatically:

- `just desktop` opens the installed release data at `~/.sailry`.
- `just desktop dev` opens the development data at `~/.sailry-dev`.
- `just desktop <name>` opens an isolated test profile at `.runtime/profiles/<name>`
  for any other name.

These runs serve Go profiles on `http://127.0.0.1:6061/debug/pprof/`, for
example `go tool pprof http://127.0.0.1:6061/debug/pprof/heap`. Other runs
serve them only when `SAILRY_PPROF` names a loopback address.

These commands run the current checkout; the choice selects data, not a different
application version. Close another instance using that profile before starting.

The desktop starts one local Node, Client and shared Network. `--preview` exits
backend startup before opening any profile, including an explicit `--data-dir`.
A Node without a supported saved network selection uses any explicit `--relay`
multiaddrs, or listens for direct connections when none are configured.
Additional live windows share the process backend, remote Clients and profile
preference authority. Each window closes its own observations and forwards;
application shutdown closes Network before the local Client and Node. MyGo keeps
window bounds and `shell.Prefs` in its application-data folder. For implementer
UI checks, isolate that folder with MyGo's path override in a disposable harness.

With [just](https://just.systems) installed, `just --list` shows recipes for these
commands with the same environment: `just preview`, `just desktop <name>` and
`just host <name>` (profiles under `.runtime/profiles/<name>`), `just check` for
the standard Go checks, and the test, plugin, pairing-service and Mobile recipes.

Implementer checks use `--data-dir .runtime/profiles/<fixed-name>` or
`just desktop <fixed-name>`. The user-facing `just desktop` and `just desktop dev`
recipes intentionally select the release and development acceptance profiles;
do not use them for routine automated checks. Tests use `t.TempDir()`.

## Layout

| Path | Responsibility |
|---|---|
| `cmd/sailry-desktop` | Official Desktop command and packaging |
| `internal/desktop/application` | Reusable Desktop startup, shared local Node and explicit preview |
| `internal/desktop/{shell,sidebar,conversation,composer,panel,panes,settings,host,transfers,onboarding}` | Accepted UI, state machines, view models and actions |
| `internal/desktop/{kit,theme,i18n,icons,viewmodel}` | Shared UI components, themes, copy, icons and neutral presentation contracts |
| `internal/desktop/{live,preferences,preview}` | Live action adapters, profile controller settings and isolated simulation |
| `bundled.go` | Root embedded themes and plugin snapshot from pinned submodules |
| `locales/` | Source catalogs for local i18n generation |
| `docs/` | Public user guides linked from the README and published on the website |
| `schemas/` | Published v1 plugin contract schemas and the Agent Plugins manifest schema |
| `third_party_licenses/` | Index of third-party notices, shared license texts and the Go module list |
| `justfile` | Local recipes for the commands in this document |
| `sdk/` | The v1 plugin contract and its declarations |
| `internal/{protocol,link,client,core,node,...}` | Backend packages ([ARCHITECTURE.md](ARCHITECTURE.md#components-and-dependencies)) |
| `cmd/sailry-host`, `cmd/sailry-mobile`, `internal/mobile`, `apps/mobile` | Host and Mobile boundaries |
| `native/` | Pinned native libraries and their build inputs |
| [Relay pairing service](https://github.com/sailry/sailry-relay/tree/main/pairing) | Short-code rendezvous service, separate from the encrypted libp2p data relay |
| `.runtime/` | Ignored fixed-purpose experiments, libraries and test profiles |

## Checks

After integrating each development stage, the coordinating agent performs the
relevant compilation and tests by category. Child agents perform only static
checks on their own changes. Use worktrees for edits that could conflict, and
keep each worktree's scope explicit.

The standard full-repository Go checks are `just check` or:

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
test -z "$(gofmt -l .)"
go build ./...
go vet ./...
go test ./...
```

Run the full checks for release review and broad changes. For a focused fix,
select the affected packages and reuse passing results until their inputs change.
Documentation-only changes need link and content review, not a repeated Go suite.

Additional checks depend on the changed inputs:

| Change | Additional check |
|---|---|
| `internal/mobile` or `cmd/sailry-mobile` | `CGO_ENABLED=1 go vet ./cmd/sailry-mobile` |
| `native/` | `cargo build --locked` in the changed native project |
| `apps/mobile` | `flutter analyze` and `flutter test` there |
| `internal/node`, `internal/client` or `internal/link` | Before a release, `CGO_ENABLED=1 go test -race -p 1 -parallel 2` on the changed packages |

The mobile C ABI and local race detector are the documented cgo exceptions;
ordinary Desktop and Host code uses `CGO_ENABLED=0`. Native artifacts belong
under `.runtime/native/<goos>-<goarch>/`, never in commits. Native Rust builds
use their committed lockfiles and keep toolchain and target settings stable.
Reuse those artifacts for Go development. Cargo runs again only when the native
source, C ABI, dependency pins or target changes.

The Flutter build runs the Mobile adapter build itself: Gradle's
`prepareSailryNative` task and the Xcode `Build Sailry Native` phase call
`cmd/sailry-mobile/build.sh`, which builds the Go C ABI for each target slice
into `.runtime/native/`. It needs Flutter at the version in
`apps/mobile/.flutter-version` and the Android NDK (`ANDROID_NDK_HOME`) or Xcode. The same script builds one slice:

```sh
cd apps/mobile && flutter pub get --enforce-lockfile
flutter build apk --release --target-platform android-arm64
flutter build ios --release
sh cmd/sailry-mobile/build.sh android arm64-v8a      # from the repository root
sh cmd/sailry-mobile/build.sh ios iphoneos arm64
```

Native transport, relay outage/recovery, SSH installation, Host task automation
and actual Dart callbacks require isolated integration fixtures. A passing
macOS c-shared build or Dart process does not establish physical Android or iOS
acceptance. Flutter analysis and the existing Flutter contract tests remain
separate checks.

When the transcript changes, run its benchmarks separately:

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
go test ./internal/desktop/conversation -run '^$' -bench .
```

The frame budgets are Steady ≤ 0.2 ms, Hover ≤ 0.25 ms and Scroll ≤ 1.2 ms.
Record before/after numbers and their environment; a build or headless fixture
does not prove live or cross-platform acceptance.

Compare storage performance on the same reference machine. The benchmark
runs 20 concurrent sessions with a budget of at least 2,000 committed records
per second and a commit p99 of at most 20 ms.

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
go test -tags integration ./internal/node/store -run '^$' -bench '^BenchmarkStorageLoad$' -benchtime=1x -count=1
```

The release integration suite runs the same workload on Linux, macOS and
Windows and verifies complete writes, successful commits, concurrent reads
and full indexing.

## Test conventions

- Test Sailry logic and contracts: mappings, state transitions, protocol fixtures,
  storage transactions and scripted model turns. Libraries own their own tests.
- Put a regression at the layer that owns the defect. Avoid repeating the same
  assertion through Store, Client and UI; use compact table-driven cases where useful.
- Keep default tests deterministic and isolated with `t.TempDir()`, injected
  clocks, local fakes and `net/http/httptest`. User profiles, real provider accounts
  and external services are not default test fixtures.
- Use `t.Parallel()` when a test touches no shared state.
- Real processes, providers, models, libp2p peers, remote storage and OS permission prompts
  belong behind `//go:build integration`. Other-OS spike programs are run on the
  named system by the user or CI; unrun checks are reported as unrun.
- Keep UI tests headless and few. Preserve accepted interaction tests; do not add
  pixel goldens, per-control framework tests or extra end-to-end gates.
- A failing check must be diagnosed and repaired without weakening assertions or
  skipping cases. Reuse passing results until a relevant input changes.
- Plugin behavior tests belong in the plugins repository. Core contract fixtures
  stay package-neutral.

## Continuous integration

`ci.yml` runs Go, native and Mobile checks without packaging or publishing. It runs on
pull requests, on `main`, manually, and when `release.yml` calls it:

- `lint`: Go formatting and `go vet -tags integration ./...` for Linux,
  Windows and macOS. The tag adds the integration files to the default ones,
  so integration tests that no routine job runs are still compiled.
- `test`: Linux runs the complete Go suite with cgo disabled on every event.
  Four lanes, `linux-0` through `linux-3`, balance complete packages and the
  top-level tests, examples and fuzz seeds of `desktop/live` and `desktop/shell`,
  retaining each parent's subtests. Measured duration hints affect placement
  only; new packages and tests join automatically. macOS and Windows each have one `platform`
  lane: `go build ./...` followed by selected native-boundary package suites and
  focused shared-package regressions in `.github/scripts/platform-tests.tsv`.
  Tests in OS-suffixed or build-constrained files compiled for that OS join
  automatically, including new cases. A suite entry naming a missing package
  or matching no test fails the lane. The three-platform vet still checks the
  full test source. The routing fixture runs in `lint` with a fake Go:
  `bash .github/scripts/test_test.sh`.
- Go setup and cache handling share local actions. Each test job restores its
  own target's Turso runtime from a cache keyed by `native/turso`, or builds
  it, without waiting for other platforms; the checksum recorded with that
  build detects a damaged cache entry. One writer per operating system saves
  the test compiler cache even after a test failure; cancelled runs and pull
  requests do not save it. Mobile jobs install the Flutter version named in
  `apps/mobile/.flutter-version` and the Android toolchain through local
  actions.
- `native`: stage the pinned Turso and capture libraries for supported targets,
  plus the app-local C++ runtime on Windows. The `native-<goos>-<goarch>`
  artifact includes `SHA256SUMS` and is read by `native/fetch -artifacts`.
- `apps/mobile`: Flutter analysis, tests and ARM64 Android/iOS release builds for Mobile, shared Client/Link/Protocol or build-input changes; CI uses a disposable APK signer and disables iOS signing.
- `integration`: never on pull requests or main. Manually with the
  `integration` input it tests `integration_packages`; left empty, and in a
  release, it tests the release set named in the workflow: packages that need
  no downloaded model and no external service. Each leg verifies its native
  artifact and runs the Link tests against local peers and a private relay.

New commits cancel obsolete pull-request and branch-push CI runs. Manual
integration checks and tag releases keep their own runs.

`release.yml` runs on a `v*` tag. Every job has read-only repository access
except `publish`:

- `version`: the tag must equal `link.ProductVersion`, the `mygo.json` version
  and the `pubspec.yaml` version, and `CHANGELOG.md` must hold the entry.
- `checks`: reuse a successful default-branch CI run for the exact source
  commit, then run integration tests. Go checks require a successful lint job
  and all six test lanes; Flutter checks are reused only if that run completed
  them. Missing evidence falls back to the corresponding checks. The native
  job always runs in the release, from its content cache when the sources are
  unchanged, so every release run holds its own native artifacts for packaging
  and later repairs. Final signed Mobile packages replace the duplicate
  unsigned builds.
- `macos` (arm64 and Intel): `cmd/sailry-desktop/macos/package.sh` and the
  macOS Host archive, signed with the Developer ID certificate.
- `windows`: `cmd/sailry-desktop/windows/package.sh` on Linux, around the
  libraries of the Windows native leg.
- `host-linux` (x64 and arm64): `internal/node/host/deployment/archive.sh`.
- `android`, `ios`: the signed APK and the App Store IPA; the IPA goes to
  TestFlight and stays a workflow artifact.
- `publish`: signs the Desktop update feed, writes the checksum files, takes
  the release notes from the tag's `CHANGELOG.md` entry and uploads to the
  tag's draft release only what it does not hold yet. An existing asset with
  other content, or a release that is already published, fails the job.
  Publishing the draft is manual.

For macOS packaging-only repairs, manually run `Release` on the version tag with
`reuse_run` set to the completed full release run ID. The workflow requires all
checks and non-macOS packages to have passed, verifies that only the macOS
packaging scripts, release workflow or this document changed, and rebuilds both
macOS packages using that run's native artifacts. It reuses the original Windows,
Linux and Android packages and does not repeat the successful TestFlight upload.
Reused artifacts retain their original source revision; the repaired macOS
packages record the new revision.

The Relay repository tests and deploys its pairing service. `website.yml`
asks the website repository to rebuild when the documents it reads change on
main.

`model-catalog.yml` refreshes the bundled model metadata every six hours and
commits validated content changes on the default branch.

**Sentry intake and repair.** Enable the two workflows in stages on
`sailry/sailry-harness`. No production event text or credentials are workflow
inputs.

1. **Verify first.** Protect the default branch. Leave `SENTRY_INTAKE_ENABLED`
   and `SENTRY_REPAIR_DRAFT_PR` unset or `false`. Run `sentry-repair.yml` manually
   on that branch with `template=queue-stopping`, `fingerprint` (the 64-digit
   lowercase SHA-256 hex digest of `sentry:<org>:<project-id>:<issue>`) and
   `target` (a published `vMAJOR.MINOR.PATCH` tag or a full lowercase commit
   SHA). The resolved commit must belong to the default branch's history.
   Verification uses the built-in `GITHUB_TOKEN` with `contents: read` and no
   Sentry secret. The fixed `TestStoppingQueueGuidance` assertion
   must fail on both the affected revision and the default-branch revision and
   pass after applying the reviewed patch. A green baseline, missing candidate,
   build error, skipped test or different failure does not qualify for a draft.
2. **Enable intake.** Create the `sentry-intake` Actions environment,
   restrict it to the protected default branch and store `SENTRY_READ_TOKEN`
   only as an environment secret. Use a read-only token with `org:read` and
   access to the selected project on `sentry.io`, not a write token. Set Actions
   variables `SENTRY_ORG` (organization slug) and `SENTRY_PROJECT_ID` (numeric
   project ID), then set
   repository variable `SENTRY_INTAKE_ENABLED=true`. Neither `SAILRY_SENTRY_DSN`
   nor `SAILRY_TELEMETRY_KEY` is an intake credential. Run `sentry-intake.yml`
   manually on the protected default branch and inspect the Actions summary;
   the schedule runs at minute 17 each hour. It samples the last closed UTC
   hour with five minutes of indexing grace, at most 20 events per query,
   without pagination or backfill. Other guide conflicts and workspace errors
   are observations only. At most two `recipient_stopping` groups with a release
   revision are selected. If none qualify there is only a run summary and an
   `[]` job output: no reservation, dispatch, branch or PR. The handoff alone
   uses the built-in `GITHUB_TOKEN` with `actions: write` to reserve each
   group/revision in the Actions cache and dispatch verification. Inspect the
   original run if dispatch is uncertain; do not blindly repeat it. Cache eviction
   can allow another verification, but does not bypass the PR check below.
3. **Enable draft publication only after reviewing verification.** Allow GitHub
   Actions to create pull requests in repository settings, then set repository
   variable `SENTRY_REPAIR_DRAFT_PR=true`. Only a verified candidate enters
   this separate job. Its built-in `GITHUB_TOKEN` needs `contents: write` and
   `pull-requests: write`; no PAT or Sentry token is needed. It checks the base
   has not moved, rebuilds the verified tree and creates or recovers one draft
   per fingerprint. An existing PR, even if closed, is not reopened or replaced.
   Without a verified candidate there is no branch or PR write. Review and
   merge remain manual; neither workflow deploys or auto-merges to production.

Actions are pinned by commit. Do not add nightly fuzzing, coverage gates,
per-test matrices or retry-on-flake wrappers.

## Release packages

Packaging entry points are `mygo build`, `flutter build`,
`native/*/build.sh` and the Host installer scripts under
`internal/node/host/deployment/`.
Signing credentials, profiles, binaries and native artifacts never enter Git.

`go run ./native/fetch` stages one target's pinned native artifacts in
`.runtime/native/<goos>-<goarch>/` after checking each download against the
lock files (`native/*.lock.json` and
`internal/localmodels/decision/llama-runtime.lock.json`). Turso and capture
come from their native build on the target or the CI native job with
`-artifacts <dir>`; `-bundle <dir>` copies the shipped set and `-host`
leaves out the speech and capture libraries.

The Desktop package is described by `cmd/sailry-desktop/mygo.json`; its version
equals `link.ProductVersion`. Shipped files sit beside the executable, or in
`Contents/Resources` of the macOS bundle, where every loader reads them.

A release build is one linked with
`-X github.com/sailry/sailry-harness/internal/core.Default=~/.sailry`
(`MYGO_LDFLAGS` for `cmd/sailry-desktop`, `go build -ldflags` for
`cmd/sailry-host`). It opens the release profile `~/.sailry` by default and
loads its native libraries from its package and checks the package seal.
Source builds can load the staged native libraries from `.runtime/native`. Source builds keep `~/.sailry-go`.

`cmd/sailry-desktop/macos/package.sh` packages the macOS Desktop on the target
architecture: it stages the natives, the dependency notices (`licenses/`) and
`build.json`, runs `mygo build -sign <identity> -skip-notarize -skip-dmg` with
the release flags, renames the executable to `sailry-desktop`, which is the
name the updater requires, seals, notarizes and creates the disk image
`Sailry-<triple>.dmg` and `Sailry-<triple>-unsigned.zip`, the archive of the
finished application for the update feed.
The macOS bundle's `CFBundleShortVersionString` and `CFBundleVersion` use the
numeric version core (for example, `0.2.0`). The executable, `build.json` and
signed update catalog retain the complete version, such as `0.2.0-alpha.1`.
`cmd/sailry-desktop/windows/package.sh -artifacts <dir>` cross-compiles the
Windows x64 Desktop on Linux or macOS with NSIS installed: the installer
`Sailry-x86_64-pc-windows-msvc-setup.exe` (mygo's per-user NSIS installer) and
the portable `Sailry-x86_64-pc-windows-msvc.zip`.
The Windows native artifact also contains the app-local Microsoft C++ runtime
from its installed compiler, with original redistribution notices and checksums.
`native/fetch` puts those files beside the executable and includes them in the
seal; no system-wide redistributable installation is required. See
[`native/windows/README.md`](native/windows/README.md) for local staging.

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
bash cmd/sailry-desktop/macos/package.sh                    # ad hoc, not notarized
go run ./native/fetch -seal .runtime/package/out/darwin-arm64/Sailry.app   # the seal step alone
```

`go run ./native/fetch -seal <path> [-sign <identity>] [-host]` is the step
after signing. Signing a macOS application rewrites its libraries, so the
upstream pins no longer describe them. The step writes `native-seal.json` (size
and SHA-256 of the ONNX Runtime, sherpa-onnx, Turso and capture libraries as
shipped), rewrites `llama-runtime.json` and the file list of
`python/python-runtime.json` from the signed files, and signs the bundle again
without `--deep` so the code signature covers these records. The loaders use
the seal in place of the upstream pins only in `Contents/Resources` of the
running bundle; an invalid seal there is an error. Unsigned and source builds
keep the upstream pins. For a plain directory (the Host archive, Linux,
Windows) the step writes `native-seal.json` for the locally built libraries
and the packaged Windows runtime as applicable. The loaders compare hashes; the code
signature itself is enforced by macOS, not by Sailry.
`cmd/sailry-desktop/package_integration_test.go` checks a built package's
loaders from inside it: compile it with the release `-ldflags`, copy it beside
the packaged executable and seal the bundle again. The Turso check creates a
temporary database and exercises its native full-text index; it does not open a
user database or start Node, models or tools.

Signing is configured by the packaging scripts and release workflow.
`SAILRY_SIGNING=developer-id` needs the variables `APPLE_SIGNING_IDENTITY` and
`APPLE_TEAM_ID`; `SAILRY_NOTARIZE=1` needs `APPLE_API_KEY_ID`,
`APPLE_API_ISSUER_ID` and the key file `APPLE_API_KEY_PATH`. The release
workflow's `macos` job imports the secrets `APPLE_CERTIFICATE_P12`,
`APPLE_CERTIFICATE_PASSWORD` and `APPLE_API_PRIVATE_KEY` into a temporary
keychain (`APPLE_SIGNING_KEYCHAIN`). The bundle identifier is
`ai.sailry.desktop`, the disk image identifier `ai.sailry.disk-image`. Every
Mach-O file must carry the team, the hardened runtime and a secure timestamp;
notarization submits with `notarytool --key` and staples the application and
the disk image. The application keeps the microphone entitlement and the
bundled Python interpreter keeps `disable-library-validation`. Windows packaging
currently leaves `windows.certificate` and `windows.signCommand` unset in
`mygo.json`, so its artifacts are unsigned.

A release opens active `storage/node.turso` and `storage/knowledge.turso` files
from `v0.2.0` onward in place with the official Turso Go SDK. The schema
synchronizer adds compatible tables and fields. When no active database files
exist, startup initializes new Turso storage; it does not migrate older archives
or preview `.sqlite` files and leaves them untouched. Node identity,
controller preferences, themes and file trees remain in place.
`internal/desktop/application/upgrade_integration_test.go` checks this with real
native databases initialized from the recorded Rust schema. Run it with
`just test-integration ./internal/desktop/application`.

The Desktop update feed is written by
`go run ./internal/desktop/updater/cmd/update-feed -version <version> -output
<dir> <target>=<unsigned zip>...` with the publisher key in
`SAILRY_UPDATE_SIGNING_KEY`, which must belong to
`internal/identity/update-public-keys.txt`. For both macOS targets it
writes the signed `Sailry-<triple>.zip`, its `.zip.update.json` and the signed
catalog `update-v1.json`, then checks them with the updater's own
verification. `TestPublishedRelease` proves the format with a throwaway key.
The updater has no Windows installer, so the feed covers macOS only.

Fork Desktop commands select their identity, profile and release settings in Go
and use matching native packaging metadata. Use `updater.PublishRelease` with
the same `DesktopRelease` when signing a fork feed. Packaged builds set the
existing release `core.Default` linker flag to the product's installed profile
to keep native libraries confined to the package. The fork fixture selects the
published official Host. SSH and WSL resolve its tag from the official release
repository independently of the fork Desktop version. A separately distributed
Host requires its own package source and distinct program, profile and service
paths.

`internal/node/host/deployment/archive.sh [-artifacts <dir>]` assembles the
Host archive of the machine it runs on. Each archive
`sailry-host-<triple>.tar.gz` (`x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-apple-darwin`)
holds `sailry-host`, the pinned runtimes of
`native/fetch -host`, the files of `internal/node/host/deployment/package/`,
`build.json`, the dependency lock files and `licenses/` from
`internal/node/host/deployment/licenses.sh`, whose optional fourth argument
names the command package. On macOS `sailry-host` and the Turso library
are signed: the pinned runtimes keep the bytes their locks record, so the
archive is not notarized. The release carries `install-host.sh`,
`SHA256SUMS-installer`, the archives and one `SHA256SUMS-<triple>` each. SSH
Add Host, its update and the WSL installation download exactly those names
from `https://github.com/sailry/sailry-harness/releases/download/v<version>/`,
so they work once the draft release is published.
`internal/node/host/deployment/SOURCE` records where the ported files come from.

## Manual acceptance

Manual checks exercise the actual application and packaged runtime. Select the
areas affected by a change and record the build, operating system, result and
limitations. A historical checklist or passing headless test does not establish
current platform acceptance.

Development checks use `just desktop <fixed-name>` with an isolated profile.
For user acceptance, `just desktop dev` opens `~/.sailry-dev`; `just desktop`
opens the installed release profile at `~/.sailry`.
Account and device checks require the corresponding account or device and remain
explicitly initiated.

### Manual conversation automation

These suites run only when explicitly started. They are not included in CI,
`go test`, `just check` or the normal development startup. A passing run covers
the listed assertions on that model and environment; it is not a claim that
all backend behavior or all interface behavior is bug-free.

Agents use scripted models, fixtures and recorded replay for routine verification.
Real-model acceptance requires the user's explicit request.

Acceptance is a development command, not a setting in the application. Configure a
provider in an isolated profile once with `just desktop acceptance`, then close
that Desktop before running tests on that profile. A channel can be selected by
its unique display name or provider ID. Model and effort parameters apply only to
the test sessions and never change the configured defaults.

```sh
just acceptance acceptance --suite quick --provider "Test channel" --model "model-id" --effort medium
just acceptance acceptance --suite full --provider "Test channel" --model "model-id" --effort medium
just acceptance acceptance --suite backend --case F01-exit127 --timeout 3m
just acceptance acceptance --suite e2e --case E2E01 --provider "Test channel" --model "model-id" --effort medium
just acceptance-ui
```

`just acceptance` builds Host and Desktop, starts one temporary Node owner of the
selected test profile, and stops it after the run. Add `--start=false` to attach
to an already running Host. The default profile is `.runtime/profiles/acceptance`;
never point an implementer run at the user's profile. A live UI child attaches to
that Node over its private control socket; it does not copy credentials or start
a second Node. UI-only and replay runs open no Node and call no model.

| Scope | Work performed |
| --- | --- |
| `quick` | Selected backend cases, fixture interface checks and three live UI cases |
| `backend` | All registered backend cases through Client and Host |
| `ui` | Fixed conversation data and real interface interactions in an isolated headless process |
| `e2e` | Three real Client-to-interface conversation cases |
| `full` | All backend cases, fixture interface checks and live UI cases |
| `replay` | Recorded conversation snapshots and tool results through the real interface, without a model |

The command saves private `latest.json` and `latest.html` under
`<test-profile>/acceptance` and opens the HTML report in the system browser even
when a case fails. `--report-dir` chooses another dedicated report directory.
The direct Host CLI opens a report only with `--open-report`; `just` adds it.
Exit codes are 0 for a passing run, 1 for failure/blocked/incomplete, and 130 for
cancellation. Case progress is also written as JSON lines.

Every created test session is recorded in an ownership ledger, including fork
and backup sessions. A run stops its work, saves the test-owned evidence, and
removes its temporary conversations after publishing the report. An interrupted
or failed cleanup leaves its recorded identities available to this command:

```sh
just acceptance-cleanup acceptance
```

Cleanup stops and confirms turns and processes before removing recorded test
sessions. It never searches by title or removes ordinary conversations. Reports can supply explicit test session IDs. Keep the report and ownership
files so interrupted runs can be cleaned up safely.

Live runs save bounded conversation recordings beside the report. Replay them
without a provider account or network access:

```sh
just acceptance-replay acceptance
```

Replay preserves recorded text, structured output, tool results and the order of
captured interface snapshots; it accelerates time through the headless clock.
Text is checked by turn and author, including draft growth without a sequence
change; plain reply tails are also checked in the production transcript.
Reports distinguish a complete captured sequence, a truncated capture and a
terminal-only recording. A historical final response cannot recover streaming
chunks that were never recorded. Replay checks presentation and interactions; it
does not establish that a provider's authentication, network or new responses
work. Normal `go test`, `just check`, CI and startup never invoke a real-model
acceptance run.

The development demo also has a command-only entry point:

```sh
just demo
```

It starts the isolated fixture UI without opening a profile or running a Node,
model or tool. Demo and acceptance switches are absent from the shipped settings.

### Application checks

Use disposable resources for mutations. Verify the current UI and its actions;
these areas are a guide for choosing checks, not additional automated gates.

| Area | Check |
| --- | --- |
| Startup and shutdown | Open the intended profile, restore the window, reject a second profile owner and close with active work or unsaved drafts |
| Conversation | Send, stream, stop, queue guidance, approve tools, answer questions and restore history; verify a split pane's independent draft and focus |
| Resource workspaces | Navigate files, Git, SSH and database views; resize shell columns, switch resources and preserve drafts and captured scope |
| Plugin lifecycle | Open navigation and settings, leave and return, disable or update a package, and verify stale callbacks cannot act on a new view |
| Files and terminals | Attach and preview a file, save with a revision conflict, type in a terminal, resize it and close its view without confusing detach with stop |
| Model transport | Use scripted failures for retry, cancellation and uncertain outcomes; account sign-in and real requests require explicit account acceptance |
| Connections | Disconnect and reconnect a Host, recover observations and inspect an uncertain original request without repeating its effect |
| Diagnostics | Check host metrics, physical-disk scope and plugin heap estimates; hidden diagnostics must stop their own polling |
| Appearance | Check the changed surface in light/dark and English/Simplified Chinese, including focus, menus, translated labels and clipping |
| Packaging | Launch the target artifact with isolated data, load its sealed native resources and exercise the affected packaged feature |

Windows, Linux, native permissions and physical Mobile behavior are checked on
those targets. Cross-compilation and a local headless run are not substitutes.
The release integration package set is maintained in `ci.yml`; tests needing
accounts, downloaded models or external services remain separate.

## Host automation

The Host entry point is `cmd/sailry-host`, built as `sailry-host`. It starts the
same Node as Desktop and never opens an interactive Agent prompt. Service
options are `--data-dir`, `--bind`, `--internet`, repeated `--relay`,
`--pairing-service`, `--bootstrap` and `--telemetry on|off`. Network modes are
mutually exclusive; the default binds loopback and pairing is opt-in.
The installed `sailry` wrapper with no arguments opens a numbered command menu
on a terminal and prints help when input is redirected. Its `link`, `status`,
`start`, `stop`, `restart`, `update` and `help` entries manage the installed Host.
Starting `sailry-host` with service options continues to run the Host in the
foreground. `--version`, `link`, `share`, `users`, `devices`, `submit` and `wait` are
management commands;
`acceptance` is the explicit manual session test runner described above.

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
go build -o .runtime/sailry-host ./cmd/sailry-host
.runtime/sailry-host --data-dir /absolute/path/to/isolated/profile --internet
```

`--internet` listens for direct TCP and QUIC connections. To use a data relay,
pass `--relay` with the authenticated multiaddr printed by a Circuit Relay v2
server. A TCP and a QUIC relay can be configured together using
distinct relay identities. The updater retains an existing service's arguments;
replace a retired HTTPS `--relay` argument with a libp2p multiaddr or `--internet`
before upgrading. Old peer addresses need refreshing after both ends upgrade.

Build the data relay and test the pairing service in the [Relay repository](https://github.com/sailry/sailry-relay).
`just test-pairing-relay` runs against the adjacent Relay checkout. Pass the checkout path to test another revision.
Harness releases in the migration window package the v0.2.8 bridge from the
Relay commit pinned in `.github/relay-bridge.commit`. The legacy updater accepts
this binary under its original program identity. The next `daemon update`
checks the independent Relay release feed. Publish the pinned Relay source and
independent release assets before issuing a Harness release with the bridge.

`sailry-relay start` runs in the foreground. It reads the file passed with
`--config`, or `./relay.toml` if no path is passed. For a public TCP relay, use
this configuration and replace the example `announce_ip` with the server's
reachable IP address. A wildcard `listen_ip` requires a non-loopback announce IP:

```toml
mode = "public"
listen_ip = "0.0.0.0"
port = 4001
announce_ip = "203.0.113.10"
data_dir = "/var/lib/sailry-libp2p-relay"
```

Keep the absolute private `data_dir` across restarts and upgrades. The default
TCP listener is `127.0.0.1:4001`. With TOML, `--port` is the only override: it
sets both TCP ports for that run without changing the file. Without a TOML file,
pass an absolute `--data-dir` and either `--public` or one or more
`--allow <PeerID>` flags. Full TCP or QUIC-v1 `--listen` and `--announce`
multiaddrs are supported without TOML; `--listen` cannot be combined with
`--port`. Existing `serve` commands keep their flags and do not read TOML.

Public mode accepts standard libp2p peers without enrollment. Private mode uses
`mode = "private"` with `allowed_peers = ["<PeerID>", ...]` (or repeated `--allow`
without a config file); both ends of a relayed circuit must be allowed. Neither
mode changes the Host's PIN pairing or authorization. Token-based and multi-token
relay admission are not implemented. Circuit Relay v2 bounds reservations per IP
and globally, and each circuit's duration and traffic. `id --data-dir <directory>`
prints the existing relay's public PeerID.

On BaoTa, set one Go project's executable to the full path of the extracted
`sailry-relay` and pass `start --config <absolute-config-path>` as arguments.
Use the project's private, persistent state directory and let BaoTa supervise
restarts and boot startup. Do not also install the relay's systemd unit. Make
the TCP port reachable through any firewall or NAT in front of the relay; no
relay command opens ports for you.

On Linux with systemd and no other manager, run `sudo ./sailry-relay daemon
install --config ./relay.toml` from the extracted package. Without `--config`,
the installer uses `/etc/sailry-relay/relay.toml` if present, otherwise
`./relay.toml`. It installs `/usr/local/bin/sailry-relay` and
`/etc/systemd/system/sailry-relay.service` without taking over an existing
service or replacing an existing managed config. The unit runs as `sailry-relay`,
starts at boot, and restarts after an exit with a five-second delay.

For a managed relay, choose an absolute `data_dir` such as
`/var/lib/sailry-libp2p-relay`, not a managed file or its containing directory.
The parent directories must already exist, be root-owned and be neither
symlinks nor writable by group or other users. The installer creates the state
directory if absent. A directory it reuses must be owned by the dedicated
`sailry-relay` user and group with mode `0700`, and must be empty or contain
only the existing private `link.key`.

To change the managed configuration, edit `/etc/sailry-relay/relay.toml` in
place and run `sudo /usr/local/bin/sailry-relay daemon restart`. The service
reads the installed file, not a separate source copy supplied at install time.
`daemon start`, `stop`, `restart` and `status` operate on the installed unit;
only `status` can run without root. `stop` does not disable the service at the
next boot.

Once the assets are published, `sudo /usr/local/bin/sailry-relay daemon update`
checks the public GitHub Release for a newer stable version and verifies its
Linux architecture archive against the published SHA-256. It does not use
GitHub tokens or support multi-token fallback. For an offline update, use
`sudo sailry-relay daemon update <binary> --sha256 <digest>` with the SHA-256
of the extracted executable, not the archive. If the online check finds no
newer release, it leaves the service unchanged. Applying either update restarts
a running relay or starts a stopped one, then authenticates its existing PeerID
with a libp2p probe. If activation fails, the updater restores the previous
binary and attempts to return the service to its prior running or stopped
state. The update does not write the managed `relay.toml` or the relay's
`link.key`.

`sailry link --data-dir <profile>` selects direct connections or a relay, saves
the selection with a revision check, and publishes a private six-digit pairing
code. The scriptable form is `sailry link --data-dir <profile> --relay
<preset-id|direct|custom> [--address <relay address>] [--pairing-service <HTTPS origin>]`.
Only Direct (LAN) and Custom relay are built in. Named relays come from the
[official libp2p directory](https://sailry.dev/catalogs/network-presets-libp2p.json).
For a custom relay, enter a hostname or IP from that directory, optionally with
its port (e.g. `relay.example.com:4001`). A hostname outside the directory can
publish `dnsaddr=<peer-addressed multiaddr>` at `_dnsaddr.<hostname>`. The input
must identify one address. Without a loaded directory or a suitable DNS record,
use the full libp2p multiaddr. Saved addresses do not depend on the directory.
An optional `https://` prefix is input shorthand, not the relay transport.
`sailry share --data-dir <profile>` remains an alias that publishes a code using
the current network selection. The code display refreshes in place with its
remaining lifetime. Pairing from a controller uses the relay route in the Host's
invitation; it does not change the controller's own network selection. Keep PINs,
invitation tickets, profile contents and private endpoint output out of logs
intended for distribution.

Pairing determines the peer's role from its authenticated `has_node` declaration.
Mobile shells use the entry's local user; peers hosting a Node receive their own
controller identity. Invitations do not select or merge users.
`users list` and `devices list` inspect membership. `devices revoke <device id>
--confirm-last` explicitly permits leaving a user's data without device access.
Revocation accepts `--request-id <id>` and returns the request ID with its JSON
result; an uncertain result is recovered with that same ID.

Management talks to the running Node through a profile-private owner-only Unix
socket with a v1 envelope. It never starts another persistence owner. Submit uses
ordinary durable admission; wait observes without resubmission or approving
tools. Uncertain submission recovers with the original request ID. Cancelling
observation does not stop admitted work. Local control commands therefore require
a Unix Host; Windows keeps the service transport but has no substitute control
socket.

`submit` chooses an existing session or a registered worktree and accepts one of
`--prompt`, `--prompt-file` or `--stdin`. New worktree tasks may supply captured
configuration, model/team execution, title and association. `--no-wait` returns
after admission. `wait --session <id> --turn <id>` observes that exact task.
Both commands emit one JSON result. Exit codes are 0 for admitted/completed, 1
for failed/stopped, 3 for approval/input/paused, 4 for uncertain, 124 for timeout
and 130 for cancelled observation.

Add Host installs the published release of `link.ProductVersion` on the remote
machine: the bootstrap downloads `install-host.sh` and the target's archive from
the release feed and verifies both against their checksum files. To install a
locally built Host instead, place `sailry-host` and its staged Turso library
and native seal under
`host-artifacts/<goos>-<goarch>/` beside the controller executable, or set
`internal/node/host.ArtifactRoot` at build time; when that directory holds
`sailry-host` the execution Node verifies and uploads those artifacts through
authenticated SSH after exact host-key trust. Isolated deployments need local
artifacts. Local and SSH installers serialize changes to the same program
location. Updates preserve the managed profile and paired Node identity. Inspect
an uncertain install or update with its original request before any new operation.

## Licensing

Contributions follow this repository's Apache-2.0 license and contribution
agreement. Imported libraries, source, fonts, icons, themes and plugin resources
retain their own copyright, provenance and license notices; the application
license does not replace them. Do not commit private profiles, credentials,
logs or generated runtime output.
