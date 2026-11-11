# Sailry engineering rules

Read [ARCHITECTURE.md](ARCHITECTURE.md) before changing code. Build, test, CI and
release procedures are in [CONTRIBUTING.md](CONTRIBUTING.md); package rules are in
`plugins/AGENTS.md` and `themes/AGENTS.md`.

## Scope

- Finish all authorized backend phases in order against their acceptance criteria.
  The user has authorized continuous implementation through B14 without pausing
  for phase confirmation. Do unlisted work only as a prerequisite or blocking
  defect; optional improvements and later-phase work are not completion gates.
- The Go module and repository are `github.com/sailry/sailry-harness`.
- Development is self-contained. Locale sources and SDK declarations live here;
  official packages come from the pinned submodules. Do not depend on a sibling
  checkout or retain old repository paths in source comments.
- Follow the architecture and accepted UI. Connect data and actions without
  changing layout, styling, copy, view-model field names or `Actions` contracts.
  A refactor does not authorize deleting existing functions.
- Each feature's contract lives in `docs/<feature>.md`. Update its backend wiring,
  ownership and deviations when behavior changes. Internal plans are not source.

## Git and workspace

- Check `git status` first. Preserve uncommitted files you do not own and stage
  only your own. Commit reviewable changes locally with imperative English
  subjects. Never push or open a pull request unless the user requests it.
- The coordinating agent owns planning, integration and final validation. Use
  parallel agents for independent development, research or review; use isolated
  worktrees when concurrent edits could conflict. Each child performs only static
  checks on its own changes. After integrating a stage, the coordinator runs the
  relevant compilation and tests once by category.
- Reuse fixed-purpose `.runtime` directories and worktree paths, without run IDs,
  timestamps or UUIDs. Inspect an invalid cache and stop its owner before cleanup.
  Never clean user profiles, plugin or theme repositories, unrelated source or
  another agent's changes.
- Official plugins and themes are separate submodules. Preserve their reviewed
  Gitlink pins; do not substitute remote tips or modify packages during an
  application-only phase.

## Environment and validation

- Before every Go command, use `unset GOROOT` and
  `export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0`. MyGo stays at the exact version in
  `go.mod`; do not upgrade it or modify its source.
