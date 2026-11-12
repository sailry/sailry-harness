# Sailry engineering rules

Read [ARCHITECTURE.md](ARCHITECTURE.md) before changing code. Build, test, CI and
release procedures are in [CONTRIBUTING.md](CONTRIBUTING.md); package rules are in
`plugins/AGENTS.md` and `themes/AGENTS.md`.

## Scope

- Do the work that was asked. Do unrequested work only as a prerequisite or a
  blocking defect; optional improvements are not completion gates.
- The Go module and repository are `github.com/sailry/sailry-harness`.
- Development is self-contained. Locale sources and SDK declarations live here;
  official packages come from the pinned submodules. Do not depend on a sibling
  checkout or retain old repository paths in source comments.
- Follow the architecture and accepted UI. Connect data and actions without
  changing layout, styling, copy, view-model field names or `Actions` contracts.
  A refactor does not authorize deleting existing functions.
- The architecture lives in ARCHITECTURE.md. Explain a deliberate deviation or
  non-obvious ownership in a code comment at the place it applies, and keep it
  true when behavior changes. Internal plans are not source.

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
  application-only change.

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
  `--data-dir .runtime/profiles/<fixed-name>`. Never use
  `~/.sailry`; `~/.sailry-go` is reserved for the user's acceptance runs.
- Other-OS checks use `//go:build integration` programs for the user or CI to run
  on those systems. Local compilation does not prove another platform's behavior.

## Language and product copy

- Source, identifiers, comments, logs, diagnostics, developer errors and public
  docs are English. Reports to the user are Chinese. Other languages live
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
- CrychicUI is the second UI library: prefer a MyGo control, then a verified
  CrychicUI component, then Kit or custom where neither fits or ours does
  more; it may replace ours only where no function is lost, and every floating
  surface stays Kit's frost. A component is verified once the tests of
  `internal/desktop/kit/crychic` probe it.
  - Direct: `color.SwatchGrid`, `color.Palette`, `files.Dropzone`,
    `files.FileCard`, `files.FileIcon`, `files.FileList` (not for the picker),
    `form.FileField`, `form.TypeToConfirm`, `input.TagInput`,
    `feedback.ErrorState`, `feedback.RetryIn`, `feedback.SaveStatus`,
    `feedback.ConnectionStatus`, `chat.JumpToLatest`, `media.Recorder`,
    `devtools.LevelFilter`, `code.DiffView`
  - Through the `kit/crychic` adapter: `agent.ImageGenerating`,
    `agent.SourceCard`, `agent.Citation`, `agent.RunTimeline`,
    `agent.TaskProgress`, `chat.LinkPreview`, `navigation.ZoomControl`,
    `media.Waveform`, `devtools.LogView`, `code.CommitGraph`, `code.Blame`,
    `diagram.Mermaid`, and `desktop.UpdatePrompt` inside a Kit surface
  - Not usable: `color.Picker`, `overlay.DangerDialog`,
    `overlay.ShortcutSheet`, `overlay.WhatsNew`, `overlay.HoverCard` and
    `overlay.Lightbox` (a floating surface of their own);
    `input.KeyValueField`, `input.PasswordField`, `code.BranchPicker` (fixed
    English) and `media.AudioPlayer` (plays no file); the `chart` package
    (charts use MyGo's `ui/chart`)
- Custom rendering requires a verified framework gap and an explanation in a
  comment at the use. Right-click menus are OS-native; left-click menus use
  Kit menus or popovers.
- Approved layouts are the baseline. Removed controls must not return through
  backend work. Rust and the root `locales/` catalogs still carry most of
  them, so before adding any control found in Rust or another reference, check
  the Removed UI list below. An entry returns only when the user asks for it
  by name, and is then deleted here. A removal keeps the command, protocol
  field and stored data. Guard a new entry cheaply: keep its keys out of
  `keys.txt` (`TestCatalogKeys/RemovedControls`) or assert its absence in the
  feature's test.

### Removed UI

- Composer: a host choice or location dialog (choosing a project sets the host)
- `@` and `/` picker: a header, footer, hint or check mark (group headings stay)
- Approval popover: the Deny menu, "Deny with reason" and the reason field;
  the arguments of a tool that declares no display (its name alone)
- Question row: state text such as "waiting for answer" (the icon tells it)
- Tool row: red for an error the tool returned (red only if the call did not run)
- Queue strip: a wait reason on a row ("Held", "Paused", "Not sent" and the
  like); held background results as rows; a paused-queue strip, "Queue paused"
  and Continue sending (a halt is one toast; Send now or a message resumes)
- Attachments: the composer's own file dialog and the system file dialog (Add
  attachment opens the in-app picker)
