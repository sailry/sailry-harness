# Contributing

Read [ARCHITECTURE.md](ARCHITECTURE.md) and [AGENTS.md](AGENTS.md) before changing
code. The Go module and repository are `github.com/sailry/sailry-harness`.

## Scope and status

The accepted desktop preview remains available through `--preview`. B1 supplies
the v1 protocol, B2 the isolated profile and SQLite storage, and B3 the local
Node, Client and live sidebar, notifications and preferences. B4 adds captured
model transport and canonical agent execution; B5 adds workspace tools, Git,
verified transfers, sandboxed commands, Web resources and hooks with live native
resource panels. B6 connects frozen teams and roles, child execution, background
delivery and immutable skill resources. B7 supplies remote Link, pairing, Host
control, SSH Host deployment, metrics, port forwarding and the Flutter Go
adapter. Plugin runtimes and the remaining resource services arrive in their
assigned phases. Each platform's runtime acceptance is recorded separately.
Implement phases in order, with centralized checks and a report per phase.
The user has authorized continuing through B14 without confirmation pauses.

The checkout is self-contained: locale sources live in `locales/`, plugin SDK
declarations in `sdk/`, and official packages in the pinned submodules. Setup,
generation, builds and tests do not read another project directory.

Maintainers review feature, UI, protocol, storage and architecture changes before
implementation. For fixes, include a reproduction and evidence of the corrected
behavior. Keep local commits cohesive with imperative English subjects. Agents
commit locally and never push or open pull requests unless explicitly requested.

## Setup

Requirements:

- Go 1.27.3 and Git.
- The MyGo version pinned in `go.mod`:
  `v0.8.4-0.20261109104234-eadf1fce6af3`. Do not upgrade it or edit its source.
- Platform-native runtime dependencies required by that MyGo version.
- B7 Link requires the pinned Rust 1.98.0 toolchain to build iroh 1.3.0. Mobile
  requires Flutter and the target's Android NDK or Apple SDK; the preserved
  TypeScript pairing service uses its own Node.js package tooling.
- Later phases add their pinned native runtimes and plugin tooling.

Use this environment before every Go command:

```sh
cd /path/to/sailry-harness
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
```

Initialize the reviewed submodule snapshots:

```sh
git submodule update --init --recursive
```

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

The source catalogs include the accepted desktop additions in `zz-desktop.*.yml`.
Those entries keep the current UI wording when shared catalogs use other labels.

