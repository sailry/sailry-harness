# Settings

The settings navigation and its pages, in the groups of the reference implementation
`settings/mod.rs` `Section::GROUPS`: App (General, Appearance,
Conversation, Shortcuts, Dictation), AI (Providers, Connectors, Teams,
Plugins, Skills, Knowledge, Decision), Tools (Terminal, Connections, Hooks,
Runtime, OCR) and System (Permissions, Backup, About). Phase 5a built the
App, Tools and System pages; phase 5b the AI group, the Market, MCP, the
model library and the extension details page.

## Package and files

`internal/desktop/settings`

| File | Holds |
|---|---|
| `settings.go` | `Section`, `Groups` (order and icons of `Section::GROUPS`/`Section::icon`), `State`, `Nav`, `Open`, `Page` routing |
| `model.go` | **The view model**: `Model`, `Stored[T]`, `Messages`, the Node settings' types, `Languages`, `ToastSeconds` |
| `general.go` | General: language, power, sidebar metrics, notifications, toast duration, diagnostics; the `choose`/`pick` select helpers |
| `appearance.go` | Appearance: mode, effects, transparency sliders, theme gallery with update check and import; a card's system right-click menu (Apply, Details, Homepage, Export, Uninstall) and the details dialog (previews light and dark, description, author, version, identifier, license) |
| `conversation.go` | Conversation: command sandbox, session order, message display, text size |
| `shortcuts.go` | `Chord`, `Shortcut`, `Registry`, `Fixed`, `Keys` (overrides, recording, validation) and the Shortcuts page with its recorder |
| `models.go` | Dictation, the OCR local-model page, managed model rows and their details dialog |
| `terminal.go` | Terminal: environment variables, shell, paste protection, font size and family |
| `connections.go` | Connections: hosts and mobile devices, add host (pairing or SSH), share code, revoke |
| `hooks.go` | Hooks: switch, scripts, hook editor, consent dialog |
| `runtime.go` | Runtime: bundled Python and its package index |
| `system.go` | Permissions (the full disk hint is an info `kit.Alert` holding Show app), About (version, build, license) |
| `updates.go` | `Updater`, About's Updates group (updater/panel.rs) with its ready and failure toasts (notices.rs), the install confirmation and the failure's details |
| `backup.go` | Backup: storage, schedule, export/backup/restore, archived sessions, delete all |
| `draft.go` | `draftDialog` (settings/draft.rs) and the dialog footer |
| `providers.go` | Providers: catalog and compaction, families, channels with quota and status, the editor (brand, method, configure, models) and sign-in |
| `connectors.go` | Connectors: toolbar and filter, service rows with status, switch and menu; the add dialog, details dialog and MCP server editor |
| `teams.go` | Teams: concurrency, Teams and Roles tabs, the Default team, team editor (lead model and effort, prompt, members), role editor |
| `plugins.go` | Plugins: inventory controls (Check updates, Download, Install; Update all and Review updates only after a check found them), cards, configuration, install, download from Git with progress, updates review with release notes, uninstall |
| `skills.go` | Skills: cards, install dialog by source, uninstall |
| `market.go` | Market (`MarketPage`): plugins, skills and themes with search, source, filters and install/update states |
| `library.go` | The model library dialog: hub, search, rows with download progress, details, delete |
| `details.go` | The extension details page (`Detail`, `ShowDetails`, `CloseDetails`, `DetailsPanel`) |
| `lists.go` | Dialog pieces: `modalList`/`listRow` (ui/modal_list.rs), `authorization` (ui/authorization.rs), `iconButton`, `confirm` |
| `ai.go` | `enabledOpacity` |

Shared rows live in `internal/desktop/kit/settings.go`: `Page`, `Group`, `Card`,
`Row`, `WideRow` (Row::wide), `Entry`
(settings/entry.rs), `Record`, `Action` and `TextAction` (an entry's small
ghost buttons), plus the switch, select, segmented, text, slider and
button rows. A group without rows (Group::empty) is `kit.Result` with
`Card`, the empty state and load failure in `internal/desktop/kit/result.go`. `kit.KeyText`/`kit.KeyName` write shortcuts for menus,
tooltips and the Shortcuts page. Preview data is `internal/desktop/preview/
settings.go` (`preview.SettingsData`) and `internal/desktop/preview/ai.go`
(`ProvidersData`, `ConnectorsData`, `TeamsData`, `ExtensionsData`).

