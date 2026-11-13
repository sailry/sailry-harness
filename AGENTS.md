# Sailry engineering rules

Read [ARCHITECTURE.md](ARCHITECTURE.md) for the implemented system and
[CONTRIBUTING.md](CONTRIBUTING.md) for development and release commands.
`plugins/AGENTS.md` and `themes/AGENTS.md` govern those separate repositories.

## Scope and source of truth

- Implement the requested work and fix defects found within that scope. Optional
  improvements, new features and new validation gates are not completion criteria.
- Use current code, schemas and accepted behavior as the baseline. Historical
  implementations and internal plans explain provenance; they do not constrain a
  new implementation or authorize restoring removed UI.
- The module is `github.com/sailry/sailry-harness`. This checkout contains its
  locale sources, SDK declarations and pinned official package submodules.
- Keep architecture documentation consistent with code. Explain a non-obvious
  ownership decision where it applies, without copying plans into source comments.

## Ownership and implementation

- Node owns execution, configuration, credentials and canonical history. Client
  owns projection and recovery. Controllers own presentation and drafts.
- Organize code by feature. Keep entry points and shell routing thin;
  `internal/desktop/live` connects Client state and actions to the UI.
- Plugins own their feature workflows and UI; core provides shared scoped
  services. Native components belong at reusable framework or platform boundaries.
- Use pinned libraries and existing infrastructure. Avoid speculative interfaces,
  pass-through layers, duplicated validation, unneeded retries and timeouts.
- Current protocol and storage versions are facts in code, not permanent version
  constraints. Keep changed contracts, schemas, SDK declarations and their callers
  consistent. Preserve stored data and report unsupported input honestly.
- Preserve imported source notices and licenses.

## Git and workspace

- Check `git status` first. Preserve other work and stage only your changes. Make
  cohesive local commits with imperative English subjects. Push or open a pull
  request only when requested.
- The coordinator owns integration and final validation. Delegate independent
  work; use isolated worktrees when edits could conflict. Children perform static
  checks, and the coordinator runs the relevant compilation and tests together.
- Reuse fixed-purpose `.runtime` directories. Before cleanup, inspect ownership
  and active processes. Remove task worktrees after their work is integrated;
  preserve unrelated edits, profiles and repositories.
- Plugins and themes are separate submodules. Keep reviewed Gitlink pins during
  application-only changes. A package change is committed in its repository before
  updating the parent pin; never substitute an unreviewed remote tip.
- Never commit profiles, credentials, `.runtime`, `.preview`, `.npmrc`, private
  endpoints, local toolchain overrides, native libraries or build output.

## Environment and validation

- Before Go commands, use `unset GOROOT` and
  `export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0`. Dependency versions come from
  `go.mod` and lockfiles; do not change them incidentally or edit module-cache code.
- Validate the changed behavior and affected boundaries using the commands in
  [CONTRIBUTING.md](CONTRIBUTING.md#checks). Reuse passing results until relevant
  code, dependencies, configuration or environment changes. Do not add a gate just
  because another check is possible.
- Use deterministic fixtures, scripted models and recorded replay for routine
  checks. Real-model acceptance runs only when explicitly requested. Report unrun
  checks and platform limitations accurately; do not weaken failing assertions.
- Reuse native libraries. Rebuild when native source, C ABI, dependency pins or
  target changes. Go-only work does not require another Cargo build.
- Tests use `t.TempDir()`. Manual development runs use
  `--data-dir .runtime/profiles/<fixed-name>`. `~/.sailry-go` is reserved for user
  acceptance and `~/.sailry` for installed releases.
- Cross-compilation does not establish another operating system's behavior.
  Platform and physical-device acceptance run on the corresponding target.

## UI and product copy

- Preserve the accepted layout and interactions unless the request changes them.
  Data or backend work does not authorize adding controls, labels or dialogs.
- Use MyGo controls and `internal/desktop/kit` with the application theme and
  semantic tokens. Reuse tested CrychicUI components and adapters where they fit.
  Do not copy framework internals, create page-local palettes or wrap controls
  one for one.
- Floating surfaces use Kit's frost. Right-click menus and confirmation-only
  dialogs are OS-native; forms and content dialogs remain Kit surfaces. Shared
  terminal, editor, preview and conversation components retain their core behavior
  when embedded by plugins.
- Workspace navigation and details use the shell's columns, widths and toggles.
  The package owns content; the shell owns layout. Closing a surface releases its
  subscriptions without implicitly stopping admitted tasks.
- Explain a custom renderer's actual framework gap in its source comment. Verify
  existing components before replacing them, and preserve their functionality.
- Keep copy short: clear labels and actions, no filler or repeated outcomes.
  Operation outcomes appear as toasts. Failed saves retain drafts; conflicts and
  uncertain effects remain distinguishable. Destructive prompts name the target
  and consequence once.
- Source, comments, logs, diagnostics and public docs are English; user reports
  are Chinese. UI translations belong in stable i18n keys and locale catalogs.
  Update `internal/desktop/i18n/keys.txt` and the root `locales/` sources, then
  generate the embedded English and Simplified Chinese catalogs. Other Unicode
  belongs only in intentional fixtures or localized resources.

## Runtime and packages

- Sailry owns the agent loop, tools, approvals, persistence and delegation.
  Fantasy supplies per-step model transport. Node callbacks, model translations
  and Desktop entries use the pinned moejs engine.
- Node JavaScript callbacks have a 64 MiB allocation budget, a 60-second deadline,
  cancellation and the engine's recursion bound. Do not describe that bound as a
  physical stack-byte limit.
- Dispatch is the Node's calendar on standard timers. Package schedules and
  automatic backups use that service.
- Package content changes need a newer SemVer version to offer an update.
  Same-version changes, build metadata and older precedence are not updates.
  Preserve installed configuration, enabled choices and removal records.
- Preview and demo stay isolated from models, tools, profiles and user data.
- Mobile uses Client, Link and the Dart FFI adapter. A Desktop test does not prove
  Flutter or device behavior.
- Usage statistics cover Sailry sessions. Provider account status is a separate
  model-plugin feature; do not collect other tools' histories or billing data.

## Data protection

- Keep safeguards at real boundaries: authenticated access, path confinement,
  credential protection, revision conflicts and destructive-action confirmation.
  Do not add redundant approvals or speculative permission systems.
- Preserve user projects, sessions, credentials and databases. Unknown outcomes
  require inspection of the original request; do not repeat effects under a new ID.
- Do not use user profiles or external services as routine test fixtures.
