# Settings

B8 connects the accepted Plugins, Skills, Market and Runtime pages through
`settings.PackageActions` and `live/packages_*.go`. The selected Node owns package
inventory, immutable package reads, revisioned configuration and secret updates,
source discovery, installation and removal. The UI retains configuration and
source drafts after failure. Secret fields send keep, replace or clear actions;
public settings reads contain configured/keepable field names only.
Configuration reads retain their Node/package revision and declared scalar
types. Enum fields keep an unset selection rather than choosing the first
option; model fields read the Node's plugin model catalog. Pending saves lock
fields and an older completion cannot close a newly opened draft.

Catalog selections capture immutable source commits before installation.
Standalone skills support Git, skills.sh, Skillhub and controller-local
directories/archives transferred through Client. A source with multiple skills
uses the existing Kit dialog/list controls for explicit selection; this adds the
discovery step missing from the source-only preview form. Installed skill details
read the exact retained package document. Python index selection uses the Node's
revisioned settings and preserves failed custom URL drafts.
Closing source discovery cancels its captured context and staged upload before
admission. Selecting discovered skills transfers ownership to the durable
installation queue. A later dialog close does not stop an admitted install.

Market cards derive installed identity from the package or skill's recorded
source, including registry namespace and Git repository/path. Catalog versions
and release notes merge with the immutable installed manifest; a catalog slug
does not replace the actual package name. Details show the captured execution
Host. Enter in the existing search field explicitly refreshes a failed query;
live fetches never reuse preview timers or add a controller retry loop.

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
| `connections_live.go` | Additive live connection callbacks, relay option timings, network editor and SSH host-key prompt data |
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
| Dictation | request access, install, cancel download | preview_notice; live: the system's prompt and the controller's speech-model manager |
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

B9 plugin settings pages join the existing groups by declared placement order,
alongside core sections. Entries retain their execution Node and package.
An immutable desktop settings entry mounts through `read_plugin_view`; a
schema-only page uses the existing protected configuration form. Disabled
packages may expose their settings without mounting their ordinary
contributions. The native ConfigurationButton opens only its own captured
package configuration. Revisions, secret keep/replace/clear behavior and failed
draft retention remain with the existing settings owner.

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
state until their owning phases; B4 additionally wires Providers. Live controls
never call preview commits.

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

## B4 provider wiring

`internal/desktop/live/providers.go`, `provider_actions.go` and
`provider_catalog.go` adapt the Node's presets, public providers, catalog status
and compaction settings to the existing Providers page. Neutral provider DTOs
live in `viewmodel`; preview aliases keep their fixture constructors isolated.
API-key choices come from offered Node presets and their captured route keys.
OAuth package workflows remain in B8.

Editor identity and its captured provider base are stable across failed or
uncertain saves. Every save, enable
and removal captures the provider revision. Keys remain blank in the form; an
unchanged key preserves the Node secret, and read-key results become presence
booleans only. Async editor callbacks check their generation so closing and
reopening cannot overwrite a newer draft. Successful saves close the editor;
failure, conflict and uncertain outcomes keep the draft and use separate toasts.

Discovery merges actual endpoint models with manual rows and preserves the
selected default. Source catalog lookups fill missing metadata in batches of
100 IDs, up to 32 pages under the source ten-second metadata deadline. Explicit
discovery capabilities take precedence. Provider saves preserve retained model
capabilities and the selected default effort. Catalog refresh never inserts sample
models. Compaction reads and saves use the Node revision; a failed save keeps its
draft and never invents a new revision.

## B7 Connections wiring

`internal/desktop/live/connections*.go` adapts the shared application `Network`,
its local Client and its Link endpoint to the accepted Connections page.
Execution peers and Mobile controllers retain separate device identities.
Offline peers keep the controller's remembered name, platform and execution
classification. Route and RTT come from the shared Link connection observer;
Mobile controllers are never queried as execution Nodes. The local execution
Node has no revoke action. Existing row menus revoke trust after confirmation,
and host Edit actions open the selected Node's network editor.

The Add dialog retains its pairing and SSH drafts across method changes. PIN
pairing uses `PairPINOnNetwork` with the selected relay network and one retained
claim identity across uncertain attempts. It does not change the controller's
relay settings. SSH installation, host-key consent and installation recovery
belong to the Host adapter; Connections passes its captured draft and selected
network to that owner. Native key-file selection returns a path through the
window queue and cannot overwrite a newer draft.

Visible network pickers read cached presets, refresh the Node-owned catalog and
run finite probes with at most four concurrent measurements. Refresh preserves
the exact selected destination, including a removed or changed preset and
retained multi-relay settings. Custom-address probes use the source's 650 ms
debounce. Picker reads and probes stop when their dialog closes or the page
changes. Timings use the existing select's rows without adding another control.

