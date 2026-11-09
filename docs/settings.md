# Settings

The settings navigation and its pages, in the groups of sailry-public
`settings/mod.rs` `Section::GROUPS`: App (General, Appearance,
Conversation, Shortcuts, Dictation), AI (Providers, Connectors, Teams,
Plugins, Skills, Knowledge, Decision), Tools (Terminal, Connections, Hooks,
Runtime, OCR) and System (Permissions, Backup, About). Phase 5a builds the
App, Tools and System pages; the AI group keeps phase 1's Providers and
Plugins pages and shows an empty group for the rest until phase 5b.

## Package and files

`internal/settings`

| File | Holds |
|---|---|
| `settings.go` | `Section`, `Groups` (order and icons of `Section::GROUPS`/`Section::icon`), `State`, `Nav`, `Open`, `Page` routing |
| `model.go` | **The view model**: `Model`, `Stored[T]`, `Messages`, the Node settings' types, `Languages`, `ToastSeconds` |
| `general.go` | General: language, power, sidebar metrics, notifications, toast duration, diagnostics; the `choose`/`pick` select helpers |
| `appearance.go` | Appearance: mode, effects, transparency sliders, theme gallery with update check and import, details |
| `conversation.go` | Conversation: command sandbox, session order, message display, text size |
| `shortcuts.go` | `Chord`, `Shortcut`, `Registry`, `Fixed`, `Keys` (overrides, recording, validation) and the Shortcuts page with its recorder |
| `models.go` | Dictation, the OCR local-model page, managed model rows and their details dialog |
| `terminal.go` | Terminal: environment variables, shell, paste protection, font size and family |
| `connections.go` | Connections: hosts and mobile devices, add host (pairing or SSH), share code, revoke |
| `hooks.go` | Hooks: switch, scripts, hook editor, consent dialog |
| `runtime.go` | Runtime: bundled Python and its package index |
| `system.go` | Permissions, About (version, build, license, updates) |
| `backup.go` | Backup: storage, schedule, export/backup/restore, archived sessions, delete all |
| `draft.go` | `draftDialog` (settings/draft.rs) and the dialog footer |
| `ai.go` | Providers and Plugins (phase 1) |

Shared rows live in `internal/kit/settings.go`: `Page`, `Group`, `Card`,
`Row`, `WideRow` (Row::wide), `EmptyGroup` (Group::empty), `Entry`
(settings/entry.rs), `Record`, `Action` and `TextAction` (an entry's small
ghost buttons), plus the switch, select, segmented, text, slider and
button rows. `kit.KeyText`/`kit.KeyName` write shortcuts for menus,
tooltips and the Shortcuts page. Preview data is `internal/preview/
settings.go` (`preview.SettingsData`).

## Entry point

`shell` keeps `App.Settings` (`settings.New()`), draws `settings.Nav` in
the sidebar and `settings.Page` in the main column, and passes `Look`
(theme, mode, effects, `theme.Surfaces`) for Appearance. `State.Open(sec)`
changes the section and cancels a shortcut being recorded, as
Workspace's park does. Settings opens on General, as Workspace does.
⌘/ (`app.shortcuts`) opens Shortcuts, About in the app menu opens About,
⌘, opens the last section.

## View model

`Model` (model.go) holds every page's fields. Two kinds of setting, as in
the reference:

- **App preferences** (preferences.rs `Data`): plain fields that apply as
  they change: `Language`, `SidebarMetrics`, `Notify`, `Toast`,
  `SessionOrder`, `MessageDisplay`, `TextSize`, `Keys`, `Microphone`,
  `SpeechLanguage`, `SpeechModel`, `PasteProtection`, `FontSize`, `Font`,
  `Channel`, and `theme.Surfaces` on the App.