## Entry point

`shell` keeps `App.Settings` (`settings.New()`), draws `settings.Nav` in
the sidebar and `settings.Page` in the main column, and passes `Look`
(theme, mode, effects, `theme.Surfaces`) for Appearance. `State.Open(sec)`
changes the section and cancels a shortcut being recorded, as
Workspace's park does. Settings opens on General, as Workspace does.
⌘/ (`app.shortcuts`) opens Shortcuts, About in the app menu opens About,
⌘, opens the last section. The rail's Market opens `MarketPage` in the
main column. `State.Details` shows `DetailsPanel` in the right column
over the settings or the market; its Close, the pane's close, opening a
section or navigating to another page clear it (navigation.rs).

## View model

`Model` (model.go) holds every page's fields. Two kinds of setting, as in
the reference:

- **App preferences** (preferences.rs `Data`): plain fields that apply as
  they change: `Language`, `SidebarMetrics`, `Notify`, `Toast`,
  `SessionOrder`, `MessageDisplay`, `TextSize`, `Keys`, `Microphone`,
  `SpeechLanguage`, `SpeechModel`, `PasteProtection`, `FontSize`, `Font`,
  `Channel`, and `theme.Surfaces` on the App. `Updater` is the app updater's state (updater/service.rs State, Failure).
