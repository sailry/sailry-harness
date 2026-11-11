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
fields and an older completion cannot close a newly opened draft. An identical
package command already in flight is not sent again; its second caller is told
it was not sent, so a reopened draft does not stay pending.

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

A plugin installed from a controller-local directory is one the Node cannot
check for updates (`manual` in its report). The desktop remembers that
directory in `plugin_directories` of its preferences, per Node and package,
with the digest and revision the installation gave. Check updates reads the
directory again while the Node still holds that digest, uploads it and offers
a strictly newer version as the update, or reports the Sailry version it
needs; Update installs that upload and remembers the directory for the new
digest. A missing directory leaves the package without an update; another
package found there is a check failure. Any other installation of the package
and its removal drop the record.

Market cards derive installed identity from the package or skill's recorded
source, including registry namespace and Git repository/path. Catalog versions
and release notes merge with the immutable installed manifest; a catalog slug
does not replace the actual package name. Details show the captured execution
Host. The Market projection searches whenever the kind, source or query it last
searched differs from the page's, which is the reference's search on every
change of the field; the page has no search action of its own. Enter in the
existing search field explicitly refreshes a failed query; live fetches never
reuse preview timers or add a controller retry loop.

The settings navigation and its pages, in the groups of the reference implementation
`settings/mod.rs` `Section::GROUPS`: App (General, Appearance,
Conversation, Shortcuts, Dictation), AI (Providers, Connectors, Teams,
Plugins, Skills, Knowledge, Media models, Decision), Tools (Terminal, Connections, Hooks,
Runtime, OCR) and System (Permissions, Backup, Experimental, About). About
is the navigation's last entry; Experimental, the one before it, is this
desktop's own section (see Deviations). Phase 5a built the
App, Tools and System pages; phase 5b the AI group, the Market, MCP, the
model library and the extension details page.

## Package and files

`internal/desktop/settings`