Sharing prepares the local Node's network with a revision-checked command before
opening the Link Share session. It shows preparation, refreshing live six-digit
codes and an explicit Retry action after failure; operation outcomes appear only
as toasts. Share, pairing and pending reads have a window-owned cancellation
context. Closing the dialog or window cancels the invitation and releases its
code. Share keeps a captured uncertain network request for outcome inspection
when Retry is selected.

Network edits capture the selected Node's settings revision and durable request.
A lost response inspects that request's outcome through the previous address
before adding the selected network's hints. It then verifies the returned
address belongs to the same Node, remembers it and confirms both the saved and
effective selection. An uncertain result locks that request's draft and makes
Retry inspect it without replaying the change. Definite failures and conflicts
keep the editable draft; a refreshed backend revision does not replace it.
Success, failure, conflict and uncertain outcomes use distinct toasts.

The preview constructor remains isolated and shows its existing empty sharing
code. The previously documented sharing-scope and WSL UI deviations remain.
Executable acceptance and cross-platform runtime checks belong to the
coordinator; this wiring's implementation review uses static checks only.

## B10 Terminal wiring

Settings › Terminal is live. `live/settings_terminal.go` keeps the page on the
Node of the host selected in the shell (`App.Host`, the local Node by default)
and holds each Node's draft while another is shown. While the page shows, it
runs `read_terminal_settings` and then `list_shells` when the Node's settings
are unknown, after a save that did not succeed, and when the snapshot reports
a newer `terminal_settings_revision`. A failed read is one
`terminal_settings_failed` toast and is tried again when the page is entered
anew. Controls stay disabled until the settings are read
(`Model.SaveTerminal` is nil) and while a save is pending.

The shell select lists the host's default, the Node's shells once each, and
the saved shell when the Node did not list it. Shells whose program names
collide show their whole command. Environment variables are shown by name.
Add, Edit, Delete and the shell select call `Model.SaveTerminal`, which sends
`save_terminal_settings` with the whole draft at the revision read. Success
takes the Node's answer; failure, conflict and unknown outcomes are the
`terminal_settings_*` toasts and keep the draft. A request with an unknown
outcome is retained and reused by an identical draft. After a failed or
conflicting save the settings are read again: the saved value and revision
follow the Node while a differing draft stays, so the next change saves it at
the new revision.

Paste protection, font size and font family are controller preferences
(`terminal.paste_protection`, `terminal.font_size`, `terminal.font_family`),
saved with the other preferences after the frame that changed them. Terminals
read them through `preferences.Data.TerminalFontSize`, `TerminalFontFamily`
and `TerminalPasteProtection`.

Deviations: an invalid request and a failed shell list use
`terminal_settings_failed`, and a kept draft has no Reload action, because
`terminal_settings_invalid`, `terminal_shells_failed`,
`terminal_settings_reload` and `terminal_settings_discard_reload` are not in
the internal key list. Shells show their program name, not the catalog's
friendly names (`locales/terminal-shells.*.yml`). An unavailable Node during a
save reports failure, not an unknown outcome. The font family is saved as
typed on each change, not trimmed on blur; readers get it trimmed.

## B11 local-model wiring

In live mode the OCR, Knowledge and Decision pages show the selected host's
Node; Dictation's speech models and options are the controller's (below). `live/settings_models.go` reads
`read_local_model_settings` and `read_local_models` when one of the pages
shows, follows `local_models_changed` and `local_model_settings_changed` on
the `node` topic from the first visit on, and projects each kind onto
`LocalModels` (`Kind`, `Models`, `Catalog`, `Model`, `Missing`, `Unavailable`,
`Enabled`). Download percent comes from the Node's installation tasks.
`live/settings_models_library.go` owns the model library dialog.

- **Saves.** The switch, the picker and a row's Select send the whole
  `save_local_model_settings` at the revision read, through
  `Model.ModelActions`. The toasts are `LocalModelMessages` (`knowledge_saved`,
  `runtime_failed`, `runtime_conflict`, `runtime_unknown`). A choice whose save
  failed, conflicted or is in doubt stays the page's draft and the settings are
  read again; an identical retry reuses the retained request.
- **Lock.** `Enabled.Pending` disables the switch, the picker and Select until
  the settings are read, during a save, and where the Node reports the kind
  unavailable; that case adds a `local_models_status` row with the
  `local_models_unavailable` badge.
- **Missing selection.** A saved model that is no longer installed stays the
  picker's last choice (`Missing`) instead of being cleared silently.
- **Knowledge.** There is no switch: Keyword only saves `enabled: false`, a
  model saves `enabled: true`.