- Model picker: capability filters and the line over them in the rail, and
  capability marks on a model's row (the score alone ends it); the heading
  over the efforts; a check mark on a chosen effort, model, family or team row (the
  neutral fill marks it; mode and permission menus keep checks); a score card
  or tooltip over a score (the controls page keeps its card)
- File picker: the host's name on a second line under the title (this device
  is never named)
- Connection banner: the Retry button (the Client reconnects by itself)
- Right panel: History and Documents as launcher or "+" entries (the tab kinds
  stay); a preview of PDF, Office or other formats MyGo cannot draw (Open uses
  the system's app, on Mobile too)
- Split workspace: a pane close that stops, closes or removes its session or
  terminal (it only detaches; a terminal pane still confirms)
- Activity strip: the `git` plugin's Status-slot entry (the +/− badge is
  core; `files` and `git` are not mounted in the Status slot)
- Project menu and page: "Manage project trust" and its dialog (a project is
  trusted when added); "Launch AI CLI", its submenu and its button (only the
  `commands` package's slot may bring one back); the Session records dialog
  (the project page lists sessions, archived ones too)
- Host page: a Revoke or Remove pairing button outside a menu, also on host
  rows of Settings › Connections; the Runtime section (the header's line keeps
  the Host and Protocol versions); the Projects section, its Add project
  button and empty state; a process row's letter tile and memory-share bar
- Usage page: the provider tile before a model, the provider's name after it,
  the project tag after a session's title and a cache-rate bar (plain text;
  the tooltip and the copied row keep provider and project)
- Settings › Connectors: an inline Connect button (it is in the row's menu)
- Settings › Dictation: the "local models" heading
- Settings › Providers: a preview note in the editor; Add model as a footer
  row of the model list (it is a button beside Refresh)
- Settings › Conversation: the value beside the text size slider and the
  stepped sizes (Small to Extra large) it replaced; the Sessions group and
  its Session order row (the sidebar's sort menu sets the order)
- Settings › General: the toast that a language change applies after a restart
- Welcome guide: the telemetry or diagnostics note and a Settings button
- Theme packages: transparency or blur values (the Appearance page owns them)

## Code organization

- Organize by feature in the owning package; keep entry points and shell routing
  thin. UI packages live under `internal/desktop`. Backend code stays outside it;
  `internal/desktop/live` owns projection and action adapters.
- Preserve the dependency boundaries in ARCHITECTURE.md. Do not add single-
  implementation interfaces, pass-through wrappers or speculative abstractions.
  Add a platform adapter only for a verified gap.
- Use the libraries already pinned in `go.mod`. Do not hand-roll infrastructure,
  add unlisted retries or timeouts, revalidate our own data or duplicate checks a
  library already enforces.
- Dispatch scheduling is our own calendar on standard timers, tested with
  clockwork; no scheduler library. Plugin JavaScript runs in moejs with a 64 MiB
  callback allocation budget, a 60-second deadline, cancellation and a recursion
  fault that does not crash the host. Use the engine's own recursion bound; do
  not claim a physical stack-byte limit.

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

- Preview and demo data never trigger models, tools, pairing, terminals or
  user-data changes.
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
