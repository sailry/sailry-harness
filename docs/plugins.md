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
installed package. Application version 0.1.6 is the first Go release; its ported
sources and pinned packages match the reviewed 0.1.5 Desktop/Host; the protocol remains v1.

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

A tool declared with `storage.get`, `storage.list`, `storage.set` or
`storage.delete` is offered to the model with the source's schema and bound by
`internal/node/plugin_tools_storage.go`. Each call is dispatched under the
declaring package of the frozen turn, so the store applies that package's
namespace, declared `storage.read`/`storage.write` action, key and namespace
quotas and revision check; another package cannot read the value. Writes are
authorized by the storage scope (approval source `storage`) and refused in Plan
mode. Lists accept a positive safe integer limit and return at most 100 keys.
The model receives the entry or page itself. These tools are found through tool
search unless declared `initial`. A `progress.update` tool with the progress
presentation is offered on the first request, validates the snapshot against
the 64 KiB text budget and returns it as `progress`. Handler tools keep their
script binding.

The store records each tool call with the presentation, grouping and display
its package declares. Installed packages are resolved from the version the turn
captured, by operation or handler alias and by MCP alias for server tools, so
an installed `progress` tool is recorded and projected as progress.

The task supervisor bounds Node work. The installation queue permits eight
pending preparations and serializes each resource through durable publication.
Caller-scoped topics expose progress, retain failures or uncertainty until
dismissal and remove successes. Disconnection does not cancel admitted work.

Callbacks use pinned `github.com/MoeForge/moejs v0.1.0` and unchanged SDK resources. Each invocation
captures package/settings/resource authority. SDK commands return through ordinary
Node admission, action checks, writer transactions and receipts. Read-only
callbacks cannot admit durable effects. Tools retain the turn's exact packages,
selected declarations, approvals, mode and role.

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
or unconfirmed cleanup returns `OutcomeUnknown`, without replay. Form and URL
elicitation use canonical questions and pause only active elicitation time.

Package behavior stays in the pinned plugin repository's tooling, which can use
the production moejs test host. Core tests cover host boundaries and admission;
tooling results do not substitute interactive account or platform acceptance.

## Desktop wiring

Each mount captures its Node, package digest, settings revision, surface,
worktree, session, locale and presentation generation. Node-owned
`read_plugin_view` returns the immutable entry and declared resources.
Navigation, settings, project renderers and contribution slots follow manifest
metadata. Files and Git stay native core surfaces and are excluded from every
desktop plugin slot. No package-specific renderer or workflow runs in core.

The persistent moejs owner retains the View, callbacks, modules and Promise
continuations across frames. Render and layout never execute arbitrary queued
jobs. Host completions and timers return to that owner before the next render;
closed generations reject callbacks and late results. Source loading, event
tasks and rendering use the source's respective watchdogs. Desktop entries are
trusted UI code and do not inherit the Node callback allocation ceiling.

Client supplies shared change cursors, observations and detail reads. Activity
uses the captured Node's bounded catalog/previews and Client lane aggregation,
with controller-owned labels and terminal read state. Hiding the page cancels
its previews. Status is produced by Node and observed through Client, including
the source's scheduling, failure cooldown and retained disconnected snapshot.
Intent resolution can reveal a hidden status popover without starting a model.

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
| `SegmentedTabs` | `kit.Segmented` (`fill` is `Block`) |
| `EmptyState`, empty `CardList`, empty `DataTable` | `kit.Result` (`Card` for the dashed list card) |
| loading `CardList` | a card of `kit.Skeleton` lines |
| `ModelPopup` trigger | `kit.SecondaryLook`, full width, title and caret |
| `Button` | `kit.Secondary`; `primary`, `danger` and `ghost` variants are `kit.Primary`, `kit.Danger`, `kit.Ghost`; sizes 20, 24, 32, 36 |
| searchable header filter | `kit.SelectTrigger` over a query popover |

`CardColumn` and `CardSummary` keep the source widths (128, 192, 48; title from
96) and text sizes.

## Deviations

- MyGo does not resolve a child's percentage size along a scrolling axis. The
  renderer resolves it against the scroll container's previous-frame bounds and
  builds those children once the viewport is known.
- `h_auto`, `w_auto`, `flex_initial` and an automatic `flex_basis` set nothing:
  MyGo's `Auto` height and basis stretch an element to its limit. `truncate` on
  a container reaches its text children, which MyGo's `SingleLine` does not.
- `SelectField` is not searchable and `ModelPopup` opens a menu; the source
  searches the select and opens the model picker popover.
- The browser package's settings calls (`readBrowserSettings`,
  `setBrowserPersistent`, `listBrowserProfiles`, `importBrowserProfile`) answer
  as the source host does, without its permission dialog: the system's
  Keychain prompt is the consent ([settings](settings.md#browser-page-wiring)).

- The source GPUI element contract is adapted to MyGo and Kit; imported runtime
  resources retain their notices and source revision.
- Cards with an embedded authorization browser use an isolated native MyGo page
  window on macOS. The source uses a resource pane. Other systems retain the
  explicit unavailable embedded-browser result.
- Native Files and Git remain the accepted core pages. Their package desktop
  entries and Git Status contribution are deliberately not mounted.
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
- The retired `computer.read` and `computer.control` operations are not
  registered: the reference keeps them only as unavailable metadata.