The desktop entry point is:

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
go run ./cmd/sailry-desktop
```

The desktop starts one local Node, Client and shared Network. `--preview` exits
backend startup before opening any profile, including an explicit `--data-dir`.
Additional live windows share the process backend, remote Clients and profile
preference authority. Each window closes its own observations and forwards;
application shutdown closes Network before the local Client and Node. MyGo keeps
window bounds and `shell.Prefs` in its application-data folder. For implementer
UI checks, isolate that folder with MyGo's path override in a disposable harness.

With [just](https://just.systems) installed, `just --list` shows recipes for these
commands with the same environment: `just preview`, `just desktop <name>` and
`just host <name>` (profiles under `.runtime/profiles/<name>`), `just check` for
the complete gate, and the test, plugin, pairing-service and Mobile recipes.

Implementer runs use `--data-dir .runtime/profiles/<fixed-name>`. Development defaults to
`~/.sailry-go` only for user acceptance. Never run against `~/.sailry`; it is the
installed release profile. Tests use `t.TempDir()`.

## Layout

| Path | Responsibility |
|---|---|
| `cmd/sailry-desktop` | Desktop composition, shared local Node and explicit preview startup |
| `internal/desktop/{shell,sidebar,conversation,composer,panel,panes,settings,host,transfers,onboarding}` | Accepted UI, state machines, view models and actions |
| `internal/desktop/{kit,theme,i18n,icons,viewmodel}` | Shared UI components, themes, copy, icons and neutral presentation contracts |
| `internal/desktop/{live,preferences,preview}` | Live action adapters, profile controller settings and isolated simulation |
| `bundled.go` | Root embedded themes and plugin snapshot from pinned submodules |
| `locales/` | Source catalogs for local i18n generation |
| `docs/*.md` | Feature contracts, ownership, backend wiring and deviations |
| `docs/guides/` | Public user guides linked from the README |
| `schemas/` | Published v1 plugin contract schemas and the Agent Plugins manifest schema |
| `third_party_licenses/` | Index of third-party notices, shared license texts and the Go module list |
| `justfile` | Local recipes for the commands in this document and `docs/mobile.md` |
| `sdk/` | Preserved v1 plugin contract and declarations; Go runtimes arrive in B8/B9 |
| `internal/{protocol,link,client,core,node,...}` | Backend packages added only by their assigned phases |
| `cmd/sailry-host`, `cmd/sailry-mobile`, `internal/mobile`, `apps/mobile` | Host and Mobile boundaries added in B7 |
| `native/` | Pinned native libraries and their phase-specific build inputs |
| `services/pairing-relay` | Preserved short-code rendezvous service, separate from the encrypted iroh data relay |
| `.runtime/` | Ignored fixed-purpose experiments, libraries and test profiles |

B0 retains `settings.Stored[T]`, `shell.Prefs` and callback-driven window
invalidation. Nodra is not adopted because its prototype did not reduce code.

## Checks

After integrating each development stage, the coordinating agent performs the
relevant compilation and tests by category. Child agents perform only static
checks on their own changes. Use worktrees for edits that could conflict, and
keep each worktree's scope explicit.

The complete gate runs before every code commit:

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
test -z "$(gofmt -l .)"
go build ./...
go vet ./...
go test ./...
```

Additional checks apply only when their inputs change:

| Change | Additional check |
|---|---|
| `internal/mobile` or `cmd/sailry-mobile` | `CGO_ENABLED=1 go vet ./cmd/sailry-mobile` |
| `native/` | `cargo build --locked` in the changed native project |
| `apps/mobile` | `flutter analyze` and `flutter test` there |
| `internal/node`, `internal/client` or `internal/link` | Once per phase, `CGO_ENABLED=1 go test -race` on the changed packages |

The mobile C ABI and local race detector are the documented cgo exceptions;
ordinary Desktop and Host code uses `CGO_ENABLED=0`. Native artifacts belong
under `.runtime/native/<goos>-<goarch>/`, never in commits. Native Rust builds
use their committed lockfiles and keep toolchain and target settings stable.
Reuse those artifacts for Go development. Cargo runs again only when the native
source, C ABI, dependency pins or target changes.
`sh native/iroh/build.sh` builds the local Host/Desktop library into that
directory. Use the pinned Rust toolchain in `PATH`; do not substitute another
Cargo implementation. Target Mobile build inputs and ABI ownership are
described in [Mobile](docs/mobile.md).

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

## Test conventions

- Test our logic and contracts: pure mappings, state transitions, captured Rust
  JSON round-trips, storage transactions and a scripted fake model's agent loop.
  Libraries own their own tests.
- Keep one assertion at one layer. Do not port Rust tests one for one or repeat
  store assertions through Client and UI. Prefer compact table-driven cases.
- Each phase names its tests and target count. The full repository ceiling by
  B14 is 400 default-run test functions, including existing UI tests; tests
  behind `//go:build integration` are not counted. B0 adds zero tests;
  existing test bodies and assertions stay unchanged while imports and obsolete
  source comments follow the package moves.
- Keep default tests deterministic and isolated. Use `t.TempDir()`, injected
  clocks, local fakes and `net/http/httptest`. No sleeps, real timers, live
  network, user profiles or real child processes in the default suite.
- Use `t.Parallel()` when a test touches no shared state. The default run budget
  is two minutes cold on an M-series Mac and 20 seconds warm with the Go cache.
- Real processes, providers, models, iroh, remote storage and OS permission prompts
  belong behind `//go:build integration`. Other-OS spike programs are run on the
  named system by the user or CI; unrun checks are reported as unrun.
- Keep UI tests headless and few. Preserve accepted interaction tests; do not add
  pixel goldens, per-control framework tests or extra end-to-end gates.
- A failing check must be diagnosed and repaired without weakening assertions or
  skipping cases. Reuse passing results until a relevant input changes.
- Plugin behavior tests belong in the plugins repository. Core contract fixtures
  stay package-neutral; B8/B9 add only their specified runtime coverage.

## Continuous integration

Workflows run only after the user pushes. There are four:

`ci.yml` holds every check and nothing that packages or publishes. It runs on
pull requests, on main, manually, and when `release.yml` calls it:

- `lint`: Go formatting and `go vet ./...` for Linux, Windows and macOS.
- `test`: `go test ./...` with cgo disabled on macOS, Windows and Linux,
  using only Go module/build caches. Pull requests select affected packages;
  main and release runs test everything. Use one macOS job per run.
- `native`: build `native/iroh` and `native/capture` with their `build.sh` for
  Linux and macOS on both architectures and for Windows x64 (Git Bash, MSVC)
  when either changes, and upload `native-<goos>-<goarch>` artifacts: the two
  libraries and their `SHA256SUMS`, the directory `native/fetch -artifacts`
  reads. Release and manual runs always build them. The Linux legs set
  `SAILRY_GLIBC=2.34`, so `native/iroh/build.sh` links the iroh library against
  that GNU libc through cargo-zigbuild, then check that the library needs
  nothing newer and that `internal/node/wsl/preflight.sh` names the same
  version.
- `apps/mobile`: Flutter analysis and tests only for `apps/mobile` or
  `internal/mobile` changes.
- `integration`: never on pull requests or main. Manually with the
  `integration` input it tests `integration_packages`; left empty, and in a
  release, it tests the release set named in the workflow: packages that need
  no downloaded model and no external service. Each leg first verifies that
  run's native artifact and loads its iroh library.

`release.yml` runs on a `v*` tag. Every job has read-only repository access
except `publish`:

- `version`: the tag must equal `link.ProductVersion`, the `mygo.json` version
  and the `pubspec.yaml` version, and `CHANGELOG.md` must hold the entry.
- `checks`: all of `ci.yml`, with the integration tests.
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

`pairing-relay.yml` tests `services/pairing-relay` when it changes; a manual
run tests and then deploys to the chosen EdgeOne environment. `website.yml`
asks the website repository to rebuild when the documents it reads change on
main.

Actions are pinned by commit. Do not add nightly fuzzing, coverage gates,
per-test matrices or retry-on-flake wrappers.

## Release packages

Release work is B14. Allowed packaging tools are `mygo build`, `flutter build`,
the specified `native/*/build.sh` scripts and the ported Host installer files
under `internal/node/host/deployment/`.
Signing credentials, profiles, binaries and native artifacts never enter Git.

`go run ./native/fetch` stages one target's pinned native artifacts in
`.runtime/native/<goos>-<goarch>/` after checking each download against the
lock files (`native/*.lock.json` and
`internal/localmodels/decision/llama-runtime.lock.json`). The iroh and capture
libraries come from `native/*/build.sh` on the target, or from the CI native
job with `-artifacts <dir>`; `-bundle <dir>` copies the shipped set and `-host`
leaves out the speech and capture libraries.

The Desktop package is described by `cmd/sailry-desktop/mygo.json`; its version
equals `link.ProductVersion`. Shipped files sit beside the executable, or in
`Contents/Resources` of the macOS bundle, where every loader reads them.

A release build is one linked with
`-X github.com/sailry/sailry-harness/internal/core.Default=~/.sailry`
(`MYGO_LDFLAGS` for `cmd/sailry-desktop`, `go build -ldflags` for
`cmd/sailry-host`). It opens the release profile `~/.sailry` by default and
loads the iroh and capture libraries only from its package, against the
package seal: `SAILRY_IROH_LIB` and `.runtime/native` are read by source
builds only. Source builds keep `~/.sailry-go`.

`cmd/sailry-desktop/macos/package.sh` packages the macOS Desktop on the target
architecture: it stages the natives, the dependency notices (`licenses/`) and
`build.json`, runs `mygo build -sign <identity> -skip-notarize -skip-dmg` with
the release flags, renames the executable to `sailry-desktop`, which is the
name the updater requires, seals, notarizes and creates the disk image
`Sailry-<triple>.dmg` and `Sailry-<triple>-unsigned.zip`, the archive of the
finished application for the update feed.
`cmd/sailry-desktop/windows/package.sh -artifacts <dir>` cross-compiles the
Windows x64 Desktop on Linux or macOS with NSIS installed: the installer
`Sailry-x86_64-pc-windows-msvc-setup.exe` (mygo's per-user NSIS installer) and
the portable `Sailry-x86_64-pc-windows-msvc.zip`.

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
bash cmd/sailry-desktop/macos/package.sh                    # ad hoc, not notarized
go run ./native/fetch -seal .runtime/b14/out/darwin-arm64/Sailry.app   # the seal step alone
```