- **Node settings**: `Stored[T]` with `Saved`, `Draft`, `Revision` and
  `Pending` (locks the page's controls): `Power`, `Telemetry`, `Sandbox`,
  `Host` (`TerminalHost`: environment, shell), `Hooks` (`HookSettings`),
  `Python`, `OCR.Enabled`, `Backup` (`BackupTarget`).

AI group fields: `Channels`, `Brands`, `Catalog`, `Compaction`
(Stored), `Family`; `Services`, `ConnTypes`, `ConnAccounts`,
`ConnQuery`, `ConnMethod`; `Teams`, `Roles`, `Concurrency` (Stored),
`TeamTab`; `Packages`, `MarketPlugins`, `SkillList`, `MarketSkills`,
`Themes`, `Library`, `SkillQuery`, the `Market*` filters; `Knowledge`
and `Decision` (`LocalModels`).

Loading states (`preview.Fetch`, played on the frame clock for
`preview.LoadAfter`; nothing is fetched): `MarketLoad` shows six card
skeletons from the market's first frame; `Detail.Load`, started by
`ShowDetails`, shows the details' skeleton under the summary;
`LibraryLoad` shows three placeholder rows before the library's first
page and a `kit.Loader` over the rows on a later search; the connector
editor's Local CLI tab shows a skeleton account row while the host looks
at its accounts.

Lists the pages show: `Devices`, `Relays`, `Permissions`, `Archived`,
`Sessions`, `Shells`, `Indexes`, `Microphones`, `Speech`, `OCR.Models`,
`OCR.Catalog`, `PythonVersion`, `Version`.

Validation is pure and returns the i18n key of the problem or "":
`Keys.Capture` (shortcut_invalid, shortcut_conflict,
shortcut_unavailable), `TerminalHost.SetVariable`
(terminal_variable_invalid, terminal_variable_duplicate),
`hookEditor.Check` (hooks_invalid), `providerEditor.Check`
(provider_name_required, _endpoint_required, _key_required,
_model_required, _model_duplicate, _limits_invalid), `mcpEditor.Check`
(mcp_invalid), `Model.saveTeam` (team_invalid, team_name_taken),
`Model.saveRole` (teams_role_invalid, role_duplicate),
`pluginConfig.Check` (plugins_settings_required, _number,
_secret_required), `skillInstall.Check` (skills_invalid,
skill_source_link_invalid). Pure helpers: `ProviderStatus`,
`StatusOf` (connector status), `serviceItems` (connector menu),
`Classified`, `Update`, `MarketAction`. `BackupTarget.Check` (backup_invalid,
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
| Appearance | check updates, import, Export, Uninstall | preview_notice |
| Appearance | Details, Homepage (card menu) | the details dialog; the system's browser |
| Conversation | sandbox | `Commit`, sandbox_settings_* toast |
| Shortcuts | click a key, then press one; reset; clear; Escape | live: the window uses the new key |
| Dictation | request access, install, cancel download | preview_notice |
| Dictation, OCR | choose model, details | in memory; details dialog |
| Terminal | add/edit (draft dialog), delete, shell | `Commit`, terminal_settings_* toasts on failure |
| Connections | add host (pairing code or SSH), connect/install, share code, revoke, edit network | dialogs open; the network steps are preview_notice; revoke confirms first |
| Hooks | add/edit (dialog), enable (consent dialog), disable, remove | `Commit`, hooks_* toast |
| Runtime | package index, custom URL | `Commit` |
| Permissions | check, settings, request | preview_notice |
| Backup | configure (storage dialog), schedule | `Commit`, backup_* toast |
| Backup | export, back up, restore, archive/unarchive/delete sessions | preview_notice; delete all confirms first |
| About | Check, Cancel, Download, Install (confirms first), Details | `Updater` plays the states on the frame clock: the stable channel finds 0.2.0-preview, the experience one fails to reach the feed; ready and failure toast with Install and Details; Restart is preview_notice |
| Providers | catalog Update, Get API key, open/reopen authorization | preview_notice |
| Providers | compaction threshold | `Commit`, compaction_settings_* toast |
| Providers | add/edit (brand, method, configure), discover models, delete | in memory; problems toast, the editor keeps its draft; delete confirms |
| Providers | sign in | authorization dialog: device code (ChatGPT) or browser with pasted callback (Claude) |
| Connectors | add (choose, method, fields), configure, enable account or server, details | in memory, connector_saved toast; OAuth authorize, probe and reconnect are preview_notice; delete confirms |
| Connectors | Add MCP, configure a manual server | MCP editor; secrets never kept; mcp_configuration_saved |
| Teams | concurrency | `Commit`, teams_settings_* toast |
| Teams | new/edit/duplicate/delete teams and roles | in memory; team_saved, teams_role_saved; deletes confirm |
| Plugins | enable, configure, check updates | in memory / toasts |
| Plugins | install, download, update all, update, review updates' Update, Update Sailry | preview_notice (download shows its progress first) |
| Plugins, Skills | uninstall | confirm, then in memory |
| Skills | install, re-import, update | preview_notice |
| Market | install, update, apply theme | preview_notice |
| Model library | search, details | in memory |
| Model library | install, cancel | preview_notice; delete confirms |

## Backend ownership

The B3 live adapter, controller preferences and remaining phase boundaries are
specified in [B3 live wiring](#b3-live-wiring). The preview remains an explicit,
separate constructor and does not own backend facts. Later feature phases supply
their owning Client actions and projections without changing the accepted UI.

## UI state vs backend data

UI state: `State.Section`, `State.Details`, `Query`, the pages'
searches, filters and tabs, the editors (`provider`, `login`,
`connector`, `mcp`, `teamEdit`, `roleEdit`, `config`, `install`,
`download`, `updates`, `skillAdd`, `library`), `Keys.Active`, dialog flags and
drafts (`env`, `pairing`, `hook`, `consent`, `backups`, `info`),
`OCR.Tab`, `OCR.Query`. Backend data: every `Stored`, the lists above and
the app preferences.

## Tests

- `internal/desktop/settings/settings_test.go`: shortcut capture (invalid,
  conflict with another shortcut and with a fixed key, clear, reset,
  idle), environment variable validation, hook and backup checks,
  `Stored.Commit`; `TestProviderEditor` (steps, back, checks, save,
  statuses), `TestConnectors` (statuses, menus, MCP check), `TestTeams`,
  `TestExtensions` (classification, configuration checks, market actions,
  skill sources, a section closing the details).
- `internal/desktop/shell/settings_test.go`: every section opens with its title in
  en and zh-CN, light and dark (snapshots with `SAILRY_SNAPSHOTS`); ⌘/
  opens Shortcuts, recording holds the window's keys, a conflicting key
  toasts, the new key works and the old one does not, Escape cancels;
  a Node save toasts its outcome; the variable dialog refuses a bad name
  and keeps its draft; a plugin card opens its details in the right
  column and Close shuts it.

## Deviations

- Preview pages show populated data where the reference shows
  "knowledge_host_required"/"hooks_project_required" without a host
  binding (Hooks, Runtime, OCR), so they can be reviewed.
- Connections follows the reference (paired hosts and mobile devices, the
  Add host dialog with pairing and SSH methods), not a separate SSH list.
  The add dialog's sharing scope section (hosts/sharing.rs) and WSL are
  not built. The pairing code is `kit.InputOTP` (six digit cells, from
  Lobe MyGo's field.InputOTP, MIT), whose full code would pair, which the
  preview does not; the share dialog shows empty cells since the preview
  has no code.
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
- Providers: brand tiles use the four bundled provider icons, others the
  name's initial; the API protocol is shown as text (the reference's
  protocol segments and region/product route fields, cloud credentials
  form, partner suggestions and provider-contributed plugins are not
  built). Usage shows the 5h/7d quota bars, not the metrics popover.
  Default effort per model is a check per effort.
- The authorization dialog's countdown is static text, and Open/Reopen
  are preview_notice; the code is shown in cells without a live clock.
- Connectors: the right-click context menu is not built (the row's
  ellipsis menu has the same items); consent review for changed targets
  (connectors/consent.rs) and the plugin-declared MCP configuration form
  are not built.
- Teams: role and team appearance (icon and color picker) is not
  built; roles show the agent mark in their ID's color. The team list's
  right-click menu is folded into the editor's Duplicate and Delete.
- Plugins: the inventory is a resource-card grid; per-plugin action
  retries, Python package preparation and issue lists are not built.
  The details page has Overview, Contents, Permissions and Versions;
  Versions shows the latest notes, not the per-version history dialog.
- Skills, Market: no paging (market "Load more"); the market's plugin
  source and skill catalogs are filtered locally. Theme Apply is
  preview_notice rather than switching the window's theme.
- The model library is a dialog opened by "Manage models" on the
  local-model pages; the Available tab still lists the kind's catalog
  inline. Dictation has no Manage models: its speech models are in the
  page's Installed and Available tabs, as the local-model pages show
  theirs.

## B3 live wiring

`settings.NewLive` constructs controller defaults without any fixture constructor
or simulated loading state. General, Appearance, Conversation and Shortcuts retain
the accepted layout. Other settings and Market pages show the existing unavailable
state until their owning phases; live controls never call preview commits.

The live adapter loads ReadPowerSettings and ReadTelemetrySettings off the UI
thread. It sets Pending while reading/saving, captures each draft/revision and
sends SavePowerSettings/SaveTelemetrySettings. Completion runs through the shell's
UI queue and uses the same power/telemetry outcome messages. Failures, conflicts
and unknown results retain Draft. External power revisions trigger a fresh read;
a failed read is not retried on every frame. Telemetry's installation identity
remains Node-owned and never enters an update. These settings are storage-only
in B3; wake assertions and telemetry sending belong to B13.

`internal/desktop/preferences` owns `<profile>/desktop/preferences.json` and the
complete v1 Rust Data format, including later-feature nullable fields, the sessions
alias for pins and deterministic map serialization. Surface percentages convert
to theme fractions; null sidebar opacity means 45 percent and absent blur uses
the existing default. Go blur is omitted until changed. Unknown/unreadable files
remain untouched and cannot be overwritten. Save failures keep editable values
and report only a toast.

The shared Store synchronizes controller choices across windows and serializes
versioned immutable writes. Language, notification categories, toast duration,
sidebar metrics, appearance/effects/materials, session ordering, message display,
text size, shortcut overrides and welcome state persist. The application callback
applies locale, native menu accelerators/global shortcuts and toast duration.
Shortcut values retain portable secondary/ctrl/alt/shift notation; empty values
mean disabled. The window UI file keeps its existing layout responsibilities.

`TestPrefsRoundTrip` covers the full serde fixture, the Go blur extension, legacy
pins, omitted optional defaults, unreadable/failed saves, startup detection and
stale multiwindow writes. `TestSidebarProjection` and `TestNoticesProjection`
exercise the live presentation boundary. The coordinator runs executable gates.

Expiring Kit success, info and uncertain toasts read the selected duration from
the current window root. Explicit persistent error/conflict timeouts remain
unchanged. The shell installs these preferences before controls render, and live
refreshes them before draining asynchronous outcomes. Message text size uses the
same window scope: user bubbles inherit its DIP size and the reply's caller-owned
Markdown theme scales body typography. The default remains 14 DIPs and preserves
the approved layout. Palette changes still refresh the Markdown theme and MyGo
invalidates retained text geometry when typography changes.
