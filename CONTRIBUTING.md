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

Each key is defined in one source file per language. Edit its existing definition
when changing copy; the generator rejects duplicate keys.

The desktop entry point is:

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
export SAILRY_HELPERS_DIR="$PWD/.runtime/native/$(go env GOOS)-$(go env GOARCH)/helpers"
go run ./cmd/sailry-desktop --data-dir .runtime/profiles/desktop
```

The helper directory points source runs at the staged computer-use helper;
`go run` places its executable in a temporary directory without bundled helpers.
`just desktop` supplies this path automatically.

The desktop starts one local Node, Client and shared Network. `--preview` exits
backend startup before opening any profile, including an explicit `--data-dir`.
A Node without a saved network selection starts on the repeated `--relay` URLs,
else on the `iroh_relays` of its desktop preferences, else on the public relays.
Additional live windows share the process backend, remote Clients and profile
preference authority. Each window closes its own observations and forwards;
application shutdown closes Network before the local Client and Node. MyGo keeps
window bounds and `shell.Prefs` in its application-data folder. For implementer
UI checks, isolate that folder with MyGo's path override in a disposable harness.

With [just](https://just.systems) installed, `just --list` shows recipes for these
commands with the same environment: `just preview`, `just desktop <name>` and
`just host <name>` (profiles under `.runtime/profiles/<name>`), `just check` for
the standard Go checks, and the test, plugin, pairing-service and Mobile recipes.

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
| `docs/` | Public user guides linked from the README and published on the website |
| `schemas/` | Published v1 plugin contract schemas and the Agent Plugins manifest schema |
| `third_party_licenses/` | Index of third-party notices, shared license texts and the Go module list |
| `justfile` | Local recipes for the commands in this document |
| `sdk/` | The v1 plugin contract and its declarations |
| `internal/{protocol,link,client,core,node,...}` | Backend packages ([ARCHITECTURE.md](ARCHITECTURE.md#components-and-dependencies)) |
| `cmd/sailry-host`, `cmd/sailry-mobile`, `internal/mobile`, `apps/mobile` | Host and Mobile boundaries |
| `native/` | Pinned native libraries and their build inputs |
| `services/pairing-relay` | Short-code rendezvous service, separate from the encrypted iroh data relay |
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
| `internal/node`, `internal/client` or `internal/link` | Before a release, `CGO_ENABLED=1 go test -race` on the changed packages |

The mobile C ABI and local race detector are the documented cgo exceptions;
ordinary Desktop and Host code uses `CGO_ENABLED=0`. Native artifacts belong
under `.runtime/native/<goos>-<goarch>/`, never in commits. Native Rust builds
use their committed lockfiles and keep toolchain and target settings stable.
Reuse those artifacts for Go development. Cargo runs again only when the native
source, C ABI, dependency pins or target changes.
`sh native/iroh/build.sh` builds the local Host/Desktop library into that
directory. Use the pinned Rust toolchain in `PATH`; do not substitute another
Cargo implementation.

The Flutter build runs the Mobile adapter build itself: Gradle's
`prepareSailryNative` task and the Xcode `Build Sailry Native` phase call
`cmd/sailry-mobile/build.sh`, which builds the Go C ABI and iroh for each
target slice into `.runtime/native/`. It needs Flutter at the version in
`apps/mobile/.flutter-version`, the Rust targets of the slices, and the
Android NDK (`ANDROID_NDK_HOME`) or Xcode. The same script builds one slice:

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

## Test conventions

- Test Sailry logic and contracts: mappings, state transitions, protocol fixtures,
  storage transactions and scripted model turns. Libraries own their own tests.
- Put a regression at the layer that owns the defect. Avoid repeating the same
  assertion through Store, Client and UI; use compact table-driven cases where useful.
- Keep default tests deterministic and isolated with `t.TempDir()`, injected
  clocks, local fakes and `net/http/httptest`. User profiles, real provider accounts
  and external services are not default test fixtures.
- Use `t.Parallel()` when a test touches no shared state.
- Real processes, providers, models, iroh, remote storage and OS permission prompts
  belong behind `//go:build integration`. Other-OS spike programs are run on the
  named system by the user or CI; unrun checks are reported as unrun.
- Keep UI tests headless and few. Preserve accepted interaction tests; do not add
  pixel goldens, per-control framework tests or extra end-to-end gates.
- A failing check must be diagnosed and repaired without weakening assertions or
  skipping cases. Reuse passing results until a relevant input changes.
- Plugin behavior tests belong in the plugins repository. Core contract fixtures
  stay package-neutral.

## Continuous integration

The checkout contains four workflows:

`ci.yml` runs Go, native and Mobile checks without packaging or publishing. It runs on
pull requests, on `main` and `sailry-go`, manually, and when `release.yml` calls it:

- `lint`: Go formatting and `go vet ./...` for Linux, Windows and macOS.
- `test`: `go test ./...` with cgo disabled on macOS, Windows and Linux,
  using only Go module/build caches. Pull requests select affected packages;
  branch pushes and release runs test everything.
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

Packaging entry points are `mygo build`, `flutter build`,
`native/*/build.sh` and the Host installer scripts under
`internal/node/host/deployment/`.
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
go run ./native/fetch -seal .runtime/package/out/darwin-arm64/Sailry.app   # the seal step alone
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

A release opens its profile in place using `storage/node.sqlite` and
`storage/knowledge.sqlite`. Recognized databases from another storage engine move
unopened into `storage/rust-archive/`; their sessions and settings are not imported.
Plugin packages and attachments are copied into that archive before the database
files move, so current storage can collect unreferenced resources safely. Node
identity, controller preferences, themes and file trees remain in place.
`cmd/sailry-desktop/upgrade_integration_test.go` checks this on a synthetic profile.

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

## Manual acceptance

Manual checks exercise the actual application and packaged runtime. Select the
areas affected by a change and record the build, operating system, result and
limitations. A historical checklist or passing headless test does not establish
current platform acceptance.

Development runs use `just desktop` or another isolated profile. The user's
acceptance profile is `~/.sailry-go`; the installed release profile is `~/.sailry`.
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
`--version`, `share`, `submit` and `wait` are management commands;
`acceptance` is the explicit manual session test runner described above.

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

Contributions follow this repository's Apache-2.0 license and contribution
agreement. Imported libraries, source, fonts, icons, themes and plugin resources
retain their own copyright, provenance and license notices; the application
license does not replace them. Do not commit private profiles, credentials,
logs or generated runtime output.