`go run ./native/fetch -seal <path> [-sign <identity>] [-host]` is the step
after signing. Signing a macOS application rewrites its libraries, so the
upstream pins no longer describe them. The step writes `native-seal.json` (size
and SHA-256 of the ONNX Runtime, sherpa-onnx, capture and iroh libraries as
shipped), rewrites `llama-runtime.json` and the file list of
`python/python-runtime.json` from the signed files, and signs the bundle again
without `--deep` so the code signature covers these records. The loaders use
the seal in place of the upstream pins only in `Contents/Resources` of the
running bundle; an invalid seal there is an error. Unsigned and source builds
keep the upstream pins. For a plain directory (the Host archive, Linux,
Windows) the step only writes `native-seal.json` for the iroh and capture
libraries, which have no upstream pin. The loaders compare hashes; the code
signature itself is enforced by macOS, not by Sailry.
`cmd/sailry-desktop/package_integration_test.go` checks a built package's
loaders from inside it: compile it with the release `-ldflags`, copy it beside
the packaged executable and seal the bundle again.

Signing uses the settings of the existing release repository unchanged.
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
bundled Python interpreter keeps `disable-library-validation`. Windows packages
are not signed: the existing release settings hold no Windows certificate, and
`windows.certificate` and `windows.signCommand` stay unset in `mygo.json`.