| File | Holds |
|---|---|
| `settings.go` | `Section`, `Groups` (order and icons of `Section::GROUPS`/`Section::icon`), `State`, `Nav`, `Open`, `Page` routing |
| `model.go` | **The view model**: `Model`, `Stored[T]`, `Messages`, the Node settings' types, `Languages`, `ToastSeconds` |
| `general.go` | General: language, power, notifications, toast duration, diagnostics; the `choose`/`pick` select helpers |
| `experimental.go` | Experimental: the sidebar's performance indicator and Demo mode |
| `appearance.go` | Appearance: mode, effects, transparency sliders, theme gallery with update check and import; a card's system right-click menu (Apply, Details, Homepage, Export, Uninstall) and the details dialog (previews light and dark, description, author, version, identifier, license) |
| `conversation.go` | Conversation: command sandbox, session order (Created, Updated, Name: the sidebar's sort menu edits the same choice, docs/sidebar.md), message display, text size |
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
over the settings or the market, and its title (`DetailsTitle`) in the
header band over that column (docs/shell.md). The panel has no title row
and no close button: the header's details toggle, opening a section or
navigating to another page clear it (navigation.rs). The toggle is
disabled while no card is selected.

## View model

`Model` (model.go) holds every page's fields. Two kinds of setting, as in
the reference:

- **App preferences** (preferences.rs `Data`): plain fields that apply as
  they change: `Language`, `SidebarMetrics`, `DemoMode`, `Notify`, `Toast`,
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
`StatusOf` (connector status and reason), `serviceItems` (the connector
menu's available actions),
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
| General | language, switches, toast duration | in memory. Live, a language applies in the frame that saves it (pages, menu and window title), without a toast; the reference applies it at restart and says so |
| General | prevent sleep, telemetry switches | `Stored.Commit`, outcome toast (power_settings_*, telemetry_*) |
| General | reset identity, send diagnostics | preview_notice |
| Experimental | performance indicator, Demo mode | in memory; live: the controller's preferences. Demo mode swaps the window's conversations (docs/shell.md, Demo mode) |
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
| Permissions | check, settings, request, show app | preview_notice; live through `Model.PermissionActions` (B13) |
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

### The Node Settings edits

Every page that configures a Node edits one Node: the host selected in the
shell (`App.Host`, the local Node by default), as the reference binds its
pages to `settings_target.unwrap_or(live.selected)`. `live/settings_target.go`
holds the rule; `Runtime.settingsOwner` is the one lookup.

| Owner | Pages and settings |
| --- | --- |
| The selected host's Node | General › Prevent sleep; Providers, the model catalog and the compaction threshold; Media models; Teams, roles and child concurrency; Connectors and MCP; Conversation › Command sandbox and Code index; Hooks; Terminal shell and environment; Backup; Knowledge, Decision and OCR models; Plugins, Skills and Python |
| This desktop (`preferences.json`) | Language, notifications, toast duration, appearance, session order, message display, shortcuts, dictation, terminal font and paste protection, experimental switches, the update channel |
| The local Node, whichever host is selected | Telemetry consent, identity reset and Send diagnostics (the reference's stated exception) |

The pages share one view model, so only the Runtime of the Node shown writes
it. Selecting another host ends a provider or connector sign-in under way on
the Node left (`Model.CloseNodeDialogs` closes the provider, connector and team
editors), drops the pages' values and drafts, and reads the new Node; the
sandbox, the hooks and the terminal keep each Node's draft, as before. An
answer for a Node the pages left is dropped, and a save sent before the turn
still ends in its toast. A host without a connection has no Node to edit: its
controls wait (`Pending`), its lists are empty, the sandbox switch is disabled
and an action answers the `chat_action_failed` toast. Select model and Manage
teams in a conversation select that conversation's host first, as the
reference's `Event::Settings(node)` and `Event::TeamSettings(node)` do.

Hooks are those of the conversation shown when it runs on the selected host;
a conversation on another host shows `hooks_project_required`, as the
reference passes a worktree only when the settings Node is the selected one.
The host edited is named where the layout already names it: the hooks consent
scope, the plugin details and the backup restore dialog. No page gained a host
label.

## UI state vs backend data

UI state: `State.Section`, `State.Details`, `Query`, the pages'
searches, filters and tabs, the editors (`provider`, `login`,
`connector`, `mcp`, `teamEdit`, `roleEdit`, `config`, `install`,
`download`, `updates`, `skillAdd`, `library`), `Keys.Active`, dialog flags and
drafts (`env`, `pairing`, `hook`, `consent`, `backups`, `info`),
`OCR.Tab`, `OCR.Query`. Backend data: every `Stored`, the lists above and
the app preferences.

An editor's request outlives the dialog that sent it, so its completion acts
only on the editor that asked: the connector and MCP editors and the team and
role editors by a request token, plugin configuration, install, download and
skill install by a generation counted at each opening, Add host by its
generation and draft. A completion for an editor closed or replaced since
changes nothing, and the dialog opened since keeps its draft. The MCP editor
sends one save at a time. A team's deletion closes the editor it was deleted
from only. Rows of an editor's list (MCP arguments, environment and header
slots, a provider's models) are keyed by their index and the editor's count of
removals, so a row that moves into a removed row's place does not take its
undo history; removing a reasoning effort keeps the default effort. Trusting
an SSH host key leaves `ConnectionPending` set, as the installation goes on;
any other way out of the prompt clears it. The network editor stays while its
save is pending, as its Cancel is disabled then.

`Model.Busy` names the operations in flight whose buttons had no pending state
of their own: Providers' catalog Update, the update checks of Plugins, Skills
and Appearance, the Permissions check, and General's telemetry reset and
diagnostics report. The live adapter sets a name with `SetBusy` as it sends the
request and clears it when the request ends, whatever the outcome; the button
is disabled and busy with a spinner meanwhile, and the outcome stays a toast.
`BusyPackage` followed by a package's, skill's or theme's ID marks its install,
update, switch, re-import or removal: its card shows the spinner in the
action's place, its switch and menu removal wait, a Market card's button and
Plugins' Update all are busy. `BusyBackup` followed by an action's name marks
Back up now, the export, the archive and deletion of all sessions and of one
archived session. A list row without an action is not a button and takes no
hover style.

## Tests

- `internal/desktop/settings/settings_test.go`: shortcut capture (invalid,
  conflict with another shortcut and with a fixed key, clear, reset,
  idle), environment variable validation, hook and backup checks,
  `Stored.Commit`; `TestProviderEditor` (steps, back, sign-in, route
  switches and what they keep, the edited channel's fixed route, checks, save,
  statuses, capability boxes) with the Media models rows, `TestConnectors` (statuses, menus, MCP check), `TestTeams`,
  statuses, capability boxes, media rows), `TestConnectors` (statuses, menus per state, the disabled menu button, MCP check), `TestTeams`,
  `TestExtensions` (classification, configuration checks, market actions,
  skill sources, a section closing the details).
  `TestDraftChecks` also renders the Connections rows of a local Host, remote
  Hosts and a Mobile controller: the detail line, the short ID's tooltip and
  Copy ID in each row's native menu.
- `internal/desktop/chrome/chrome_test.go`: the reference's key and cipher
  vectors, a read-only import of a synthetic profile with its skipped rows, the
  schema check, confined profile discovery, the failure states, and no secret
  logged.
- `internal/desktop/live/browser_settings_test.go`: the Browser page's four
  calls against a synthetic profile, a fixed password and a cookie store in
  memory, and the package's real page rendered in zh-CN.
- `internal/desktop/shell/settings_test.go`: every section opens with its title in
  en and zh-CN, light and dark (snapshots with `SAILRY_SNAPSHOTS`); ⌘/
  opens Shortcuts, recording holds the window's keys, a conflicting key
  toasts, the new key works and the old one does not, Escape cancels and so
  does leaving the section by any way;
  a Node save toasts its outcome; the variable dialog refuses a bad name
  and keeps its draft; a plugin card opens its details in the right
  column with the title in the header, no Close button, and the details
  toggle, disabled before, shuts it.

## Deviations

- A remembered plugin directory offers only a strictly newer version. The
  reference also offers changed content whose version is not semantic; the
  packages list here marks an update by its version, and a same-version change
  is not an update anywhere else. The reference's package editor, which reads
  the remembered directory again for Update, has no counterpart: Check updates
  is the one path.
- Experimental is a section the reference does not have: the sidebar's
  performance indicator, moved from General, and Demo mode (user decision
  2026-11-11). It sorts after every page of the System group but About,
  plugin pages included, so About ends the navigation.

- The extension details have no title row and no close button; the title is
  in the header band and the details toggle closes them (user decision
  2026-11-11; docs/shell.md).

- The settings Node is the host selected in the rail; the reference keeps a
  separate `settings_target` that a conversation's Settings entry sets
  without moving the rail's selection. Go has one selection (`App.Host`), so
  those entries select the host.
- Selecting another host drops the drafts of Prevent sleep, compaction, child
  concurrency and media models instead of holding one per Node: each is a
  single control that saves on change.
- Conversation › Execution has a Code index switch the reference does not
  have ([code index](codeindex.md)). After a save that did not take it shows
  the Node's value again; the sandbox and Prevent sleep switches keep the
  position chosen.

- Preview pages show populated data where the reference shows
  "knowledge_host_required"/"hooks_project_required" without a host
  binding (Hooks, Runtime, OCR), so they can be reviewed.
- Connections follows the reference (paired hosts and mobile devices, the
  Add host dialog with pairing and SSH methods), not a separate SSH list.
  The add dialog is 620 wide so that five methods fit as segments. It
  offers the SSH agent, which the reference's dialog does not, except on
  Windows, where WSL takes its place; the sharing scope lists the local
  Node's projects, which the reference leaves empty (docs/host.md). The
  Host update form has no sharing scope. The pairing code is `kit.InputOTP` (six digit cells, from
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
- A downloading library row shows its bar and `Downloading`; the reference
  adds the percent as text.
- Backup's local folder picker, restore from file and the records dialog
  are preview_notice.
- About's license is the reference's (Apache-2.0) as text; the build row
  reads "UI preview".
- Kit's row description is 12 DIP; `kit.Row` keeps the 14 DIP accepted in
  phase 2.
- Providers: marks come from the AI Logo module (the reference bundles its
  own files); a brand the collection lacks shows its name's initial. The
  editor's protocol, product and region are `kit.Segmented` controls (the
  reference's are dropdowns), and a single choice is not shown. Switching
  one keeps an endpoint the user typed (the reference overwrites it). The
  cloud credentials form, partner suggestions and provider-contributed
  plugins are not built. Usage shows the 5h/7d quota bars, not the metrics
  popover.
  Default effort per model is a check per effort.
- The authorization dialog's countdown is static text, and Open/Reopen
  are preview_notice; the code is shown in cells without a live clock.
- Connectors: the right-click context menu is not built (the row's
  ellipsis menu has the same items); consent review for changed targets
  (connectors/consent.rs) and the plugin-declared MCP configuration form
  are not built.
- Connectors: an available server that was never asked to connect reads
  Not connected where the reviewed implementation shows no status, so a row
  always says why it has no tools. The row's ellipsis is disabled when the row
  has no action; statuses and menu rules are in
  [Declared authorization](connectors.md#server-status-and-row-actions).
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
sidebar metrics, Demo mode (`demo_mode`, omitted while off), appearance/effects/materials, session ordering, message display,
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
A provider saved without a route shows its preset's first route.
OAuth package workflows remain in B8.

### Brands, methods and routes

`applyPresets` projects the Node's preset listing. A preset's `brand` (else its
`id`, prefixed by its source for partner and package presets) is its family: a
`viewmodel.Brand`, named by the `brand_display` of its first offered preset,
else by the preset the brand is named after (brand `openai` is preset `openai`,
whichever of its presets the listing has first), else by that first preset.
Each offered preset is one `Method` of its brand, and the preset's routes
(protocol, product, region) are that method's `Routes`, switched inside the
form and never listed as methods. A channel saves the route as
`<source>:<preset>/<index>`. Presets that are not offered stay known, so a
channel saved with one keeps its brand and mark; a channel without a preset
belongs to the family of its wire protocol.

The Add provider dialog has two headed groups. Account sign-in lists every
sign-in method by the account's product (ChatGPT, Claude) and opens its form.
API key lists each brand that has a method other than a sign-in, once; a brand
with one such method opens its form, and a brand with several asks which,
each row naming the method and how it is accessed (API key, Custom endpoint).
Back returns from the form to the methods when the choice offered several,
else to the brands; from the welcome guide the brands' Back returns to the
guide.

The form is the same for a new and a saved channel. Protocol, service and
region are each a segmented control where the method's routes differ in them.
A switch keeps the name, key, typed endpoint and the models the user added or
changed, and replaces what the form filled in: the route's endpoint and the
models the previous route came with. A sign-in method has no key field and
its action is Save and sign in. Editing keeps the channel's route: the three
controls are disabled, as in the reference; name, endpoint (custom routes),
key, OAuth fields and models stay editable.

The family filter lists All, the offered brands and the brands only a saved
channel has, each with its mark in the menu; the closed control shows the
label alone. Channel tiles use the channel's preset mark, else its brand's.

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
draft and never invents a new revision. A `compaction_settings_changed` with a
revision newer than the one shown, as another controller's save is, reads the
threshold again once no save is under way and the slider holds no draft.

`ChannelModel` carries `Vision`, `Tools`, `Reasoning`, `Generates` and
`CustomCapabilities`. The model form's capability boxes edit them, and the
adapter sends them as the model's tags only when `CustomCapabilities` is set;
other models keep the Node's tags. Discovery fills `Generates` from the
catalog's output kinds for display. `Model.Media` (`Stored[[4]string]` of
option keys in `viewmodel.MediaKinds` order), `MediaOptions`,
`MediaUnavailable` and `SaveMedia` back the Media models section
(`settings/media.go`), which `live/settings_media.go` reads and saves. The
section is core and takes the navigation place of the retired media package's
page; the Providers page has no media rows. Controls, outcomes and deviations
are in [media.md](media.md#desktop).

## B7 Connections wiring

`internal/desktop/live/connections*.go` adapts the shared application `Network`,
its local Client and its Link endpoint to the accepted Connections page.
Execution peers and Mobile controllers retain separate device identities.
Offline peers keep the controller's remembered name, platform and execution
classification. Route and RTT come from the shared Link connection observer;
Mobile controllers are never queried as execution Nodes. The local execution
Node has no revoke action. Existing row menus revoke trust after confirmation,
and host Edit actions open the selected Node's network editor.

A device row's second line is one muted line: the system's name with its
version, `Sailry <release>` and the first eight characters of the Node ID, each
part only when known. The system version is the Host's `inspect_host` answer,
so the local Host and bound Hosts show it; Mobile controllers and offline peers
do not. The release is the peer's reported software version. Hovering the short
ID shows the whole ID in a tooltip, and every row's native right-click menu has
Copy ID (`connections_copy_id`), after Edit on Host rows. The route stays in
the status label.

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
code.
Executable acceptance and cross-platform runtime checks belong to the
coordinator; this wiring's implementation review uses static checks only.

## Browser page wiring

The Browser page is the bundled `browser` package's `settings.js`; the desktop
answers its four `sailry/ui` calls in `internal/desktop/live/browser_settings.go`
as the reference host does. They need a settings view of a package that
declares `browser.read` (read, list) or `browser.control` (set, import);
otherwise they fail with "browser capability is unavailable in this view".

| Call | Answer |
|---|---|
| `readBrowserSettings()` | `{supported, enabled, persistent}`: macOS; a profile-backed desktop; `browser_persistent`, on by default |
| `setBrowserPersistent(value)` | Stores the preference; `browser_import_unavailable` off macOS or without a profile |
| `listBrowserProfiles()` | `[{id, name}]` with IDs made for this view and scan; never a path |
| `importBrowserProfile(id)` | `{count, skipped}`; `null` when the user refuses the Keychain prompt |

A view runs one scan or import at a time; another, an unsupported system or a
desktop without a profile answers `browser_import_unavailable`, and an ID the
last scan did not return `browser_chrome_missing`. The Chrome reader
(`internal/desktop/chrome`) answers `browser_chrome_missing`,
`browser_chrome_access_denied`, `browser_chrome_failed` (also a database Chrome
holds locked), `browser_chrome_schema` (a cookie store that is not version 24),
`browser_chrome_key_denied` and `browser_chrome_cipher`; a store that cannot be
read back answers `browser_import_failed`. The page shows each as its toast.

The import opens the profile's cookie database read-only in place, leaves out
partitioned and expired cookies and values that are not `v10`, reads the
`Chrome Safe Storage` password of account `Chrome` through `mygo.Secrets` (an
in-process Keychain query, so the system's prompt names Sailry), decrypts with
Chromium's PBKDF2-HMAC-SHA1 and AES-128-CBC scheme and checks each value's
host digest. Cookies are set in the browser tabs' store through MyGo's
`Session.Cookies`, read back, and counted when the store holds them as given;
the rest count as skipped. The password, the derived key and the decrypted
values are overwritten after use and are never logged or sent to a Node or a
package; the page receives names and counts only.

Deviations from the reference: there is no permission dialog before the
Keychain prompt (see B13 Permissions), so the system's prompt is the consent;
the Keychain's user-cancelled status (-128) is the silent `null`, any other
Keychain failure is `browser_chrome_key_denied`; the value handed to WebKit is
a Go string, which cannot be overwritten.

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
the new revision. A read answered before a save that was accepted since is
older than the settings held and does not replace them; the same holds for
the local model, backup and team settings reads.

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
`Enabled`). Download percent comes from the Node's installation tasks. A
registry event that arrives while a read is under way makes that read
repeat, so an older answer never replaces the event's registry.
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
| The speech, ONNX Runtime and capture libraries load only from the executable's directory, or `Contents/Resources` of the macOS application bundle | B11 loader rule; without them dictation and the device list report `dictation_unavailable` and the picker offers the system default alone |

## B13 Permissions wiring

In live mode `live/permissions.go` owns the Permissions page while it shows.
Each frame it projects `Model.Permissions` and sets `Model.PermissionActions`
(`Check`, `Request`, `Settings`, `ShowApp`); the preview leaves the actions nil
and keeps `preview_notice`.

- **Sources.** Sailry's own grants (Full Disk Access, Accessibility,
  Microphone; subject `sailry`) are read in this process with MyGo
  `permissionflow.Check`, which needs no application loop. Every enabled
  package's `permissions` contributions on the selected host and on the
  controller add grants owned by the package's subject: the computer package
  declares Accessibility and Screen Recording for the helper. A package grant
  is read with `read_plugin_callback` and requested with `call_plugin` on the
  `permissions` surface; its host handlers call `read_computer_permissions` and
  `request_computer_permission`, which run the helper's own `permissions`
  diagnostic on the execution Node (see [plugins](plugins.md)).
- **Rows.** One row per resource. Several owners fold into granted, partially
  authorized (`permission_partial`) or required. A row's `Settings` flag is the
  settings shortcut of a granted resource, or marks an action that opens the
  system's settings (Full Disk Access, a refused microphone).
- **Check** runs when the page opens, when its packages or their connection
  change, after a request, and from the header button. **Request** asks each
  missing owner in turn: `permissionflow.Request` raises the system prompt for
  Sailry, the package's request handler raises the helper's. **Settings** is
  `permissionflow.OpenSettings`; Full Disk Access is checked first, because the
  refused protected open is what lists the application. **Show app** reveals
  the `.app` bundle with `open -R`; a bare executable is the
  `permission_app_unavailable` toast.
- **Outcomes.** A failed check, request or settings page is the
  `permission_failed` toast, once per action however many rows failed and
  including the check that follows a request; the row of a failed check
  reads Unknown. A grant the system does not keep (`errors.ErrUnsupported`)
  reads `permission_unavailable` without a toast. An unavailable or remote
  row draws its action disabled, as the reference does.
- Outside macOS Sailry's rows read `permission_unavailable`; a package grant
  on another device reads through that device and is never requested from here.

Deviations from the reference:

| Deviation | Reason |
|---|---|
| A requestable row that is not granted shows Request alone, without the settings shortcut | The accepted row draws the shortcut and the action label from one `Settings` flag |
| Buttons are not disabled or shown loading while a check or request runs; a second click is ignored | The accepted view has no busy state |
| A restricted microphone reads Denied | `permissionflow` reports refused and restricted alike |
| Full Disk Access is probed by opening the TCC database, then Safari's folder, not by listing the Trash | `permissionflow.Check` owns the probe |
| The Chrome and Keychain workflow cards and the permission dialog opened by other features are absent | Not part of the accepted Settings page |


## B13 system wiring

Ownership: `internal/desktop/updater` owns release discovery, verification,
staging, installation and the receipt; `internal/node/power` owns the one
keep-awake request; `internal/node/telemetry` owns reporting consent, the
anonymous identity's use, the diagnostics report, automatic error reports and
usage counters; `internal/node/logging` owns the process's rotating log and
the one policy for what a record may disclose. `internal/desktop/live`
(`about.go`, `settings_general_telemetry.go`) only maps their state and outcomes.

| Page | Control | Live behavior |
|---|---|---|
| General | Prevent sleep | `save_power_settings`; the Node holds one system keep-awake request while the setting is on and work runs: a turn, a command, an installation, a knowledge collection indexing changed sources, a code index query or a backup (IOKit `PreventUserIdleSystemSleep` on macOS, a `PowerRequestSystemRequired` power request on Windows, unsupported elsewhere). A refused request writes one `power` notice and is not retried |
| General | Error reports, Usage metrics | `save_telemetry_settings`; the Node reapplies consent after each committed change. With Error reports off no failure, interface hang or crash is reported or kept for a later report; with Usage metrics off nothing is counted, feature uses included, and no timer runs. Turning either off drops what was pending |
| General | Reset | `reset_telemetry_identity` with the shown revision; toasts `telemetry_reset`, `telemetry_conflict`, `telemetry_unknown` or `telemetry_failed` |
| General | Send | `send_diagnostics`; one Sentry event with the reviewed fields only (level, logger, message `runtime.diagnostics:requested`, release, environment, fingerprint, the installation, channel, application, OS and architecture tags, and the `diagnostics.jsonl` attachment described below); toasts `telemetry_diagnostics_sent` or `telemetry_diagnostics_failed` |
| About | Version, Build | `link.ProductVersion`; `settings_build_release` when the build carries a revision, otherwise `settings_build_debug` |
| About | Updates | `Model.UpdateActions` call the process's one `updater.Service`; `Runtime.aboutFrame` copies its state into `Updater`, restarts the check after a channel change and shows its notices as toasts in the focused window (ready with Install, failures with Details, an uncertain installation as a warning). A channel never saved is the running build's (`updater.InitialChannel`: a prerelease starts on Experience) and is written once About shows it |

The updater verifies the signed `update-v1` catalog against the release-owned
ed25519 keys, the archive's zipsign signature and digest, the bundle identifier
and the code-signing identity before Ready. Installation stops the Node, swaps
the bundle with a retained original and writes a receipt the next launch
acknowledges.

Deviations from the reference:

| Deviation | Reason |
|---|---|
| Keep-awake is held only while work runs, not whenever the setting is on | An idle Node has nothing to finish, so the machine may sleep; `power.Service.Hold` is the one counter and every source brackets its own work with it. A terminal alone does not count |
| The MyGo updater is not used | It cannot verify the signed `update-v1` envelope or zipsign archives, keep Download, Ready and Install as separate steps, or report the reference's failure keys; the documented fallback ports the reference's verification |
| Installation is macOS only; other systems fail a check with `updates_platform` | The reference's out-of-process Windows install helper and draft recovery are not ported |
| A host's required-version check is not requested by Hosts | `Service.CheckVersion` exists; no caller is wired |
| The Sentry event also carries the SDK's `sdk` and an empty `user` object | sentry-go adds them to every event |
| Rotated files are named `current-<time>.log`, not `current.log.<n>` | lumberjack names its backups; size, count and the active name match |
| Records are written synchronously; there is no bounded queue that drops under load, no macOS unified-log sink and no temporary preview log | `log/slog` with lumberjack has neither; the preview installs no log |
| A report's public fields travel as `contexts.sailry`, not `extra` | sentry-go's event has no free `extra` object |
| A panic is reported at the next launch, not before exit, and there are no debug images | Go has no process panic hook that could report before exit, and MyGo recovers panics only in its IPC, protocol and FFI handlers: a panic in a view or an interface callback ends the process. `debug.SetCrashOutput` keeps the runtime's traceback for the next launch (Interface reports below) |
| A failed operation's package is always reported as `third_party` (`logging.Remote`); only usage counters name a shipped package | Records carry a package's name without the digest that proves it is the shipped one |
| A shipped package updated past the embedded copy counts as `third_party` | Its digest no longer equals the embedded tree's, the one proof of identity the Node has |
| `plan_review` is no longer a counted name | No tool of that name is registered; the counted built-ins are now derived from the registered tools |
| A cancelled command, and any refusal of a read whose caller has gone, is recorded as `cancelled` at info level and never reported | Closing a window abandons its reads; that is not a failure of the Node |
| Every command that changes data is recorded, under its own kind even when a package sends it, with its provider and worktree where the payload names them | The user's November 11, 2026 decision: the reference's partial table left most failed changes undiagnosable. The reference's reads keep their records |
| Only command records are written | The reference's other record sites (runs, model streams, MCP, attachments, sign-in, desktop feedback) are not ported; their fields are in the policy and unused |
| A model request is counted per attempt, without a retry count | The engine reports attempts, not a finished request |

### Logs, reports and counters

Send diagnostics is a controller's own act. **The Error reports and Usage
metrics switches do not gate it**: it is refused only by
`SAILRY_TELEMETRY=off`, `DO_NOT_TRACK=1` or a missing endpoint, as in the
reference. The switches gate everything that would otherwise leave unasked.

`cmd/sailry-desktop` and `cmd/sailry-host` install the log once, before the
Node starts, with `logging.Install`; the preview installs none. The Node
attaches only when the installed log lies in its own profile.

| Part | Behavior |
|---|---|
| Files | `<profile>/logs/<desktop\|host>/current.log`, mode 0600 in a 0700 directory, with `writer.lock` held by the one writing process. 10 MiB a file, ten files, rotated by size only. Earlier daily `YYYY-MM-DD.log` files are removed once the Node owns the profile |
| Records | `<UTC time, microseconds> <DEBUG\|INFO\|WARN\|ERROR> <application> key=value…` in the policy's order. Only the policy's keys are written; strings lose control characters and stop at 256 characters. Records of other libraries stay with the previous `slog` handler |
| Commands | `logging_commands.go` records every command that changes data: `begin`, `admitted` and one outcome (`completed`, `failed` with the fault code such as `revision_conflict`, `cancelled`, or `interrupted` with `outcome_unknown`), each with the kind, Node, caller, request and `duration_us`. The rule follows the registry: a command with durable admission (`protocol.Command.Durable`) is always recorded, and so is any command the file does not name. Commands without durable admission are classified one by one in `transientCommands`; see the classes below. A package's unrecorded commands appear as `plugin_command` with its name, digest and settings revision, its reads at DEBUG. Not found, invalid request and network unavailability are WARN |
| Command classes | Of 365 commands, 189 are durable and 5 transient ones change state (`complete_browser`, `refresh_model_catalog`, `dismiss_installation`, `cancel_local_model`, `request_computer_permission`): full records. 3 registry reads keep the reference's full record and 18 reads its slow record (100 ms, failed or uncertain). Unrecorded: 132 read-only commands (reads, probes, cancelled reads, `send_diagnostics`, `synthesize_speech`, a schedule's reads), 13 transfers that open or cancel a byte stream whose durable finishing command is the recorded change, and 5 interactive commands (`claim_terminal`, `input_terminal`, `resize_terminal`, `set_terminal_appearance` and the desktop's `count_feature`) that would bury the log. `TestCommandLogClassification` fails for a registered command that is neither durable nor classified |
| Redaction | A record names only typed identifiers from the top level of the payload: `session` (`SessionID`), `turn` (`TurnID` in `turn` or `recipient`), `provider` (`ProviderID`) and `worktree` (`WorktreeID`). No other field is read, so payloads, results, prompts, message text, file contents and paths, endpoints, credentials and environment values cannot reach a record |
| Disclosure | `logging.Remote`: `area`, `phase`, `operation`, `code` and `cause_code` as lowercase tokens, `duration_us`, `elapsed_us` and `count`, a SemVer `app_version`, and a package only as `third_party`. Identifiers, names, details and paths never pass |
| Attachment | The last 500 records in the last mebibyte of the active file, each reduced by `Remote`, one JSON object a line. Rotated files are not read |
| Error reports | While Error reports is on, an ERROR record with `phase=failed` becomes one Sentry event (`<area>.<operation>:<code>`, exception `SailryError`, module-relative stack frames) with up to 50 earlier WARN records as breadcrumbs: ten an hour for one fingerprint, 32 in flight, ten seconds each, no redirects |
| Usage metrics | While Usage metrics is on, launches, finished task turns, tool calls, model requests, web searches and desktop feature uses are counted in the tables below. A window of at most 256 groups, and separately at most 256 feature groups, is posted to Telemetry.sh by table 15 minutes after its first count and at shutdown; a model is named only when a built-in preset lists it |
| Interface reports | `cmd/sailry-desktop/diagnostics.go` and `telemetry/interface.go`. Both traces are *reduced*: an optional class word, each goroutine's number and state, and each frame as `function:line`. A reduced trace has no panic message, argument, file path, register or source. **Crash**: `debug.SetCrashOutput` copies a fatal panic's traceback to `<profile>/logs/desktop/interface-crash.raw` (0600) while the process prints it and exits as before; the next launch reduces it to `interface-crash.txt`, empties the raw copy and, while Error reports is on, sends one event `interface.crash:<class>`. **Hang**: while frames are drawn and for 30 s after the last one, a watchdog asks the interface thread for a heartbeat every second with `mygo.RunOnMain`, which draws nothing; a heartbeat unanswered for 5 s while a window is visible and the system is awake writes every goroutine's reduced trace to `interface-hang.txt` and, while Error reports is on, sends one event `interface.hang:unresponsive`, once for that hang. Two seconds between the watchdog's own ticks (sleep, a debugger) restart the measurement; `Power.OnSuspend` and hidden or minimized windows pause it; native dialogs and menus run nested loops that still answer. An idle interface has no heartbeat and no timer. Both files stay local and are written whatever the switch says; the events carry the exception's frames, the reduced trace as `goroutines.txt`, and the usual tags, under the ten-an-hour limit |

### Usage tables

Every row carries `event_id`, `window_start`, `window_end`, `count`,
`installation_id`, `channel`, `application`, `os`, `arch` and `release`, and
only the dimensions listed here. `TestCountersAndInterfaceReports` sends one of
each observation and fails for any other key, for a feature outside the list
and for a planted secret, path or name.

| Table | Dimensions and sums |
|---|---|
| `app_launches` | none |
| `turns` | `result` (`completed`, `failed`, `cancelled`), `mode` (`code`, `plan`, `flagship`), `subagent`, `error_code` |
| `tool_calls` | `tool` (`source`, `name`), `result` (`success`, `error`, `denied`, `cancelled`), `error_code`; `elapsed_us_total`, `elapsed_us_max` |
| `model_requests` | `api`, `authentication`, `model` (a built-in preset's identifier or `custom`), `result`, `http_status_class` (`1xx`…`5xx`, `unknown`), `error_code`, `retries`; `input`, `output`, `cache_read`, `cache_write` |
| `search_requests` | `driver`, `result`, `error_code`; `elapsed_us_total` |
| `feature_uses` | `feature` |

`tool.source` and `tool.name` (`engine.CountedTool`, from the tools the turn
registered; a name the turn did not register is not counted):

| Source | Name | Rule |
|---|---|---|
| `builtin` | the registered name | A tool without a package binding is registered by the Node itself: the engine's `set_session_title`†, `ask_user`, `list_sessions`, `read_session`, `search_session`, `read_tool_output`, `spawn_agent`, `guide_agent`, `wait_agent`, `load_skill`, `read_skill_resource`, `tool_search`, and the Node's `manage_skills`, `manage_mcp`, `manage_plugins`, `list_agents`, `add_agent`, `edit_agent`, `delete_agent`, `apply_agents`, `list_teams`, `add_team`, `edit_team`, `delete_team`, `apply_team`. The set is derived, not listed: a new built-in is counted under its name. † `telemetry.UncountedBuiltins` leaves a tool out with its reason (`set_session_title`: asked for once in every session, so its count repeats the session count). `TestCountedTools` and `TestTelemetryOffSendsNothing` fail for a registered tool that is neither |
| `official` | `<package>.<tool>` | The tool's owning package is one the application ships: its reference equals an embedded package's by name **and** digest (`operations.Official`, the test `CaptureCatalog` applies), and the tool is one that package's embedded manifest declares. Both parts are the embedded manifest's own spelling |
| `mcp` | none | A connected server's tool, whoever declares the server |
| `third_party` | none | Any other package's tool, including a package installed under a shipped package's name |

`feature` is one identifier of `protocol.FeatureUse`'s closed list. The
desktop sends it with `count_feature`, a command that carries the identifier
alone; the Node counts it only from the controller on its own device and
refuses anything outside the list. A window that shows a remote Host still
counts on the local Node only. One count is one use in the window; no row has
an order, a time within the window, or a second dimension.

| Identifier | Counted when |
|---|---|
| `page.conversation`, `page.activity`, `page.files`, `page.git`, `page.market`, `page.host`, `page.settings`, `page.notifications`, `page.project` | The page shown changes to it. A window's first frame counts nothing |
| `page.plugin_page.official:<package>`, `page.plugin_page.third_party` | A package's page is shown. `<package>` is sent only for a package whose Node reports its origin as bundled, and accepted only when the local Node ships a package of that name |
| `settings.section.general`, `.appearance`, `.conversation`, `.shortcuts`, `.dictation`, `.providers`, `.connectors`, `.teams`, `.plugins`, `.skills`, `.knowledge`, `.decision`, `.terminal`, `.connections`, `.hooks`, `.runtime`, `.ocr`, `.permissions`, `.backup`, `.about`, `.plugin_page` | The settings section shown changes to it; `.plugin_page` is any package's settings page |
| `panel.tab.browser`, `.terminal`, `.files`, `.git`, `.history`, `.documents`, `.artifact`, `.subagent` | A tab of that kind opens in the right panel (`panel.State.Show` of a tab not yet open) |
| `panes.split` | A pane is split off beside another |
| `composer.submit`; `composer.submit.has_attachment`, `.has_reference`, `.dictated`, `.queued`, `.guided`, `.edited`; `composer.mode.code`, `.plan`, `.flagship`; `composer.permission.ask`, `.project`, `.full` | A message is submitted: once, and once for each flag that applies, its mode and its permission level. The counts are marginal; nothing ties one flag to another. `.dictated` means dictation was started in that draft |
| `composer.model_picker`, `search.palette` | The model controls or the search palette open |
| `read_aloud.os_voice`, `read_aloud.model_voice` | A reading starts, by the voice that reads it |
| `dictation.start`, `session.export` | Dictation starts; an export is requested |
| `theme.change.builtin`, `theme.change.package` | The theme changes to the built-in one or to a package's, never named |
| `settings.save.provider`, `.team`, `.role`, `.connector`, `.mcp`, `.plugin`, `.hooks`, `.backup`, `.terminal`, `.media`, `.power`, `.telemetry`, `.network`, `.sandbox`, `.search`, `.compaction`, `.python`, `.local_models`, `.code_index`, `.knowledge`, `.credential`, `.ssh`, `.database`, `.defaults` | A command of that settings family is sent (`live/feature_usage.go` `savedFeatures`, from the command's kind alone, never its values) |

An interface event's class is `unresponsive` for a hang and, for a crash, one
of `panic.nil_dereference`, `panic.index_out_of_range`,
`panic.slice_out_of_range`, `panic.divide_by_zero`, `panic.type_assertion`,
`panic.nil_map`, `panic.closed_channel`, `panic.runtime_error`, `panic.other`,
`fatal.concurrent_map`, `fatal.deadlock`, `fatal.out_of_memory`,
`fatal.stack_overflow`, `fatal.other`, `signal` or `unknown`.

Limits of the watchdog: it follows frames, so an interface callback that
blocks more than 30 s after the last frame, with no frame in between, is not
seen; and the raw traceback of a crash, which may hold a panic's message,
stays in the profile's log directory until the next launch reduces it.

## B13 Backup wiring

Settings › Backup is live. `live/settings_backup.go` keeps the page on the
selected host's Node (`App.Host`); `Model.BackupActions` is nil in the preview,
which keeps `Commit` and `preview_notice`. Turning to another host drops the
page's dialogs and drafts. Each Node keeps its own settings and running action,
so an action finishes and toasts after the page has moved on.

| Control | Command | Notes |
|---|---|---|
| Page | `read_backup_settings`, the controller's snapshot | Read when the page shows and after every action that did not return settings. Controls are disabled (`Backup.Pending`) until the settings are known and while an action runs. Archived sessions and the session count come from the snapshot |
| Storage dialog › Save | `save_backup_settings` | Sent at the revision read. Typed secrets replace, blank ones keep, Clear credentials removes; WebDAV clears the session token. The schedule is left as saved. A clean draft closes without a request. The draft stays in the dialog until the Node has saved it |
| Choose folder | the in-app host file picker (docs/files.md), titled `backup_local_directory` | Browses the shown host through its controller, opening at the field's path |
| Schedule switch, time, timezone | `save_backup_settings` | The switch saves at once, time and timezone 350 ms after they rest, with every secret kept. An enabled schedule without a timezone waits. A draft the Node refused is not sent again until it changes |
| Export | save panel, `download_backup`, download stream | Written beside the chosen path and published when complete |
| Back up now | `export_backup` | |
| Import › Restore | file picker, `upload_backup`, upload stream, `restore_backup_file` | A refused restore releases the staged upload with `cancel_file_transfer` |
| Restore from storage | `list_backups`, `restore_backup` | Each record offers its local and online copies |
| Unarchive, Delete | `set_session_archived`, `remove_session` | Delete confirms first, naming the session |
| Archive all, Delete all | `archive_sessions`, `remove_sessions` | Named at the revisions in the snapshot; delegated sessions follow their parents |

The restore form names the host, shows `backup_restore_consequence` once and
asks for the passphrase (blank uses the saved one) and the new profile path.
The Node refuses a path that exists with content or lies inside the running
profile, so a restore never writes over a live profile.

Outcomes are toasts only: `backup_saved`, `backup_exported`,
`backup_restored`, `backup_archived`, `backup_unarchived`, `backup_deleted`;
`backup_conflict`, `backup_invalid`, `backup_denied`, `backup_unknown`,
`backup_failed`; for sessions `backup_sessions_busy`, `session_changed`,
`backup_sessions_unknown`, `backup_sessions_failed`. A request whose outcome is
unknown is retained and sent again by the same action.

Backend ownership: `internal/node/backup` (archive, encryption, storage,
restore staging), `internal/node/backup_service*.go` (commands),
`internal/node/store/backup_settings*.go` (settings, schedule, restored
profile), `internal/node/files/backup_transfers.go` (streams).

Deviations from the reference:

| Deviation | Reason |
|---|---|
| Saving a local folder inside or around the running profile is refused (`PermissionDenied`) | The reference refuses it only when an export runs; the save would store a folder that can never be used |
| The staged copy of each archived file is not flushed to disk; the finished archive is | The reference flushes every staged file; the copies are read back into the archive and removed, so only the archive needs to survive a crash |
| The records, restore and session removal dialogs exist only in live mode | The reference has all three; the preview's Backup actions are the notice |
| No toast actions (Retry, Reload) | The accepted toasts carry no actions; repeating the action resends a retained request, and the page reads the settings again on entry |
| The page does not subscribe to `backup_settings_changed` | The snapshot has no backup revision; a stale revision surfaces as `backup_conflict` and a fresh read |
| A file restore with an unknown outcome is staged again when repeated | The staged stream belongs to the first request |
| A running action is not cancelled when the page is left | Its toast still reports the outcome |
| An archived session that never ran shows no date | A session carries no archive time |
