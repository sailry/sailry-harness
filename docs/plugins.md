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
installed package. Application version 0.1.5 matches the reviewed Desktop/Host
source and pinned packages; the protocol remains v1.

Repository discovery freezes a Git commit before inspecting or installing files.
Catalogs, signed registry skills and authenticated one-use uploads share package
validation. Local sources stay inside their captured worktree and outside the
profile. Symlinks, traversal, conflicting archive paths and oversized captures
are rejected. Installed packages are independent of their source checkout.

Settings preserve public values and explicit keep/replace/clear secret updates.
Immutable settings revisions capture declared destinations. Credentials are absent
from public snapshots and settings history. HTTP and MCP inject them only into
declared origins, servers and slots, without forwarding across redirects.

Private KV writes check key revisions. Indexed values update KV and the SQLite
fallback's FTS5 corpus in the same transaction. Conversation values follow
canonical branch history, fork and rewind. Transactions admitting a turn anchor
preceding state writes to that turn.

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

## Deviations

- The source GPUI element contract is adapted to MyGo and Kit; imported runtime
  resources retain their notices and source revision.
- Cards with an embedded authorization browser use an isolated native MyGo page
  window on macOS. The source uses a resource pane. Other systems retain the
  explicit unavailable embedded-browser result.
- Native Files and Git remain the accepted core pages. Their package desktop
  entries and Git Status contribution are deliberately not mounted.