B2 selected the prescribed SQLite fallback after the Turso contention load
failed. On November 11, 2026 the user decided that releases open `~/.sailry` in
place. The Go Node creates `storage/node.sqlite` and `storage/knowledge.sqlite`
and moves an earlier release's database files into `storage/rust-archive/`
without opening, importing, changing or removing them; sessions and settings
stored in them do not appear. It keeps the
Node identity (`link.key`), `desktop/preferences.json`, installed themes and
the profile's file trees. `cmd/sailry-desktop/upgrade_integration_test.go`
checks this on a synthetic profile.

The Desktop update feed is written by
`go run ./internal/desktop/updater/cmd/update-feed -version <version> -output
<dir> <target>=<unsigned zip>...` with the publisher key in
`SAILRY_UPDATE_SIGNING_KEY`, which must belong to
`internal/desktop/updater/update-public-keys.txt`. For both macOS targets it
writes the signed `Sailry-<triple>.zip`, its `.zip.update.json` and the signed
catalog `update-v1.json`, then checks them with the updater's own
verification. `TestPublishedRelease` proves the format with a throwaway key.
The updater has no Windows installer, so the feed covers macOS only.

`internal/node/host/deployment/archive.sh [-artifacts <dir>]` assembles the
Host archive of the machine it runs on. Each archive
`sailry-host-<triple>.tar.gz` (`x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-apple-darwin`)
holds `sailry-host`, the iroh library, the pinned runtimes of
`native/fetch -host`, the files of `internal/node/host/deployment/package/`,
`build.json`, the dependency lock files and `licenses/` from
`internal/node/host/deployment/licenses.sh`, whose optional fourth argument
names the command package. On macOS only `sailry-host` and the iroh library
are signed: the pinned runtimes keep the bytes their locks record, so the
archive is not notarized. The release carries `install-host.sh`,
`SHA256SUMS-installer`, the archives and one `SHA256SUMS-<triple>` each. SSH
Add Host, its update and the WSL installation download exactly those names
from `https://github.com/sailry/sailry-harness/releases/download/v<version>/`,
so they work once the draft release is published.
`internal/node/host/deployment/SOURCE` records where the ported files come from.

## Host automation

The Host entry point is `cmd/sailry-host`, built as `sailry-host`. It starts the
same Node as Desktop and never opens an interactive Agent prompt. Service
options are `--data-dir`, `--bind`, `--internet`, repeated `--relay`,
`--pairing-service`, `--bootstrap` and `--telemetry on|off`. Network modes are
mutually exclusive; the default binds loopback and pairing is opt-in.
`--version`, `share`, `submit` and `wait` are management commands.

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
go build -o .runtime/sailry-host ./cmd/sailry-host
.runtime/sailry-host --data-dir /absolute/path/to/isolated/profile --internet
```

The running binary loads its target's iroh library through the shared loader.
`share --data-dir <profile>` asks the existing owner to publish a private
six-digit pairing code. Keep PINs, invitation tickets, profile contents and
private endpoint output out of logs intended for distribution.

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
locally built Host instead, place `sailry-host` and its iroh library under
`host-artifacts/<goos>-<goarch>/` beside the controller executable, or set
`internal/node/host.ArtifactRoot` at build time; when that directory holds
`sailry-host` the execution Node verifies and uploads those artifacts through
authenticated SSH after exact host-key trust. Isolated deployments need local
artifacts. Updates preserve the managed profile and paired Node identity. An
uncertain install or update must be inspected with its original request before
any new operation.

## Licensing

Contributions follow the Sailry reference's Apache-2.0 terms and contribution
agreement. Imported libraries, source, fonts, icons, themes and plugin resources
retain their own copyright, provenance and license notices; the application
license does not replace them. Do not commit private profiles, credentials,
logs or generated runtime output.