- The coordinator runs the complete gate before every code commit:
  `test -z "$(gofmt -l .)"`, `go build ./...`, `go vet ./...`,
  `go test ./...`. Conditional mobile, native and race checks are listed in
  [CONTRIBUTING.md](CONTRIBUTING.md#checks).
- Choose additional checks from the change and dependency boundaries. Reuse passing
  results until relevant code, dependencies, configuration or environment changes.
  Diagnose failures; never weaken assertions or report an unrun check as passed.
- Reuse built native libraries. Rebuild Rust only when its source, C ABI,
  dependency pins or target changes; Go-only development does not rerun Cargo.
- Tests use `t.TempDir()`. Manual backend runs use
  `--data-dir .runtime/profiles/<fixed-name>` once B2 adds that flag. Never use
  `~/.sailry`; `~/.sailry-go` is reserved for the user's acceptance runs.
- Other-OS checks use `//go:build integration` programs for the user or CI to run
  on those systems. Local compilation does not prove another platform's behavior.

## Language and product copy

- Source, identifiers, comments, logs, diagnostics, developer errors and public
  docs are English. User-facing phase reports are Chinese. Other languages live
  only in i18n resources under stable keys or intentional Unicode fixtures.
- UI copy belongs in `internal/desktop/i18n/keys.txt` and both
  `locales/en.yml` and `locales/zh-CN.yml` in that package. Maintain their source
  catalogs under the root `locales/` directory and preserve the existing keys.
- Copy is minimal: short labels, direct actions, consistent terms, no filler or
  terminal punctuation on short UI text. Open prompts use wording such as
  "Describe your task".
- Operation outcomes appear only as toasts, never repeated in pages or dialogs or
  replacing content or drafts. Toasts distinguish failure, conflict and uncertain
  outcomes. Destructive and consent prompts name the target and consequence once.

## MyGo and Kit

- Use pinned MyGo controls and `internal/desktop/kit`, with one application theme
  and semantic tokens. No page-local palettes, one-to-one wrappers or copied
  framework internals.
- Lobe MyGo is a source reference for already adapted Kit components, not a module
  dependency. Preserve imported notices.
- CrychicUI is allowed only for the approved gaps in
  [docs/crychic.md](docs/crychic.md); Kit wins when both have a component.
- Custom rendering requires a verified framework gap and an explanation in the
  feature's deviations. Right-click menus are OS-native; left-click menus use
  Kit menus or popovers.
- Approved layouts are the baseline. Removed controls must not return through
  backend work.

## Code organization

- Organize by feature in the owning package; keep entry points and shell routing
  thin. UI packages live under `internal/desktop`. Backend code stays outside it;
  `internal/desktop/live` owns projection and action adapters when B3 lands.
- Preserve the dependency boundaries in ARCHITECTURE.md. Do not add single-
  implementation interfaces, pass-through wrappers or speculative abstractions.
  Add a platform adapter only for a verified gap.
- Use the libraries named by the backend plan. Do not hand-roll infrastructure,
  add unlisted retries or timeouts, revalidate our own data or duplicate checks a
  library already enforces. A failed spike uses only its documented fallback.
- The user's November 10, 2026 B8 decision replaces gocron with a direct port of
  the Rust dispatch calendar, standard production timers and clockwork tests.
  This source port is explicitly authorized. The later decision that day
  replaces QuickJS with pinned moejs v0.1.0 after compatibility and resource
  checks. Keep the 64 MiB callback allocation budget, 60-second deadline,
  cancellation and Rust-compatible recursion fault without crashing the host.
  Use the engine's own recursion bound; do not claim a physical stack-byte limit.

## Dependencies and versions

- Pin dependencies in `go.mod` and commit `go.sum`. Native libraries keep their
  own exact pins and lockfiles. Desktop and Host use pure Go plus purego; only
  `cmd/sailry-mobile` contains cgo for the Dart C ABI.
- No alternative desktop stack, agent loop or JavaScript engine without an
  authorized architecture change. Fantasy supplies per-step model transport;
  Sailry owns the loop. `github.com/MoeForge/moejs v0.1.0` is the one plugin
  JavaScript engine for Node callbacks, model translation and Desktop entries.
- Sailry-owned contracts stay at `v1` and change in place until the first release
  ([details](ARCHITECTURE.md#contract-and-storage-versions)).
- New Sailry plugin packages start at `0.1.0`. Changed content bumps SemVer,
  usually the patch. Only strictly newer precedence offers an update; same-version
  changes, build metadata and older versions do not. Preserve installed
  configuration, enable choices and removed records.

## Product boundaries

- Preview data is clearly identified and never triggers models, tools, pairing,
  terminals or user-data changes. All B0 surfaces remain preview-only.
- Mobile is a Flutter controller with Client, Link and a thin Dart FFI adapter.
  A desktop-only Go test is not Mobile acceptance; desktop tasks do not expand
  into Mobile work.
- Usage statistics cover only Sailry's own sessions. An enabled model plugin may
  show its provider's account status separately. Never collect third-party tool
  histories or analyze third-party billing.
- Plugins own their UI and workflows; core provides scoped services
  ([details](ARCHITECTURE.md#extensions-and-plugins)).

## Safety

- Keep safeguards proportional: authenticated access, path confinement, credential
  protection, revision conflicts and destructive-action confirmation. No redundant
  approvals or speculative permission systems. Saves keep drafts on failure and
  report real conflicts or uncertain outcomes.
- No destructive operations on user projects, sessions, credentials or databases.
- Never commit `.runtime`, `.preview`, credentials, `.npmrc`, local toolchain
  overrides, native libraries, build output or private endpoint data.
