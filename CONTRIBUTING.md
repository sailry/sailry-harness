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
delivery and immutable skill resources. Remote Link, Host, Mobile and plugin
runtimes arrive in their assigned phases.
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
- Later phases add pinned native libraries, Flutter, Node.js for plugin tests and
  the unchanged TypeScript pairing service. Those are not B0 prerequisites.

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

The desktop starts one local Node and Client. `--preview` exits backend startup
before opening any profile, including an explicit `--data-dir`. Additional live
windows share the process backend and profile preference authority. MyGo keeps
window bounds and `shell.Prefs` in its application-data folder. For implementer
UI checks, isolate that folder with MyGo's path override in a disposable harness.

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
| `sdk/` | Preserved v1 plugin contract and declarations; Go runtimes arrive in B8/B9 |
| `internal/{protocol,link,client,core,node,...}` | Backend packages added only by their assigned phases |
| `cmd/sailry-host`, `cmd/sailry-mobile`, `internal/mobile`, `apps/mobile` | Host and Mobile boundaries added in B7 |
| `native/` | Pinned native libraries and their phase-specific build inputs |
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
  B14 is 400 test functions, including existing UI tests. B0 adds zero tests;
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

B0 adds no workflow. B14 supplies one `.github/workflows/ci.yml`, running only
after the user pushes:

- `lint`: Go formatting and `go vet ./...`.
- `test`: `go test ./...` with cgo disabled on macOS, Windows and Linux,
  using only Go module/build caches. Pull requests select affected packages;
  main and release runs test everything. Use one macOS job per run.
- `native`: build changed or repinned C-ABI libraries and upload their artifacts.
  Test and release jobs consume those artifacts when required.
- `apps/mobile`: Flutter analysis and tests only for Mobile input changes.

Integration checks run manually or on release. Do not add extra workflows,
nightly fuzzing, coverage gates, per-test matrices or retry-on-flake wrappers.

## Release packages

Release work is B14. Allowed packaging tools are `mygo build`, `flutter build`,
the specified `native/*/build.sh` scripts and the ported Host `release.sh`.
Signing credentials, profiles, binaries and native artifacts never enter Git.

B2 selected the prescribed SQLite fallback after the Turso contention load
failed. It refuses Turso profiles, performs no import and cancels release option A.
Source builds keep `~/.sailry-go`. Release packaging must preserve a separate
profile rather than opening `~/.sailry`; the user delegated unresolved
implementation decisions to the coordinator. Updater feed compatibility, bundle identity
and signing acceptance remain later-phase work.

## Host automation

Host implementation is B7. The target binary is `cmd/sailry-host`, with
`--version`, `share`, `submit` and `wait` matching the cited Rust options.
These commands are not available before B7.

Management talks to the running Node through a profile-private owner-only Unix
socket with a v1 envelope. It never starts another persistence owner. Submit uses
ordinary durable admission; wait observes without resubmission or approving
tools. Uncertain submission recovers with the original request ID. Cancelling
observation does not stop admitted work.

## Licensing

Contributions follow the Sailry reference's Apache-2.0 terms and contribution
agreement. Imported libraries, source, fonts, icons, themes and plugin resources
retain their own copyright, provenance and license notices; the application
license does not replace them. Do not commit private profiles, credentials,
logs or generated runtime output.