- **Node settings**: `Stored[T]` with `Saved`, `Draft`, `Revision` and
  `Pending` (locks the page's controls): `Power`, `Telemetry`, `Sandbox`,
  `Host` (`TerminalHost`: environment, shell), `Hooks` (`HookSettings`),
  `Python`, `OCR.Enabled`, `Backup` (`BackupTarget`).

Lists the pages show: `Devices`, `Relays`, `Permissions`, `Archived`,
`Sessions`, `Shells`, `Indexes`, `Microphones`, `Speech`, `OCR.Models`,
`OCR.Catalog`, `PythonVersion`, `Version`.

Validation is pure and returns the i18n key of the problem or "":
`Keys.Capture` (shortcut_invalid, shortcut_conflict,
shortcut_unavailable), `TerminalHost.SetVariable`
(terminal_variable_invalid, terminal_variable_duplicate),
`hookEditor.Check` (hooks_invalid), `BackupTarget.Check` (backup_invalid,
backup_passphrase_required, backup_passphrase_invalid,
backup_passphrase_mismatch).

Secrets are never loaded into the model. SSH passwords, keys and
passphrases, backup credentials and the backup passphrase are what the
user types to replace them; saved ones show as the `backup_keep_secret`
placeholder in an empty masked input.

## Actions

| Page | Action | Preview |
|---|---|---|
| General | language, switches, toast duration | in memory; language toasts settings_language_restart |
| General | prevent sleep, telemetry switches | `Stored.Commit`, outcome toast (power_settings_*, telemetry_*) |
| General | reset identity, send diagnostics | preview_notice |
| Appearance | mode, effects, theme, transparency | live on the window |
| Appearance | check updates, import | preview_notice |
| Conversation | sandbox | `Commit`, sandbox_settings_* toast |
| Shortcuts | click a key, then press one; reset; clear; Escape | live: the window uses the new key |
| Dictation | request access, manage library, install, cancel download | preview_notice |
| Dictation, OCR | choose model, details | in memory; details dialog |
| Terminal | add/edit (draft dialog), delete, shell | `Commit`, terminal_settings_* toasts on failure |
| Connections | add host (pairing code or SSH), connect/install, share code, revoke, edit network | dialogs open; the network steps are preview_notice; revoke confirms first |
| Hooks | add/edit (dialog), enable (consent dialog), disable, remove | `Commit`, hooks_* toast |
| Runtime | package index, custom URL | `Commit` |
| Permissions | check, settings, request | preview_notice |
| Backup | configure (storage dialog), schedule | `Commit`, backup_* toast |
| Backup | export, back up, restore, archive/unarchive/delete sessions | preview_notice; delete all confirms first |
| About | check updates | preview_notice |

## Wiring a backend

- Load each `Stored` from its Node read (power, telemetry, sandbox,
  terminal, hooks, python index, local models, backup) and set `Revision`.
- Replace `Stored.Commit` with the revisioned request: set `Pending`, send
  `Draft` with `Revision`, and return the outcome. `report` maps it to the
  section's `Messages`: `kit.Failed` keeps the draft, `kit.Conflict` keeps
  it until reloaded, `kit.Uncertain` for a request whose result is unknown.
- Persist the app preferences (preferences.rs) and apply `SessionOrder`
  (sidebar), `MessageDisplay` and `TextSize` (conversation), `Font`,
  `FontSize`, `PasteProtection` (terminal), `Notify`, `Toast` (toasts).
- Rebuild the native menu when `Keys` changes so its accelerators follow,
  and register the system shortcuts (`System`) with the OS.
- Replace the preview_notice actions with the real requests; the
  dialogs already collect what they need.

## UI state vs backend data

UI state: `State.Section`, `Query`, `Keys.Active`, dialog flags and
drafts (`env`, `pairing`, `hook`, `consent`, `backups`, `info`),
`OCR.Tab`, `OCR.Query`. Backend data: every `Stored`, the lists above and
the app preferences.

## Tests

- `internal/settings/settings_test.go`: shortcut capture (invalid,
  conflict with another shortcut and with a fixed key, clear, reset,
  idle), environment variable validation, hook and backup checks,
  `Stored.Commit`.
- `internal/shell/settings_test.go`: every section opens with its title in
  en and zh-CN, light and dark (snapshots with `SAILRY_SNAPSHOTS`); ⌘/
  opens Shortcuts, recording holds the window's keys, a conflicting key
  toasts, the new key works and the old one does not, Escape cancels;
  a Node save toasts its outcome; the variable dialog refuses a bad name
  and keeps its draft.

## Deviations

- Preview pages show populated data where the reference shows
  "knowledge_host_required"/"hooks_project_required" without a host
  binding (Hooks, Runtime, OCR), so they can be reviewed.
- Connections follows the reference (paired hosts and mobile devices, the
  Add host dialog with pairing and SSH methods), not a separate SSH list.
  The add dialog's sharing scope section (hosts/sharing.rs) and WSL are
  not built. The pairing code is a plain text input, not Kit's OTP cells;
  the share dialog shows empty cells since the preview has no code.
- Shortcut buttons show the key as text, not Kit `Kbd` chips. A key needs
  ⌘, ⌃ or ⌥ (or is a function key); the reference accepts what GPUI's
  keystroke parser does.
- The hook editor saves on Close after validation; the reference saves as
  fields change (debounced) and Close ends the edit.
- The model details dialog shows the source and status only; size and
  license come with the model library (phase 5b).
- Backup's local folder picker, restore from file and the records dialog
  are preview_notice.
- About's license is the reference's (Apache-2.0) as text; the build row
  reads "UI preview".
- Kit's row description is 12 DIP; `kit.Row` keeps the 14 DIP accepted in
  phase 2.