- **Library.** `LibraryRequest` (kind, hub, searched query, search serial)
  drives `search_models`; a search is abandoned when the request changes or
  the dialog closes. Install is `resolve_model` → `register_local_model` →
  `install_local_model`, Details is `read_model_versions` → `resolve_model`,
  Cancel is `cancel_local_model` (`knowledge_cancel_requested`), Delete is
  `delete_local_model` after the confirmation (`knowledge_deleted`). An
  install's request is retained for the installation queue's Retry and reused
  after an unknown outcome; a canceled install shows no toast.
- **Preview.** Unchanged: `ModelActions` is nil, pages change in memory and
  downloads are `preview_notice`.

Deviations from the Rust reference:

- The library loads the first hub page only (following empty pages); the
  accepted dialog has no load-more control.
- `refresh_local_model` is not called: the accepted details dialogs have no
  Update control.
- Only the save locks the page; a running download does not.

### Dictation (controller-local)

Dictation's options are desktop preferences, as in the reference: the
`dictation` object of `desktop/preferences.json` holds `microphone` (device
identifier or null), `language` (`auto`, `chinese`, `english`) and `model`
(speech model ID or null for the recommended model). No Node command reads or
saves them, and a Node refuses the speech kind in `save_local_model_settings`
and `register_local_model`. Coverage rows SDICT-003 and SDICT-007 are
therefore preferences (SDICT-007 names `SaveLocalModelSettings`, which the
reference does not use), and SDICT-006 is the controller's speech-model
manager, not `ReadLocalModels`.

- `live/preferences.go` maps `Model.SpeechLanguage` to `dictation.language`.
- `preferences.Data` has `DictationModel()`, `DictationLanguage()` and
  `DictationMicrophone()`, which a dictation reads when it starts.
- `internal/speech/models` is the controller's speech-model manager under the
  desktop data directory (the directory of `preferences.json`): the index
  `models/speech.json`, registered models in `models/speech/<id>` and the
  recommended SenseVoice INT8 model, pinned by size and SHA-256, in
  `models/sensevoice-int8`. Downloads reuse `internal/localmodels/assets`
  (resume, size and SHA-256) over `netproxy.Transport()`, so they follow the
  desktop's proxy settings. A downloaded model counts as installed only after
  the recognizer was constructed over it. A model in use by a dictation is
  neither removed nor downloaded again.
- `live/settings_models_speech.go` owns the page (`dictationFrame`, called by
  `drain`):
  - SDICT-001: `Model.MicrophoneGranted` is `capture.Status()`, read on every
    frame the page shows, so a change made in the system's settings appears
    on return. Request is `ModelActions.Microphone` → `capture.Request`; a
    refusal is the `dictation_permission` toast.
  - SDICT-002: `Model.Microphones` are the input devices listed by
    `capture.Devices()` when the page opens; the picker's choice is stored in
    `dictation.microphone` (null for System default). A stored device that
    is not listed stays chosen as a last entry labelled
    `dictation_device_missing` (`dictation_device_loading` while listing).
  - SDICT-005/006: the rows are the recommended model, whose files are
    verified when the page opens, followed by the registered models. Install
    is `Manager.Start`, the row shows the download's percent, Cancel is
    `Manager.Cancel`; the end of a download is the `knowledge_installed`
    toast or the failure's (`dictation_download`, `dictation_storage`,
    `dictation_model_invalid`, `dictation_unavailable`).
  - SDICT-007: Select stores an installed row in `dictation.model`.
- Only `resolve_model`, `search_models` and `read_model_versions` with the
  speech kind reach a Node in the reference, from its model library. The
  accepted Dictation page has no library, so nothing registers a speech model:
  `Manager.Register` and `Manager.Remove` exist and are tested but have no
  control, and the library dialog still answers a speech result with
  `dictation_unavailable`.

Deviations from the reference:

| Deviation | Reason |
|---|---|
| `dictation.microphone` holds miniaudio's device identifier in hexadecimal, not cpal's | The capture library is miniaudio; a preference written by the Rust desktop shows as the missing-device entry |
| Outside macOS the access row always reads Allowed | Only macOS gates the microphone per application; the reference shows an inert Request on Windows |
| A refused or restricted microphone is the `dictation_permission` toast | The reference's permission card with its system-settings link is not part of the accepted page |
| A registered model is validated by constructing the recognizer, not by reading the graph's SenseVoice metadata | The ONNX binding exposes no model metadata; no control registers a model |
| Speech downloads are not listed among a Node's installation tasks | They are the controller's and the accepted UI has no controller task list |
| The speech, ONNX Runtime and capture libraries load only from the executable's directory | B11 loader rule; without them dictation and the device list report `dictation_unavailable` and the picker offers the system default alone |
