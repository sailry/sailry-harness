# Plugin packages and execution

The execution Node owns installed packages, settings, private storage, callbacks,
connectors, MCP and installation outcomes. Controllers use the shared protocol;
the live desktop adapters project those resources without importing preview data.
`internal/desktop/plugins` maps the preserved v1 desktop element and Host SDK
contracts to MyGo and Kit. `internal/desktop/live/plugin_*.go` binds those
contracts to the captured execution Node.

`internal/node/plugins` validates Agent Plugins manifests and the Sailry v1
extension. Packages are captured before publication and stored by BLAKE3 digest
under `plugins/packages`. Resource readers verify the complete captured content.
Installation stages files before the existing Node writer atomically publishes
inventory and its durable result. A failed update preserves the previous
inventory, configuration and enable choice.

Bundled defaults are initialized once. Startup refreshes an existing bundled
package only for strictly newer SemVer precedence and respects removal records.
Same versions, build metadata changes and older versions do not replace an
installed package. A retired bundled package (`plugins.RetiredBundled`, now
`media`) is removed at startup when its installation is of bundled origin: the
removal record and captured versions stay, and its pages, tools and skills no
longer appear ([media.md](media.md#retired-media-package)). Application version 0.1.6 is the first Go release; its ported
sources and pinned packages match the reviewed 0.1.5 Desktop/Host; the protocol remains v1.

Package versions and settings revisions are collected when the Node starts,
before it admits a command, and when it stops, after its last work has ended
(`store.CollectPlugins`, then `plugins.Manager.Collect`). A version stays
while an installation, a queued or running dispatch job, an admitted turn, a
saved or captured provider binding or a bound assistant names its digest; a
settings revision stays while one of those names it. Everything else is
deleted: first the `plugin_packages` and `plugin_settings` rows in one
transaction, then the digest directories under `plugins/packages`. Only
canonical digest directories are touched, an interrupted run leaves an orphan
for the next one, and a failure is logged without blocking startup or
shutdown. A retired or removed package therefore keeps exactly the versions
that turns captured.

Repository discovery freezes a Git commit before inspecting or installing files.
One acquisition fetches a single revision at depth 1 into a temporary bare
repository that is removed afterwards. A captured commit is fetched by its
identifier and a moving reference is never resolved again; a server that does
not serve commits by identifier has its branches and tags fetched instead and
the commit looked up there. Every transfer is limited to 512 MiB of pack data
(`invalid_request`, "Git transfer exceeds its size limit") and 120 seconds
(`unavailable`, "Git transfer timed out"). The newest inspected package is kept
in memory only until it is installed or another repository is inspected; skill
catalog pages are kept for the 32 most recently used queries.
Catalogs, signed registry skills and authenticated one-use uploads share package
validation. Local sources stay inside their captured worktree and outside the
profile. Symlinks, traversal, conflicting archive paths and oversized captures
are rejected. Installed packages are independent of their source checkout.

Settings preserve public values and explicit keep/replace/clear secret updates.
`read_plugin_secret` returns one saved secret slot to a controller and is refused
to plugin callers. A secret saved under a binding that has since changed stays
hidden. Where Rust answers `invalid_request` "setting binding changed" for a
changed binding with nothing saved, the Go Node answers with no value.
Immutable settings revisions capture declared destinations. Credentials are absent
from public snapshots and settings history. HTTP and MCP inject them only into
declared origins, servers and slots, without forwarding across redirects.

Private KV writes check key revisions. Indexed values update KV and the SQLite
fallback's FTS5 corpus in the same transaction. Conversation values follow
canonical branch history, fork and rewind. Transactions admitting a turn anchor
preceding state writes to that turn.

Storage writes feed knowledge. A `node` collection declared with
`index.semantic: true` becomes an automatic knowledge collection once it
holds an indexed value with a text `source`, and `search_plugin_values` in
`semantic` or `hybrid` mode ranks those values by meaning as well as by
keyword. A package may also create a knowledge collection over a prefix of
its own values. Every storage write, removal and plugin transaction requests
an indexing pass after it commits and never waits for it
([knowledge.md](knowledge.md#plugin-storage)). Of the bundled packages only
`memory` declares a semantic collection (`memory/entry/`).

A tool declared with `storage.get`, `storage.list`, `storage.set` or
`storage.delete` is offered to the model with the source's schema and bound by
`internal/node/plugin_tools_storage.go`. Each call is dispatched under the
declaring package of the frozen turn, so the store applies that package's
namespace, declared `storage.read`/`storage.write` action, key and namespace
quotas and revision check; another package cannot read the value. Writes are
authorized by the storage scope (approval source `storage`) and refused in Plan
mode. Lists accept a positive safe integer limit and return at most 100 keys.
The model receives the entry or page itself. These tools are found through tool
search unless declared `initial` ([Tool catalog](#tool-catalog)). A
`progress.update` tool with the progress
presentation is offered on the first request, validates the snapshot against
the 64 KiB text budget and returns it as `progress`. Handler tools keep their
script binding.

The store records each tool call with the presentation, grouping and display
its package declares. Installed packages are resolved from the version the turn
captured, by operation or handler alias and by MCP alias for server tools, so
an installed `progress` tool is recorded and projected as progress.

## Tool catalog

`internal/node/agent/engine/tools.go` decides once per turn which captured tools
the first model request carries; the rest stay discoverable through
`tool_search` and their names are sent in the turn context under
`Deferred tools`, without the ones the conversation already loaded.

- A package tool starts visible when every operation it declares (its own, its
  handler's and its flow's) is one of the 21 everyday operations: `files.read`,
  `files.read_lines`, `files.list`, `files.find`, `files.search`, `files.write`,
  `files.edit`, `files.apply`, `code.search`, `code.outline`, `code.graph`,
  `git.status`, `git.diff`, `process.run`, `process.read`, `process.stop`,
  `web.fetch`, `web.search`, `progress.update`, `roles.list` and
  `artifacts.revisions`.
- A tool declared `initial` starts visible to the main agent only. The tools of
  the package that owns the selected conversation mode start visible.
- Of the Node's own tools `read_tool_output`, `load_skill`,
  `read_skill_resource`, `set_session_title` and `ask_user` start visible;
  `list_sessions`, `read_session`, `search_session`, the management, role, team
  and media tools are discoverable. The delegation tools start visible in
  Flagship mode or when the message selects a role.
- A plugin assistant turn and a child whose role lists its tools defer nothing
  and have no `tool_search`.
- `set_session_title` and `ask_user` belong to the conversation the user reads.
  A child never has them: a role that lists either fails the child with
  `role references an unavailable tool`, and an MCP server's input request in a
  child is refused with a tool error instead of waiting for an answer.

Records, approvals, session rules, hooks (`SAILRY_HOOK_TOOL`) and telemetry
identify a package tool by its alias `plugin_<16 hex>_<name>` and an MCP tool by
`mcp_<label>_<24 hex>`. Only the model boundary
(`internal/node/agent/engine/names.go`) renames them: a package tool is offered
under its declared name and an MCP tool as `mcp__{server}__{tool}`, in the
declarations, the tool calls and results of the history, the `Deferred tools`
list and `tool_search` queries and results; a model's call is mapped back before
it is stored. A name is used only when it is unique among the turn's tools and
the names earlier calls of the conversation used, is not itself shaped like an
alias and fits the provider rule (at most 64 characters of `[A-Za-z0-9_-]`).
Otherwise a package tool keeps its alias; equal or overlong MCP names take an
eight-character hash of the alias, cut to 64 characters. Compaction requests
carry no tools and keep the stored names.

A call that changes something asks for approval before it runs, except where
the write cannot leave its owner:

- `artifacts.write` writes the conversation's own artifact. It is approved with
  source `storage` at every permission level and in Plan mode; a child has no
  such tool.
- A tool, handler or flow whose declared operations are only `storage.set`,
  `storage.delete` and `storage.transaction` writes its package's own values and
  is approved with source `storage`. Plan mode refuses it.
- A plain script handler (no operation, no flow) records no approval: every
  command its script dispatches is checked against the package's grants on
  admission. Plan mode refuses one that is not read-only.
- Every other handler or flow that is not read-only asks the user under Ask and
  Project permission.
- A read-only tool asks for the one call that also writes the project: a
  browser capture with `save=true` (`engine.Tool.Confirm`).

A flow step runs a read-only operation, the package's own storage
(`storage.*`), `settings.read` or `http.request`. A step has no approval of its
own, so a workspace change (`files.write`, `process.run` and the other writing
operations) runs only as a handler's bound `operation`, under that tool's
approval. A flow that declares one is refused before the user is asked, with
`plugin flows cannot run <operation>: ...` (`pluginFlowRuns`).

Deviations from Rust: a declared name that breaks the provider rule keeps its
alias instead of being sent as declared; an MCP tool declared `initial` starts
visible to the main agent, where Rust always defers MCP tools outside a selected
mode; a child's MCP input request is refused, where Rust records a question the
parent page never shows; Rust runs every declared operation as a flow step under
the flow's one approval, including commands and file writes, which no bundled
package uses.

The task supervisor bounds Node work. The installation queue permits eight
pending preparations and serializes each resource through durable publication.
Caller-scoped topics expose progress, retain failures or uncertainty until
dismissal and remove successes. Disconnection does not cancel admitted work.

Callbacks use pinned `github.com/MoeForge/moejs v0.1.0` and unchanged SDK resources. Each invocation
captures package/settings/resource authority. SDK commands return through ordinary
Node admission, action checks, writer transactions and receipts. Read-only
callbacks cannot admit durable effects. Tools retain the turn's exact packages,
selected declarations, approvals, mode and role.

A package scope confines every command to its own worktree and session, with
two rules of their own. `start_session` creates a session rather than
addressing one: it is admitted with no turn and no session in scope, the
association must be filed under the package's own name, an assistant or
resource configuration is refused, and the draft's worktree is compared only
when the scope itself names one. A Node-wide callback, such as a scheduled
task's, therefore starts its session in whichever worktree the task stored. A
package scope without a turn or session may then address a session it started,
recognised by that association, when the session is not a delegated child and
lies inside the scope's worktree if one is set. `store/plugin_authority.go`
owns both rules; `TestScheduledTaskStart` fires the bundled scheduled-tasks
callbacks through them.

The same file carries the remaining scope rules of the source's plugin check:

- `set_session_team`, the command behind the SDK's `setSessionExecution`, is a
  `control_conversation` action.
- Roles are listed and written only from a turn or the package's own settings
  surface; `set_session_roles` only from a turn.
- `read_project_catalog` and `stop_dispatch_turn` need a Node-wide scope.
- A notification names only a session the caller may address, and a saved
  dispatch handler's callback scope stays inside the scope that saves it.
- A turn reads Git in any worktree of its captured worktree's project. Worktree
  management requires a captured worktree with a project, names that project
  and that source, and removes any worktree of the project.
- Without a turn, a child conversation's scope does not write files, Git or
  worktrees.

Not compared with the source: role and local-model approval checks, the session
an artifact read resolves to, and the per-call enabled check for media and
local-model commands outside controller settings.

The callback engine has a 64 MiB allocation budget. Calls have a 60-second
deadline and honor cancellation, including compilation and result conversion.
The engine's own fixed call-depth bound handles recursion; it is not a physical
stack-byte bound. Allocation accounting is not a process RSS ceiling. Infinite
recursion returns `InvalidRequest` with
`plugin JavaScript execution failed: stack overflow`; the host remains alive.
Callback and translation paths share the engine and error mapping. Retained
model translation owns its module state, registers Go-held live values and
releases call scratch between operations. Cancellation joins any in-flight
interrupt before the owner can be reused or released.

MCP keys capture package digest, settings revision, server, launch and
authorization identity. Changed captures retire old owners. Retained leases
check current authorization, credentials and destination after acquiring their
call locks and before each RPC. Connection, tool, descriptor and message sizes are bounded. The
owner retains subprocess/Python resources until cleanup settles. Lost transport
or unconfirmed cleanup returns `OutcomeUnknown`, without replay. Cleanup is
confirmed by the transport's own close; the error a stream ends with when it is
closed from this side is not a failed cleanup. Form and URL
elicitation use canonical questions and pause only active elicitation time.

### MCP servers outside a turn

The pool owns three things a turn does not drive:

- Idle servers. A shared connection without a lease for 30 minutes is closed by
  the pool's one-minute timer, which ends a stdio server's process group. Node
  shutdown closes every connection, leased or not.
- Runtime. The pool keeps, per server of the installed package version, the
  runtime its last attempt reached, and wakes the Node when it changes. The Node
  records `plugin_changed` for that package. A changed version, a disabled or
  removed package forgets the runtime.
- Warm-up. `sailry-desktop` and `sailry-host` start the Node with
  `mcp.Options.Warm`. The Node then connects, lists and releases every server
  that is allowed, `available` and shared (`reuse`), at start and after a
  package or account change. It remembers per package the version, the startable
  servers and the accounts it last tried, so a server is tried once for each of
  them and a failed one waits for a change. A Node started without the option
  (every test that does not ask for it) contacts no server ahead of use.

Opening is bounded to 20 seconds, a tool listing to 10, a call to 120 seconds of
active time, and a stdio server's exit to 10 seconds of close, terminate and
kill.

Package behavior stays in the pinned plugin repository's tooling, which can use
the production moejs test host. Core tests cover host boundaries and admission;
tooling results do not substitute interactive account or platform acceptance.

## Package loops

A package that keeps a session working declares `host.turn.after_turn` and
continues with the `continue` transaction operation. The Node states each
ended turn's facts to the callback, bounds automatic turns with one count
that the package's declared `limit` owns for its chain, names the reason of a
refused continuation, hands a continuation the session's waiting background
results, and shows the continuation under the package's marker
(`continuation_marker`, or one of `continuation_markers` selected by
`marker`) instead of its prompt. A plain tool handler may declare
`approval: true` to ask for the ordinary tool approval. The contracts are in
[conversation](conversation.md#turn-settlement-and-queue) and
`sdk/plugins.md`; the bundled goals package is described in [goal](goal.md).

## Desktop wiring

Each mount captures its Node, package digest, settings revision, surface,
worktree, session, locale and presentation generation. Node-owned
`read_plugin_view` returns the immutable entry and declared resources.
Navigation, settings, project renderers and contribution slots follow manifest
metadata. Files and Git stay native core surfaces and are excluded from every
desktop plugin slot. No package-specific renderer or workflow runs in core.

A slot's entry is drawn from what its package publishes (`plugins.Registry`,
`PluginUiState`); the core owns the trigger and the surface and the package
its content:

| Slot | Drawn by | Where |
|---|---|---|
| `statistics` (`metric`) | `live/plugin_metrics.go` into `conversation.UsageBar` | The composer's context bar: the declared icon and the published value; the hover card lists the label, the value and the published detail rows, a detail with a progress as a meter, or the package's own view where the entry declares a handler. A narrow bar folds every statistics figure into one button and panel |
| `footer` (`metric`) | The same | Before the statistics figures, while the package publishes it visible and enabled: a provider's account status (`provider-status`, `workbuddy`) |
| `composer` (`indicator`) | `live/plugin_controls.go` (`pluginIndicators`) | The context ring beside the send button |
| `composer`, `context` (other kinds) | `live/plugin_controls.go` | The composer's control rows |
| `header` (`button`, `popover`, `toggle`, `menu`) | `live/plugin_views_header.go` | The conversation header's right end. The window opens a `popover` itself; its panel is the package's handler |
| `model` | `live/plugin_controls.go` | Beside the composer's model button |
| `commands` | `plugins.Registry.Commands` | The composer's command list |
| `status` | `live/plugin_status.go` | The activity strip's badges, from the Node's status feed |
| `provider`, `project`, `project_menu` | Not mounted | The registry and the Node accept their surfaces; the desktop mounts neither (`docs/settings.md` records the provider's) |

Figures need a session: a draft shows none. Demo mode mounts the statistics
package alone over a demo conversation, with a composer state built from the
conversation's own usage (`live/plugin_demo.go`; see `docs/shell.md`).

The persistent moejs owner retains the View, callbacks, modules and Promise
continuations across frames. Render and layout never execute arbitrary queued
jobs. Host completions and timers return to that owner before the next render;
closed generations reject callbacks and late results. Source loading, event
tasks and rendering use the source's respective watchdogs. Desktop entries are
trusted UI code and do not inherit the Node callback allocation ceiling.
A `fetchWeb`, `searchCode` or `outlineCode` still running when its view closes
sends `cancel_web_fetch` or `cancel_code_index_query` for that request under
the view's scope, so the Node stops the work.

Client supplies shared change cursors, observations and detail reads. Activity
uses the captured Node's bounded catalog/previews and Client lane aggregation,
with controller-owned labels and terminal read state. Hiding the page cancels
its previews. Status is produced by Node and observed through Client, including
the source's scheduling, failure cooldown and retained disconnected snapshot.
Intent resolution can reveal a hidden status popover without starting a model.

A package's declared `display.icon` and `tint` reach every surface that shows
the package. Live projection carries them on `viewmodel.Package` (`Icon` is the
declared name or the package's own SVG markup) and on connector types and
services. Market cards, Settings > Plugins cards, the details header, the
updates dialog and connector service tiles draw it with one function
(`iconTile` in `internal/desktop/settings/plugins.go`) on a rounded tile,
radius 11 at 36 DIPs:

- A glyph (`phosphor:<name>`, `reicon:<group>/<name>` or the package's own SVG)
  is white, 5/9 of the tile, on the gradient of the declared tint, or of the
  tint the name hashes to when none is declared.
- A brand mark (`ailogo:<slug>`) is drawn on a neutral tile of the theme's
  hover fill and border: in the brand's own colors where AI Logo has a color
  variant, otherwise the one-color mark in the foreground color.
- No declaration, or a name nothing draws, is the puzzle piece on the gradient
  tile (the plug for a connector service). It is not an error.

The tile is still: nothing in it follows the pointer. The fill glyph and the
brand mark are for the tile only: the package's own `display.icon` and a
connector's. Text-colored slots (rail feature buttons, pages, panels, settings
sidebar entries, tools and tool rows, menus, commands, status badges and script
`Icon` components) draw the icon declared for that slot through `icons.Plugin`,
and it should be an outline icon, `reicon:<group>/<name>`, as the
application's own chrome icons are. A `phosphor:` name there still draws,
solid, and a bare `ailogo:<slug>` draws its mono variant. `icons.ByName` falls
back to `icons.Plugin` for tool rows.

`icons.Plugin` resolves four kinds of name: `phosphor:<name>` from the embedded
Phosphor fill archive (`internal/desktop/icons/phosphor`, the complete set of
`@phosphor-icons/core` 2.1.1, read on first lookup and cached), `ailogo:<slug>`
from the AI Logo module, `reicon:<group>/<name>` from the Reicon glyphs kept in
`internal/desktop/icons/svg`, and the application's own icon file names. The v1
validation in `internal/protocol/navigation.go` accepts a `phosphor:` name
listed in `phosphor_icon_names.txt` and a `reicon:` name listed in
`icon_names.txt` for navigation, panel and contribution icons; `ailogo:` names
are for package, tool and connector `display` icons and view components, which
that validation does not cover. Bundled packages declare a Phosphor icon of
their own, or their brand's mark where AI Logo has it, and keep the source's
Reicon outline icons everywhere else; `TestElementMapping/plugin icon catalog`
holds every bundled manifest to that.

Native bridges expose scoped terminal, conversation, artifact, configuration and
file owners. They preserve the captured workspace and action requirements.
Services belonging to later phases return `Unavailable` until their owners
exist. Native credentials are opaque handles; scripts never receive their
contents. Disconnection stops script execution and service callbacks while
retaining the last native presentation. Final close releases its native owners.

## Plugin assistants

A session's assistant binding names one package version and one declared
conversation. Creation requires the current available version and the resource
its declaration names; a plugin-resource assistant shares one private scratch
workspace under the profile across its conversations. The binding and resource
are immutable afterward. Connection-bound database and SSH assistants wait for
their resource owners in B9b.

Each turn freezes the owner and only the packages whose tools the declaration
imports. The model receives the declared native, owner and imported package
tools plus tool-output reads, all on the first request, and the declaration's
context. Imported packages contribute no other tool, skill or MCP server.

An agent reference in an assistant conversation is checked by Node before the
turn is admitted, for submit, queue, guide, replace, continue, start coding,
task start and queued edits. The assistant must select the native `spawn_agent`
tool or a package tool bound to `agents.delegate`; otherwise the command
returns `PermissionDenied` with `plugin assistant does not allow delegation`
and no turn is created. The desktop keeps the draft and shows its existing
failure toast.
A plugin or skill reference in an assistant conversation
must name the assistant's own package. The full submit-time reference rules are
listed in docs/conversation.md.

The location SDK's session base is retained with its draft. The first send or
attachment creates a managed worktree on the chosen branch from the chosen
commit, moves the draft into it and then creates the session. An uncertain
creation keeps the base, so a retry reuses the captured request.

## Connections

Saved SSH and external database profiles belong to the execution Node. Secrets
never enter snapshots, events, results or diagnostics. A saved database password
is bound to its exact connection and current SSH destination; changing either
requires a new password.

`internal/databases` executes one statement per call on PostgreSQL, MySQL or a
local SQLite file with bounded rows and bytes. Require TLS verifies the database
hostname and system roots, also through an SSH tunnel. SQLite is a file on the
execution Node, never inside its profile; the same pinned engine's low-level
API opens it without following links and classifies each statement before it
runs, because the driver's `database/sql` surface exposes neither.

`internal/ssh` adds SFTP paging, file changes, bounded command output and
transfers to the existing verified connection. Staged uploads use the Node's
single-use transfer streams and are published once for their original caller,
profile revision and path. Remote effects that may exist after cancellation,
timeout or a lost connection report an uncertain outcome and are never replayed.
A file change the server refuses with an SFTP status keeps its definite fault;
a create, move, copy, remove or upload whose reply never arrives reports
`outcome_unknown`, because the server may have applied it.

A worktree transfer refuses Node storage, except a registered execution
workspace inside the profile (a session, connection or assistant workspace or
a managed worktree), which is the worktree of its own conversation. The
workspace handle still confines the path and refuses links.

Database and SSH commands share the Host admission path and run outside the
storage writer. Plugin scopes reach them only through their declared database
and SSH actions and the connection's sharing. A connection-bound conversation
uses that connection's private scratch workspace. The agent's connection tools
see only reachable connections; SQL execution, remote commands, transfers and
saved-profile changes require approval, and an unknown SSH host key is confirmed
in that approval before it is trusted.

WSL discovery and WSL2 Host setup run only on Windows; other systems return the
documented unavailable result. WSL setup downloads the release installer, which
exists once the B14 release feed is published.

## Canonical cards

Tool details retain canonical card receipts and ordering. The desktop reads the
captured declaration and entry rather than substituting current inventory.
Node still requires a current available package with the captured digest before
authorizing reads or actions. Settings references can advance within that
digest. Cards expose only `sailry/card`, with declared native actions and QR
rendering; props publication does not rewrite canonical history.

## UI state vs backend data

The UI owns focus, drafts, expansion, native element state and mount visibility.
Node owns package code, configuration, credentials, storage, canonical card
receipts and operation outcomes. Client owns observations, ordered projections
and activity aggregation. Each asynchronous completion retains its original
scope and generation; it cannot select a newly focused draft or another Node.

## Native components

Package text follows the window's resolved catalog, as the source's
`rust_i18n::locale()`: the frame reports `i18n.Match(view.Locale())` and every
mount, manifest label and `messages(locale)` call reads that locale. Packages
ship their own translations; core adds none.

A `sailry/ui` component is drawn by the control the application's own pages use
for that concept, at the same sizes and tokens:

| Component | Control |
|---|---|
| `SettingsPage` | `kit.Page`, the one centered page column |
| `SelectField` | `kit.SelectField`, filling its slot |
| `MenuButton` | `kit.MenuButton` with `kit.SecondaryLook` and a caret |
| `Menu` with an icon and no caret | `kit.MenuButton` with `kit.IconLook` (28, or 32 for `medium`): the label is the tooltip and accessible name |
| `SettingsRow` | `kit.LeadRow` (`kit.WideRow` for `wide`): label, description and a leading icon |
| sortable `SettingsGroup` | `kit.SortableList` with `Divided` rows inside `kit.Card` |
| `CardRow`, `CardSummary` | a frosted card at Kit's row metrics (`RowPad`, `RowMinHeight`, `RowFont`, `TitleGap`) |
| `SegmentedTabs` | `kit.Segmented` (`fill` is `Block`) |
| `EmptyState`, empty `CardList`, empty `DataTable` | `kit.Result` (`Card` for the dashed list card) |
| loading `CardList` | a card of `kit.Skeleton` lines |
| `ModelPopup` trigger | `kit.SecondaryLook`, full width, title and caret |
| `Button` | `kit.Secondary`; `primary`, `danger` and `ghost` variants are `kit.Primary`, `kit.Danger`, `kit.Ghost`; sizes 20, 24, 32, 36 |
| searchable header filter | `kit.SelectTrigger` over a query popover |

`CardColumn` and `CardSummary` keep the source widths (128, 192, 48; title from
96). Their rows follow Kit's row metrics
([components](components.md#row-metrics)), so a package's settings rows and
card rows have the rhythm of the application's own settings rows.

A sortable `SettingsGroup` draws each child behind `kit.SortableList`'s drag
handle and reports a drop as the group's control event
`{id, value: {from, to, order}}` ([SDK](../sdk/plugins.md)). The renderer keeps
no order: the package persists it and returns its children reordered. The
bundled `web-search` Settings page orders its services this way; no bundled
package or core settings list reorders with up and down buttons.

A row's `icon` is resolved by `icons.Plugin`. A name it does not know, such as
an `ailogo:<slug>` brand mark before that collection is present, draws no icon
and raises no error; when another row of the group has an icon the slot stays
empty so the text remains aligned.

## Deviations

- The source draws `CardSummary` subtitles at 12 px and `CardRow` without a
  minimum height. Both follow Kit's row metrics here (14 px, 56 minimum), so
  package rows match the application's settings rows.
- The source `Menu` draws its label beside its icon. An icon's menu is Kit's
  icon button here, as every row menu of the application; `show_label` keeps
  the label.
- `SettingsRow` `description`, `icon`, `wide` and `SettingsGroup` `sortable`,
  `sort_label`, `sort_disabled` are additions to the v1 element contract: the
  source has no reorderable list component.

- MyGo does not resolve a child's percentage size along a scrolling axis. The
  renderer resolves it against the scroll container's previous-frame bounds and
  builds those children once the viewport is known.
- `h_auto`, `w_auto`, `flex_initial` and an automatic `flex_basis` set nothing:
  MyGo's `Auto` height and basis stretch an element to its limit. `truncate` on
  a container reaches its text children, which MyGo's `SingleLine` does not.
- `SelectField` is not searchable and `ModelPopup` opens a menu; the source
  searches the select and opens the model picker popover. The select stays
  Kit's as it is. The composer's model picker is a page of the composer's own
  state (its families, team member and effort), not a component a package's
  control can be given, so `ModelPopup` keeps Kit's menu.
- A page is told it was entered (`Host.Enter`) before it is mounted, so the
  entry is part of the first change it reads and it loads its data once. Told
  after the mount, the entry was a second change and every entry command of
  the page went out twice.
- `readBrowserSettings` reports an unsupported browser: the host keeps no
  browser profile, so the Browser page shows the package's unavailable notice.
  searches the select and opens the model picker popover.
- The browser package's settings calls (`readBrowserSettings`,
  `setBrowserPersistent`, `listBrowserProfiles`, `importBrowserProfile`) answer
  as the source host does, without its permission dialog: the system's
  Keychain prompt is the consent ([settings](settings.md#browser-page-wiring)).

- Package icons are Phosphor fill glyphs on tinted tiles, or AI Logo brand
  marks on neutral tiles; the source shows a Reicon glyph on a tinted tile.
  `phosphor:<name>` is an additional icon namespace of the v1 icon contract.
  Navigation, tool and other text-colored icons are the source's Reicon
  outlines, unchanged. Reicon glyphs that no bundled package, SDK example or
  application icon names are not embedded; such a name still validates and
  draws the placeholder.
- Collection keeps two things the source deletes. A removed package keeps its
  latest settings revision, because reinstalling it restores that
  configuration here (the source starts a reinstalled package from no
  configuration). A version named by a saved provider's binding stays, so a
  provider saved before a package update still starts a turn.
- MCP outside a turn is simpler than the source in four places. Warm-up is
  chosen by what changed for the package (version, startable servers,
  accounts) instead of the source's separate set of authorized targets, and a
  server that is not `available` is not tried. A server that ends its own
  session reports `failed` at once; the source notices at the next package
  change. A package change retires the connections of its other versions
  without the source's ten-second validation of the ones it keeps: a lease
  already validates its authority before each call. A failure before the pool
  is reached (launch preparation) records no runtime; the source records it
  as failed.
- The source GPUI element contract is adapted to MyGo and Kit; imported runtime
  resources retain their notices and source revision.
- Cards with an embedded authorization browser use an isolated native MyGo page
  window on macOS. The source uses a resource pane. Other systems retain the
  explicit unavailable embedded-browser result.
- Native Files and Git remain the accepted core pages. Their package desktop
  entries and Git Status contribution are deliberately not mounted.
- The `project` and `project_menu` slots are drawn where the source draws
  them: `project` controls follow the project overview's own actions as its
  secondary buttons, and `project_menu` buttons and menus follow New terminal
  and Records in the project's system menu as items and submenus of choices.
  Both belong to one registry per project, scoped to its main worktree (the
  overview's selected one). The application already draws six bundled
  controls itself, so those declarations validate and are not mounted:
  `commands` `terminal-new`, `terminal-cli`, `terminal-menu-new` and
  `terminal-menu-cli` (the overview's and the menu's New terminal and AI CLI)
  and `worktrees` `manage` (the menu's Worktrees); the Git package's `manage`
  goes with its unmounted entry. A default installation therefore mounts
  nothing for a project. The system menu is built when it is asked for, so
  while an installed package declares a menu item of its own, every listed
  project keeps its registry mounted; the source mounts on the click and
  waits up to ten seconds before showing the menu.
- Extension validation at install follows the source for the declarations
  that cross-reference each other: every tool, handler and flow operation
  needs its action declared; retired language-server operations and actions
  are refused; a flow has one to eight distinct operations and none that
  writes files, delegates, reports progress or reads settings; a handler tool
  has a description and an object parameter schema of at most 16 KiB; an
  `initial` tool stays inside package storage; host handlers are 1 to 64
  exportable names, host resources are scripts, and `turn`, `commands` and UI
  message commands name declared handlers and routes; model sign-ins,
  resources, adapters and provider presets are distinct, reference each other
  and carry their actions; `artifact_view` needs both artifact actions;
  `enabled_service` is `code_index` on a host package with `code_index.read`;
  `conversation_options` parses as an option schema; a permission
  contribution names a read-only check, a declared request handler and the
  package's own actions. Three rules differ on
  purpose: a handler may be bound to `agents.guide`
  ([delegation](delegation.md)), which the source refuses; a `display` icon
  is not checked, because the tile accepts `ailogo:` names the source's icon
  list lacks; and a server tool's name may be 512 bytes, as in the source,
  where this validator allowed 128. Not ported to install time: the field
  rules inside one model sign-in, model resource, model adapter, provider
  preset, connector, conversation mode, artifact view and tool `display`.
- Bundled packages are read from the embedded tree by
  `agent/operations` (its own manifest tool decoding, digest and skill
  reader) and installed packages by `plugins`; the source has one path. The
  two skill readers accept the same documents: an empty `compatibility`, a
  locale tag that is not ASCII subtags joined by hyphens and a control
  character in a localized name are refused by both, and a display name is
  trimmed and ignored above 256 bytes. The two readers are not yet one
  function.
- A request on the `permissions` surface is confined as in the source: it
  carries no worktree, session or turn, reads only a contribution's `check`,
  calls only its `request`, and inside those callbacks runs only commands
  whose action a contribution lists.
- The `provider` slot validates and is not drawn. Settings shows a channel's
  5h and 7d quota from the Node's own `provider_quotas`
  ([settings](settings.md)); the bundled `provider-status` `provider_usage`
  metric repeats it. The bundled `workbuddy` `provider_credits` metric has no
  native counterpart and is not shown.
- The window opens a header `popover` on the press. The source also opens one
  whose declaration sets `hover` while the pointer rests on it; no official
  package declares it.
- A footer figure shows while its package publishes it visible and enabled.
  The source also requires the package's model context to name a provider;
  the official packages publish theirs for their own provider only.
- An SFTP change whose reply is lost reports `outcome_unknown`; the source
  reports every non-status SFTP error as `unavailable`.
- SSH worktree transfers accept registered execution workspaces inside the
  profile; the source refuses every path inside the profile, which leaves a
  connection session unable to transfer its own files.

## Agent management tools

`manage_plugins`, `manage_mcp` and `manage_skills` are Node-owned agent tools
(`internal/node/agent_tools*.go`). They are bound for unrestricted turns and for
assistant turns, are deferred until `tool_search` loads them, and dispatch the
same commands the Settings pages use (`list_plugins`, `read_plugin`,
`search_plugin_catalog`, `read_catalog_plugin_info`, `inspect_plugin_source`,
`check_plugin_update`, `check_plugin_updates`, `install_bundled_plugin`,
`install_plugin_source`, `install_mcp`, `install_skill`,
`install_registry_skill`, `install_skill_upload`, the discovery commands and
`remove_plugin`) as the turn's caller. They add no installation, enablement or removal logic. In Code
mode every call, including a listing, takes one non-rememberable approval;
`full` permission approves it automatically. Checks that need no effect, such
as the standalone-package and revision checks, run before the approval. In Plan
mode listings run without approval and a mutation returns the manager's own
`permission_denied` fault. Children without an explicit role tool list do not
receive these tools.

Package tools declaring `models.install`, `models.remove` or a `roles.*`
operation are bound by `internal/node/plugin_tools_scoped.go` with the package
scope, the workspace approval binding and the existing `install_local_model`,
`delete_local_model`, `list_roles`, `put_role`, `remove_role` and
`set_session_roles` commands. A script handler such as the `files` package's
`install_ocr_model` calls that native binding, so installation enters the
local model queue and removal passes `AuthorizeLocalModelRemoval`. The package
needs the matching action (`models.install`, `roles.read`, `roles.write`).
`TestAgentManagementTools` runs these tools in real turns.

## B13 computer helper

The computer package's tools are its MCP server's `desktop_*` tools, approved
like any MCP tool. Core supplies only the helper:

- **Locator.** An stdio server whose command is `${SAILRY_COMPUTER_USE}`
  launches the execution Node's packaged helper (`internal/node/computer`):
  `Sailry Computer Use.app/Contents/MacOS/agent-desktop` under the bundle's
  `Contents/Resources/helpers` on macOS, `helpers/agent-desktop[.exe]` beside the
  executable elsewhere, or under `SAILRY_HELPERS_DIR`. `PATH` is never
  searched. A missing or unsupported helper is an `Unavailable` fault, with no
  fallback. Linux launches inherit the graphical session variables.
- **Identity.** On macOS the helper starts through a trampoline of the Node
  executable (`computer.Trampoline` in `cmd/sailry-desktop` and
  `cmd/sailry-host`) that replaces itself with the helper under a disclaimed
  responsibility, so the helper application owns its Accessibility and Screen
  Recording grants. The trampoline triggers only when the executable's first
  argument is exactly `--sailry-computer-helper` and a program path follows;
  no environment variable, later argument, URL or document open selects it,
  and the Node adds the argument only to its own helper launches. It runs
  before any flag parsing, opens nothing and reads no profile. The helper
  receives the remaining arguments, the environment, the standard streams, the
  signal mask and the ignored signals exactly as a direct launch would,
  because the spawn attributes carry only the replace-image flag and the
  disclaimer. A failed disclaimer falls back to a plain `exec`; a failed
  `exec` exits with status 127. The trampoline grants nothing: a disclaimed
  program answers for its own permissions and never inherits Sailry's, so
  starting it by hand gives no more than running that program directly.
- **Permissions.** `read_computer_permissions` and `request_computer_permission`
  need the calling package's captured helper launch on macOS; they run
  `agent-desktop permissions [--request]` with a 15-second deadline and a
  64 KiB output bound. Other systems report no requirement without starting the
  helper, and a request there is `Unavailable`. A request from another device
  is `PermissionDenied`.
- **Overlay.** A helper launch of an enabled package whose `cursor_overlay`
  setting is not off joins one `agent-desktop` session per helper home
  (`AGENT_DESKTOP_SESSION`), configured with the cursor overlay. The session
  ends when the package is removed, disabled or turns the setting off, and at
  Node shutdown. A failed overlay step leaves the launch without an overlay.
- **Screenshots.** The helper returns a capture at the size of the display.
  `boundToolImages` (`internal/node/tool_images.go`) gives every image part of
  an MCP result the limits of the other tool images, 1568 pixels on a side
  and 3 MiB (`files.PrepareCapture`, at most 20 MiB in), and replaces one it
  cannot reduce by the note "image omitted: <reason>". The reference passes
  MCP images on unreduced, at sizes model providers refuse.
  `TestToolImageBounds` covers it.
- The retired `computer.read` and `computer.control` operations are not
  registered: the reference keeps them only as unavailable metadata.
- A declared handler operation `media.inspect`, `media.image` or `media.video`
  binds no tool: image inspection and image and video generation are built-in
  tools.

