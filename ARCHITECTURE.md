# Architecture

Sailry has one execution runtime, Node, and two controllers, Desktop and Mobile.
Desktop includes a local Node; Host runs the same Node without a desktop window.
This document describes the implemented system. Engineering guidance is in
[AGENTS.md](AGENTS.md), development and release procedures in
[CONTRIBUTING.md](CONTRIBUTING.md), and the package API in
[sdk/plugins.md](sdk/plugins.md). Dependency versions are recorded in `go.mod`
and the native and application lockfiles.

## System context

```text
Desktop A / MyGo ── Client ── Node A (controller and resource entry)
                                  ├── local resources and execution
                                  ├── Link / libp2p ── Node B
                                  └── Link / libp2p ── Node C

Mobile / Flutter ── Dart FFI ── Client ── Link / libp2p ── Node A

Host service ── Node + Link endpoint
Node ── agent loop + resource services ── models / files / Git / PTY / SSH / databases
```

Node owns execution, configuration, credentials and durable conversation history.
Link authenticates peers and transports requests and observations. Client reduces
those observations into ordered controller state. MyGo and Flutter own view state.
The initiating controller's Node is the resource entry. Its directory combines
local resources and its directly connected hosts. Mobile connected to A uses
that same directory and reaches B and C through A. Each route retains both the
entry and the actual resource owner; executing on C does not replace A as the
controller for that task. A controller disconnect does not stop admitted work.

Official plugins and themes are separate repositories, pinned by the `plugins/`
and `themes/` submodules. `bundled.go` embeds those exact snapshots for offline
installation. Installed packages and their mutable data live in the Node profile. Immutable
package content is shared by digest; installation choices, settings revisions,
connections, writable package directories and reusable runtimes belong to the
Node's authenticated user. Personal plugin KV belongs to that user on the Node
that runs the plugin. Project and conversation storage stay with their resources.
Admitted work and queued jobs retain their captured storage user.
Dispatch handlers, schedules, events and jobs remain on their execution Node.
The shared scheduler restores those principals for callback execution and
recovery. Native automatic backups belong to the local Node owner.

## Components and dependencies

The Go module is `github.com/sailry/sailry-harness`.

| Component | Responsibility |
| --- | --- |
| `internal/protocol` | Commands, events, IDs, snapshots, faults, capabilities and JSON decoding |
| `internal/protocol/wire` | Envelopes, framing, compression and snapshot paging |
| `internal/link` | Identity, pairing, authenticated transport, receipt, reconnect and local transport |
| [Relay data service](https://github.com/sailry/sailry-relay/tree/main/data-relay) | Independently deployed Circuit Relay v2 with private device allowlists or bounded public access |
| `internal/client` | Typed commands, projections, ordering, deduplication, request and snapshot recovery |
| `internal/node` | Startup, admission, configuration, execution and resource-service composition |
| `internal/node/moduleapi` | Leaf contract (`Host`, `Module`, `Assembly`) for compile-time native Node modules |
| `internal/node/agent` | Turn engine, canonical records, prompt, model adapters, tools and streaming feed |
| `internal/node/store` | Turso schema, transactions, durable requests, session records and event log |
| `internal/node/program` | Package-declared executable lifecycle and process isolation |
| `internal/core` | Profile ownership, paths, confined files and common faults |
| `internal/localmodels` | Model registry, verified installation and CPU inference scheduling |
| `internal/office` | Document extraction. Product-specific external database drivers and SSH connections are owned by Code, not the framework. |
| `internal/speech` | Controller-local speech recognition and capture |
| `internal/shipped` | Packaged resources and native library seals |
| `internal/desktop` | Feature views, view models and action contracts |
| `internal/desktop/kit` | Shared MyGo controls, themes, overlays and CrychicUI adapters |
| `internal/desktop/plugins` | JavaScript desktop runtime, SDK and native component bridge |
| `internal/desktop/live` | Client projections, live actions and scoped plugin bindings |
| `internal/desktop/preview` | Isolated fixture views and simulated actions |
| `internal/acceptance`, `internal/desktop/acceptance` | Explicit conversation acceptance and recorded UI replay |
| `cmd/sailry-desktop`, `internal/assembly/desktop` | Runnable Harness desktop and its default native pages |
| `internal/assembly/host` | Default Harness Node assembly shared by Desktop and Host |
| `sdk/desktop`, `sdk/host`, `sdk/native`, `sdk/product`, `sdk/release` | Go contracts used by downstream product entry points, native modules, and release publishers |
| `internal/desktop/application` | Window, local Node and shared Network lifecycle for the supplied assembly |
| `modules/*/desktop` | Common native workspace and settings contributions selected by each product |
| `modules/*/node` | Common native Node modules: files, OCR, web search, memory, browsers and computer use |
| `cmd/sailry-host`, `internal/hostctl` | Headless service and local management commands |
| `internal/mobile`, `cmd/sailry-mobile`, `apps/mobile` | Client handles, Dart C ABI and Flutter presentation |
| [Relay pairing service](https://github.com/sailry/sailry-relay/tree/main/pairing) | Independently deployed expiring short-code ticket exchange |

Harness is both a framework and a runnable reference assembly. [Sailry Code](https://github.com/sailry/sailry-code) is a separate downstream module, not a fork. Its own entry points select the common components, add its developer pages and Node modules, and supply its version, package identity and release sources. The framework does not import the product. Product code cannot import its dependency's Go `internal` packages.

The principal dependency direction is:

```text
Desktop views ← desktop/live → Client → Link → Protocol
                              ↑
Mobile / Dart FFI ─────────────┘

Desktop and Host entry points → Node → agent / store / resource services
Node and Link share Protocol contracts
```

`internal/client` imports only Protocol and Link from this repository. Service
packages such as Core, SSH, databases, local models and speech do not import
Node. Desktop production packages do not start Node or import its root package;
`desktop/live` uses Node leaf value types and pure helpers from Git, knowledge,
plugins, providers and telemetry. Production startup belongs to the entry points.

Node uses MyGo's proxy-aware HTTP and headless terminal implementation. Terminal
appearance also uses MyGo color values; Node creates no application, window or
view. Desktop and Host use pure Go with purego native bindings. The Mobile C ABI
is the cgo entry point. It posts results to a Dart port through the Dart API DL
rather than calling into Dart. After a Flutter engine exits, a failed post or
the next bridge retires its runtime. A replacement controller waits in Go for
retired runtimes to release their profile locks. Flutter owns presentation,
while shared Client code owns conversation reduction, paging and transfer
verification.

## Data and ownership

| Owner | State |
| --- | --- |
| Controller entry | Direct peer directory, source availability and authenticated command routing |
| Execution or storage Node | Projects, files, sessions, knowledge, configuration, credentials, personal and scoped KV, dispatch, admitted revisions and operation outcomes |
| Execution capture | Provider, package and settings references, credential identity and request receipts |
| Client | Ordered projections, recovery state and shared detail caches |
| Desktop / Flutter | Drafts, focus, layout, selection and expansion |
| Controller device | Identity, paired peers, preferences and rebuildable caches |

Each Node profile has one owner holding the OS advisory lock on `node.lock`, one
Link identity and one endpoint. Desktop's local Client uses that owner. A
shell-only controller profile with `peers.json` cannot be opened as a Node profile.

`RouteKey{Entry, Target}` separates the controller entry from the Node that owns
the resource. Clients, projections, subscriptions, notification namespaces and
request recovery retain both. A resource reached through A and the same resource
reached through another controller have distinct controller state. Resource
identity below the target is project → worktree → session or terminal.
Navigation does not change that scope. File and Git operations retain their
captured worktree; SSH, database and plugin surfaces retain their Node and resource
identity. Registered projects remain at their original filesystem paths.

Machine paths carry their owner as `HostPath{node, path}`. File-backed database
profiles and SSH key-file credentials preserve that location. SSH agents also
retain their authentication Node. A different execution Node rejects the operation with a target fault
instead of interpreting the path or `SSH_AUTH_SOCK` on its own machine. Legacy
file locations resolve against the Node that owns their saved definition.

Usage statistics read each execution Node's accessible project and provider
inventory, independently of recorded usage. Filter selections retain Node plus
resource ID; project and provider queries go to that Node. Client combines
all-project reports and their request pages, preserving partial source failures
and the timestamp, Node and sequence pagination order. The statistics plugin owns
the filters and presentation, without a separate host selection.

API-key channels can adjust model cost estimates by a percentage, defaulting to
100%. Node uses the captured channel with the reported usage to record the
estimate at commit. Both conversation and usage reports use that recorded
estimate; later channel or catalog edits do not reprice it. Older usage without
a recorded estimate keeps the unadjusted catalog lookup. Unknown prices are not
zero charges, and catalog estimates are separate from provider invoices.
Media and search responses with recorded usage count even when input or output
tokens are missing. Request rows distinguish missing counters from reported zero;
token subtotals include only responses with both counters and carry the number
excluded. Price coverage counts responses with a known estimate independently.

Projects have a Node-local user owner and private or host-shared visibility.
New projects default to host-shared visibility; existing private projects keep
their visibility, including when older stored records omit that field.
Projectless resources remain personal. Projects retain their execution Node;
the rail has no global Node switch. Settings Connections opens device monitors
and pairing management without changing execution or configuration scope. A new
projectless draft chooses its execution Node with a separate control beside
the Chats location menu.

Knowledge collections likewise retain their storage Node, source location and
owning user. New independent collections default to host-shared. Existing
collections retain their visibility. Shared collections are readable by other
authenticated users on that Node while management remains with the owner.
Worktree and project-storage collections inherit the project's visibility.
Conversation collections retain their session boundary. A controller can browse
its own collections and accessible collections on directly connected hosts.
A task on a host uses that host's collections under its captured user.

Node authenticates callers before every command family, direct subscription and
byte stream; typed resource identifiers are checked centrally and unknown commands
fail closed. Snapshots, catalogs and batch events contain only accessible
resources. Node event streams retain a cursor watermark, not prefiltered pages.
Each delivery reads its row and current audience in the same snapshot. Historical
audiences retain deletion scope; hidden rows advance the cursor without payload,
and visibility changes invalidate projections. Open streams recheck current access.

`Session` is the common conversation container for controllers, Host automation
and plugin assistants. `StartSession` creates a session and admits its first turn
atomically. An optional immutable association (`namespace`, `kind`, `id`) links a
session to an external object without granting resource access. Defaults initialize
new sessions; revision-checked commands change existing configuration. A turn
captures its model or team, configuration, permissions and referenced revisions.
Editing a task's original input updates the visible history and admits its
replacement turn in the same session atomically. Consumed guidance belongs to
its recipient task and cannot be replaced independently. Original records remain
stored. Explicit forks and rewind backups copy history into a separate session.
History boundaries follow recorded execution order and retain consumed guidance
with its recipient turn. Queue admission order remains the public history-head
concurrency token; it does not determine which messages a history action retains.

Removing sessions covers the selected root sessions and their delegated
descendants. The Node validates the removal first; a stale or invalid removal
stops nothing. It then stops each running turn on the `stop_turn` path and
waits for every turn of the removal to settle before recording it, in the
project lane so other commands are not delayed. A turn the scheduler starts
meanwhile leaves that attempt unrecorded and is stopped and awaited the same
way. The removing transaction
discards queued input that was never admitted, ending it cancelled outside any
queue, so no scheduler can claim it later. Desktop's delete confirmation states
once how many selected conversations are running and will be stopped first.

### Personal configuration and credentials

Each Node owns its model channels, model credentials and session defaults for its
authenticated users. Desktop presents connected Nodes together, while each save,
login, renewal and default-model choice remains bound to the owning Node. Static
API-key channels can have explicit per-Node copies tracked in desktop preferences;
OAuth grants stay on one Node. Selecting a project selects that Node's channels.
New sessions cannot borrow a model channel or credential from their controller.
The ChatGPT OAuth channel discovers the signed-in account's model list using
the Node's reviewed Codex catalog version unless the channel specifies an
override. The public model catalog can supplement the account's metadata.
ChatGPT OAuth Responses requests use the captured session ID for prompt
cache routing. Native transports emit cache controls supported by the selected
protocol. GPT-6.2 Responses uses the first developer message for reusable
instructions.

Roles, teams, media assignments, plugin installs and settings, connectors,
MCP, SSH and database profiles also belong to the authenticated user on the
Node that uses them. Desktop settings share a host selection per window.
Pages with host settings place that selection in their headers.
Host-owned settings and package catalogs use that Node's Client and identity.
Settings navigation and search list the selected host's installed plugins with
compatible settings pages. Local desktop pages follow the local installation.
An open plugin page can show availability and the host selector through a switch.
Settings for desktop devices use the local controller. On the Knowledge page
the model controls follow the settings host. A fresh Node uses keyword search
until an installed embedding model is selected. The catalog spans direct hosts
and actions follow each collection's storage Node. Host administrators manage
one hooks configuration per Node through the shared settings host selector.
New host-wide configurations start disabled. A new task captures settings on its
execution Node, including tasks from sessions without a project. Admitted tasks
keep their package and settings revisions. Device monitoring has its own selection.

New sessions are created on the Node that owns the selected model. Existing
durable request ledgers continue to bind recovery to the original identity.
Desktop stages a draft's attachments on its selected Node, scoped to the
authenticated user and device. Admission of the first message creates the
session and binds those attachments to its worktree in one transaction.
After a lost acknowledgement, Desktop adopts the admitted session from the
original request receipt or a matching task observation. A first send whose
outcome is unknown remains bound to that Node and request, including across a
replacement connection. Controllers inspect that request without resending it.
New request IDs carry their creation time; ledger retention is seven days, and
a missing old record is not proof that the request was never admitted.

Model providers, channels and credentials belong to their authenticated user.
Machine administrators do not gain another user's configuration access. Legacy
provider-sharing metadata remains readable but grants no cross-user access.
The Models page lists channels on the selected Node. Removing one affects that
Node even when the channel retains its historical controller origin.
Already admitted tasks retain their provider, credential identity and package
capture when the source changes. Execution resolves that captured credential
even after the channel selects a newer key. Plugin-backed authentication checks
the current installation's availability while loading its captured package and
settings; changing a package does not replace an existing account's binding.

Each Node signs in and renews the model and connector accounts it owns.
Refresh tokens, OAuth client secrets, private registrations and other renewal
material remain on the owning Node. Declared runtime header and
connector slots cannot expose refresh material. Machine-bound CLI credentials
stay on their own Node. The implementation stores secrets as plaintext in
private profile databases; it has no encrypted vault or OS keychain. Public
projections and diagnostics exclude secret values.

Renewable grants with a known issue time are due at
`issued_at + (expires_at - issued_at) / 4`, respecting any provider-declared
earliest refresh time. Renewal does not shorten the recorded expiry. Legacy
grants without an issue time retain their existing refresh rule; no lifetime is
invented. Refreshes are serialized by credential identity, and an uncertain
rotation is not replayed. Node timers renew the selected account of each enabled,
listed channel and retry failures that sent no renewal request. Admitted tasks
can renew their retained accounts when they need them. An early
refresh failure does not invalidate an access token before its real expiry;
uncertain rotations block further refresh attempts while allowing a still-valid
access token to run. A Node can renew its model accounts while other Nodes
are offline. If a captured model credential expires, the current turn waits
at its next model step with `controller_login`
state. A replacement for that credential resumes the same turn. An unresolved or rejected renewal keeps
the model step waiting with its reason; it does not replay the rotation.
Completed tools are not rerun to resume it.

Manual sign-in creates a new credential identity. A task waiting for a credential
owned by its execution Node exposes an explicit Restore sign-in action.
Its durable recovery request binds the session, turn, exact waiting-state token,
old credential and replacement credential. The executor verifies the original
controller and user, captured provider and authentication target, and matching
service-issued account and workspace fingerprints. Email and display names are
not account identities; credentials without a trustworthy stable identity cannot
be substituted. The executor records the accepted reference and recovery receipt
separately from the immutable admission capture, then continues the unsent model
step. A stopped, completed or changed task rejects a stale recovery. Repeated or
unconfirmed requests use their original request identity. A recovery for a
historical remote capture requires a replacement already retained on the
executor. A missing replacement returns a configuration fault before durable
admission. Ordinary sign-in, account switching and logout preserve credentials
captured by existing tasks.
Plugin, MCP and connector callbacks retain their deadlines and effect recovery.

Desktop controller preferences live at
`<profile>/desktop/preferences.json`; UI layout state lives beside MyGo's
`window-state.json` in its application-data directory.
Appearance keeps interface size (`view_zoom`) separate from conversation font
size. The shell keeps saved widths and fit limits in window DIPs, allows extra
space for the conversation sidebar at larger sizes where room permits, and uses
MyGo's `Context.SetZoom` for the window content. Details widths and split shares
are not scaled with zoom.

### Storage

`internal/node/store` uses the native Turso engine through the official
`turso.tech/database/tursogo` SDK for `storage/node.turso` and
`storage/knowledge.turso`, retaining the Rust filenames and file families.
The Node creates `link.key` only for a fresh storage family. An existing Node
database must open with its original key, including when restored from backup.
Each database has one writer goroutine, MVCC mode and a separate read-only pool.
The writer groups queued operations in a
`BEGIN IMMEDIATE` transaction with a savepoint per operation. Results become
visible to callers after commit. Identifiers and event sequences advance in the
same transaction as their records.
Readers pin a snapshot before calling application code. If a local commit's
checkpoint blocks a new snapshot, the reader rolls back its read transaction and
waits for the commit to finish. The read request can cancel that wait.

The SDK's default `"node"` KV scope identifies the authenticated user's
namespace on the execution Node. Plugin writes, associated dispatch or
conversation changes and their events commit in one transaction. Project and
conversation KV retain their captured scope. New keys and search indexes stay on
the Node that accepted the write.

Every connection uses full synchronous durability. `internal/turso` delegates SQL
and value binding to the official SDK. Its native extension supplies connection
ownership and interruption, which SDK 0.8.3 does not expose. Cancellation joins
the interrupt watcher before reusing a connection. The shipped native library
enables the same full-text index implementation as the Rust database.

Canonical conversation data uses Sailry-owned records, messages and blocks.
Binary content is stored by digest. Unknown usage counts remain unknown.
Knowledge chunks, full-text indexes and vectors are derived data; their source
files and configuration are independent. Code Desktop and Host supply the
code index to their Node at startup; the Harness assembly does not. The
`sailry_no_code_index` build tag omits gotreesitter. A Node without the service
reports code queries and index settings as unavailable, preserves saved
settings, and offers plain file tools and file-only pipelines.

Code queries use live source by default. An outline parses one file. Search and
graph queries select candidates by scope and lexical matches, and hold their
parsed rows only for the query.
The live scan bounds the number of files parsed, source bytes examined, result
rows and elapsed scan time. Results report incomplete coverage when bounds or
graph selection leave part of the worktree unchecked.

A machine administrator can enable persistent search and graph indexes for the
Node. Saved project-specific selections keep their existing scope until that
switch is changed. The first such query opens the derived worktree index under
`cache/code-index/<WorktreeId>/index.turso`. Version 2 keeps typed declaration
fields and uses numeric file IDs for declaration, reference and import rows.
Recognized version 1 caches are rebuilt on first persistent use. Data, lock,
locale, vendored and minified files contribute file metadata only. Persistent
queries reconcile worktree changes alongside watcher notifications, including
changes whose notifications have yet to arrive. Index handles and watches close
after two minutes without a persistent query, checked once a minute. Startup
and hourly maintenance reclaim inactive derived indexes after fourteen days or
when aggregate cache size exceeds 512 MiB, oldest first. Both query paths share
one parser permit, bound per-file parser memory and drain idle parser arenas
after high-allocation batches and queries.

Managed checkouts use the main worktree's `.sailry/worktrees/<id>` directory,
excluded through Git's `info/exclude`. Command, browser and transfer staging uses
the profile's `tmp` directory, cleared after acquiring the profile lock at startup.
Command output is bounded both per stream and across the Node.

### Contract and storage versions

The current Link protocol is version 2. The Agent Plugins manifest and Sailry
extension contract remain version 1; they are separate version boundaries.
Desktop, Host and Mobile peers must support the same Link protocol. Compatibility
compares protocol numbers, not product version strings. The hand-maintained
decoder distinguishes the source and purpose of input:

| Input | Unknown object members | Unknown enum values or union variants |
| --- | --- | --- |
| Authored configuration, manifests, stored records and tool input | Rejected for closed objects | Rejected |
| Authenticated peer command | Ignored | Rejected |
| Authenticated peer data | Ignored | Retained; unknown union content uses `UnknownVariant` |

Required members are validated in each mode. Optional members use their declared
defaults. The wire reader selects `PeerCommand` or `PeerData`; ordinary
`json.Unmarshal` on Protocol types is `Authored`. Consumers skip unknown data
without applying an unknown operation. Signatures and digests retain their original
JSON input. Contract and storage version changes are independent.

Current Node storage uses `node.storage_generation=1`. Startup accepts an empty
database or owned storage with the expected identity and generation. An owned
Rust database without that column receives the normal additive schema default.
Unsupported generations and foreign databases fail without replacing the database.
Schema additions are rehearsed on an isolated snapshot, backed up and applied
transactionally with current-format validation. Configuration and recovery records
are checked at startup; history and captured turns are checked when read.
Active Turso files from the `v0.2.0` release onward are opened in place. A profile
without active database files starts with new Turso storage, even if it contains
an older archive or preview `.sqlite` files. Startup does not select, convert or
remove those files. Incomplete active storage still fails without replacement.
Legacy plugin records stay
preserved when the normal schema update adds per-user configuration tables;
initialization runs once with those tables so later restarts do not resurrect
removed configuration.

Source builds default to `~/.sailry-go`. Release packaging sets `core.Default` to
`~/.sailry` at link time. Development checks use isolated profiles as described in
[CONTRIBUTING.md](CONTRIBUTING.md#setup).

### Backups

Backups include both databases, referenced packages, attachments, session documents
and registered managed workspaces. Maintenance pauses transactions, checkpoints
and closes databases, then copies complete native file families (the active basename,
the sibling `.db-log`, `-wal` and `-tshm`). Both databases are reopened even when copying fails
or is cancelled. Password-encrypted age
archives support local folders, WebDAV and S3. Restore validates into an empty
profile, retains Node identity and does not start it. Automatic backups use the
Node's dispatch service. A restored identity must not run beside its original.
Listing selects the newest records across each location's pages. Retention
finishes enumerating before deleting older backups owned by the execution Node.
Workspace archives preserve relative, absolute and dangling symbolic links.
Validation rejects any archived entry beneath a link. Restore copies verified
regular files first and creates links last, so archive extraction never writes
through a restored link.

### Configuration transfer

`export_configuration` and `import_configuration` move one user's settings
between Nodes as a versioned JSON document (`internal/node/store/configuration_transfer*.go`).
The document is the unit a later sync or backup would carry; transport is out
of scope. It holds providers with their API keys, search settings and
credentials, media, compaction and team settings, roles, teams, and each
installed package's enabled, pinned and settings values and secrets. Secrets are
plaintext. Sandbox, power, Python and the personal instructions are included and
applied only for a machine administrator. Hooks, terminal, local model, code
index, network and MCP server settings are machine-bound and never included, and
account sign-ins are not carried: such providers import without a credential.
Export orders lists by ID or name, so unchanged configuration gives the same
bytes apart from `exported_at`. Import merges: entries are matched by ID (roles
and teams then by key, since built-in roles have per-profile IDs) and replaced,
new entries are added, and entries only on the target are kept. Each section is
optional and an absent one leaves its area unchanged. A refused entry is
reported as skipped; missing packages are skipped and a different package build
receives only its enabled and pinned choices. SQL changes commit together and
the instructions file is written after the commit. A preview runs the same
import and rolls it back.

## Command admission and recovery

1. Client submits a typed command with a stable request ID and captured resources.
2. Node checks access and revisions, then durably admits persistent mutations.
3. Link acknowledges safe receipt; Node executes and records the business outcome.
4. Client observes the result and ordered events, recovering by the same request
   identity after a disconnect.

Durable admission captures the authenticated user separately from its original
device caller. Internal task callbacks retain that actor after device revocation;
new public requests always resolve current membership. Captured turn, credential
and plugin storage identities remain bound to the original controller and user,
so equal revision numbers cannot substitute another user's inputs. Recovery
metadata stores command kind, actor and typed resource IDs, never authored text
or secrets. Inspecting or replaying a result rechecks current access without
repeating its effect; remote session forwarding retains its specialized recovery.

The in-process local transport fixes its authenticated caller at construction and
uses the same Node handler as remote transport. Transport receipt and business
success are separate facts. Replaying a completed durable request returns its
stored result; reusing its identity with different content produces a conflict.
An admitted request interrupted by Node shutdown becomes unknown at restart.
`InspectRequest` recovers its status without repeating the effect.

A snapshot and cursor come from one read transaction. Subsequent observations
follow the committed event log. An expired cursor causes a new snapshot, not
command replay. Failed and uncertain installations remain observable in the
bounded installation queue shared by plugins, models and language packages.

## Agent loop and model boundary

`internal/node/agent/engine` owns asynchronous turn execution. Its loop reloads
canonical state, settles approvals and tool results, consumes guidance and child
outcomes, compacts when needed and requests the next model step. Effects commit
through Store before publication. The engine keeps ownership until turn settlement
is durable and drains admitted operations before finalizing the turn.
Unavailable captured package versions settle their queued turn and retain any
unrecorded input. Other sessions on the Node can still claim their work.

The model boundary supports provider SDK adapters, package-declared translations
and Agent Client Protocol programs. Fantasy supplies the per-step
`LanguageModel` stream interface; Sailry runs the loop, tools, approvals,
delegation, retries and persistence. Provider SDK retry layers are disabled.
ACP programs act as models: their file, terminal and MCP capabilities and their
own permission requests are refused. Packages declare executables and protocol
differences; the process service owns their lifecycle.

The Node's model factory selects the transport from the binding and reports
its route: a per-prompt byte bound, whether the transport keeps a session
per Sailry session, and whether that session currently holds the history.
As in T3 Code, an ACP program keeps one agent session per Sailry session and
sends only the messages the session has not seen; requests with session
routes therefore never shorten earlier tool results. The Node records which
agent session holds each Sailry session, with the message digests it has
seen, under the package's `program-sessions` data; the record holds no
conversation text and the least recently written of 256 per program leave
first. A program started again after an idle stop, an eviction or a Node
restart reopens that session with `session/resume`, then `session/load`,
where the agent advertises them. Only when neither succeeds, when earlier
history or the system text differs (an edit, rewind or compaction), or when
another model replied since the session last answered, is the whole history
replayed into a new session; `replay_bytes` bounds that replay, and a larger
one is a context overflow handled by compaction. A replaced session is closed
when the agent offers `session/close`. A program whose copy of a session is
older than the record, because another program continued it, resumes the
session as recorded and neither continues nor closes its copy. Transcript
summary requests, discovery and a request whose agent session is busy
share any compatible program with room, each in an agent session of its own
that ends with the request. A package runs at most
four programs for each user across every session. One program serves as many
requests at once as its adapter's `concurrency` declares, one by default, each
Sailry session in its own agent session; updates are routed by agent session.
A request beyond that, such as one of several parallel delegations, waits
until a running request releases its share. A request holds a share only for
one model step, never while tools or delegations run. A cancelled or failed
prompt leaves its agent session unknown, so the next request of that Sailry
session opens a new one.

Automatic compaction on these routes uses the ordinary threshold. While the
agent session holds the history, Sailry asks for its handoff in place, so
the session receives only the summary instructions. When the session
advertised a `compact` command (`available_commands_update`), the saved
handoff is followed by a bare `/compact` in the same session; the session
then counts the retained messages as seen and receives only what follows.
That second pass over the agent's context is billed by the agent's account
and not recorded as Sailry usage. Without the command nothing is sent and the
next request replays the handoff into a new session. The handoff and retained
tail are sized for the replay bound, so that replay fits. Discovered agent
models take their window from the bundled catalog when it knows them.

Switching models retains the same canonical session and history. Outbound
projection adapts messages and paired tool results to the selected protocol;
private reasoning is replayed only to its producing provider and model. Reviewed
capability lookup corrections recognize supported model aliases without renaming
the provider's requested model or changing stored records.
Repeated provider call IDs are disambiguated by their message occurrence before
execution. Model requests render diagnostic-only tool failures as readable
messages with their code and applicable details; a failure with other fields,
parts or attachments keeps its bounded original body. Canonical responses
retain the original evidence.
Package hooks observe a bounded set of their own recent, complete tool results
from the active turn.

Anthropic API-key model discovery and conversations share the Claude Code
print-mode User-Agent, `x-app: cli`, `Accept: application/json` and
`anthropic-version: 2023-06-01`. Messages prepends the matching client identity
block while retaining the application's original system blocks, tools and cache
markers. Its beta header includes the client flag and the thinking/effort flags
used by that request, merged with configured beta values. This is shared across
Anthropic API-key connections and model IDs; it does not select OAuth, add OAuth
betas, rewrite tool names or generate billing attribution.
Anthropic discovery and Messages requests derive `/v1/models` and `/v1/messages`
from the same gateway mount. Messages uses the SDK's `?beta=true` route. Native
Anthropic search shares the same API-key identity and session metadata.
Other API-key routes identify as `Sailry/<product version>`. When a request has a
session, API-key conversations carry its ID in `x-session-affinity` and
`X-Session-Id`. Responses and Chat Completions also send it as `prompt_cache_key`.
An API-key provider's custom headers, set in its advanced settings, replace
defaults regardless of letter case; Anthropic beta tokens merge with required
request features. The transport sets credentials and framing.
Account sign-ins set their own client identity.

OpenAI API-key discovery, Responses and Chat Completions share the same base
URL. A bare origin defaults to `/v1`. An explicit path supplies the API mount,
and a full `/responses` or `/chat/completions` URL selects the mount before
that operation. Each request uses the appropriate endpoint under that mount.

Amazon Bedrock (`bedrock`) and Vertex AI (`vertex_ai`) are cloud routes that
carry another API's protocol: `ConversationModelApi.Family` reports Bedrock as
`anthropic` and Vertex AI as `gemini`. Request shaping, reasoning choices,
thinking replay, cache markers and attachment support follow the family;
transport, credentials and endpoints follow the cloud. The transport
configuration marks the route (`transportConfig.Cloud`, `Bedrock()`), so
features the cloud lacks can be decided per route. A cloud provider uses
API-key authentication and stores its region, or project and location, as
connection options; its endpoint is the service host they name, which Desktop
derives and the Node accepts as given; saving refuses one with a path.
Bedrock sends the
Bedrock API key as a bearer token through the Anthropic SDK's Bedrock
adaptation, and the configured key replaces any ambient
`AWS_BEARER_TOKEN_BEDROCK`. It omits the Claude Code identity, session metadata
and `?beta=true`, carrying the betas it needs in the body. Native web search
is not offered on either route. Vertex AI stores the service-account JSON key
as the channel credential. The Node mints cloud-platform access tokens from it
with the JWT bearer grant and caches them per key until a minute before expiry.
One request mints a replacement while others keep using the unexpired token or
wait within their own cancellation; a 401 drops the refused token. Minted tokens
are redacted from that attempt's failures. Only RSA keys are accepted, and only
HTTPS token endpoints, or loopback HTTP, receive the assertion. The genai
client receives an explicit project, location, endpoint and HTTP client, so it
reads no ambient credentials or environment endpoints. Neither route lists
models: they are entered by ID and completed from the catalog's
`amazon-bedrock` and `google-vertex` offerings. Context windows resolve
Bedrock model and inference-profile IDs (`us.`, `eu.`, `global.`, ARNs)
through the Bedrock catalog and then their Claude name.

Anthropic API-key requests carry an opaque, stable session identity so compatible
account pools do not infer a new route when cache breakpoints move. Replies with
context-bound thinking retain a digest of the projected request that produced
them. When that prefix changes or an older block has no verified prefix, private
reasoning is omitted from the outgoing projection; canonical text, tool calls and
results remain intact.
Anthropic thinking parameters use the capability flags reported by `/v1/models`
or the reviewed capability snapshot for known models. Models without either
source use their server's default thinking and effort.

Provider editing can switch a declared protocol route while retaining the
provider identity, credentials and model settings. The editor masks explicitly
read API keys and omits unchanged keys from saves. Image capability editing
distinguishes standalone image models from Responses hosted image tools, so a
conversation model is not sent to the image-model endpoint.

Each Node embeds a models.dev metadata snapshot for offline capability and
reasoning-effort lookup. Its successfully downloaded catalog takes precedence;
failed downloads retain the last usable data. The bundled source has no network
update timestamp. Desktop displays the Node's source and attempts one refresh on
entering model settings when metadata or scores have never been downloaded or
were last checked more than 24 hours ago.
Missing metadata blocks adding channels or discovering models with a prompt to
update model data; it does not block deletion. The repository refreshes the
snapshot every six hours when its content changes, and releases embed their
committed snapshot. This reference data is separate from the reviewed model
request policies under `internal/node/agent/model/data`.

Model settings separate input and native output modalities from tool,
reasoning and callable generation capabilities. Native outputs come from the
catalog and do not enable a generation endpoint. Automatic input capabilities
follow the model and channel in the owning Node's catalog, including declared
image support from native discovery.
Manual capabilities are kept on refresh. Older settings and unknown gateways
retain their text and image compatibility behavior. The captured model, native
adapter and detected file format determine whether an attachment can travel as
media. Available inspection routes stay usable through attachment references.
The turn releases cached bytes once the outgoing history no longer needs them.

Admission freezes the worktree, permission, configuration, model or team,
reasoning effort, credential identity and package revisions. Renewed runtime
tokens replace an expired token for that captured identity. Explicit same-account
login recovery can record a different effective credential for a waiting run
without rewriting that admission capture. Other updates affect subsequent
admission. Policy, instructions, guides and selected
capabilities form the stable prompt; changing per-turn state is carried as agent
context. The execution identity section includes the turn's UTC start time.
Plugin initializer text stays in agent context and fixed package guidance
follows the tools available to inference. Unavailable MCP servers contribute
no tool instructions. Named context sections append changed values. New user
input refreshes the last successfully recorded unfinished plan. The deferred
tool catalog is emitted on change or when its last copy is no longer visible.
Leading `update_plan` receipts are recorded before the rest of the tool batch.
Delegated children with the plan tool record and restore their own plan.
Earlier plans use the same history projection. Compaction or visibility changes
restore sections no longer present in model-visible history.
Automatic compaction starts at the earlier of 80% of the captured model's window
and the window minus its output reserve. A saved percentage or explicit model
threshold replaces the percentage boundary. The same threshold applies on
every route; a prompt byte bound does not move it. Summaries preserve original records and raw tool output.
Context-length failures allow up to two reactive compactions.

Desktop Conversation settings edit the selected Node profile's `instructions.md`.
Edits save when the editor loses focus, using that Node's revision.
Window close and application quit flush pending edits and wait for a save in
flight. A failed save, or a draft that cannot be saved, reports its outcome
and keeps the draft in the editor for retry; the next close or quit goes
through. Repeating the close or quit stops the wait, and the window's closing
runtime cancels the request. A session end never waits.
The Node captures that file for each new turn, including turns in existing
sessions. Each turn retains the instructions it ran with.

Project and personal instruction files are captured whole rather than using the
128 KiB file-tool page size. Reads retain file confinement, cancellation, UTF-8
validation and change detection. The existing history-frame capacity bounds raw
reads and the combined JSON-encoded configuration before persistence. Fixed
instructions are never truncated or summarized: when their offline token estimate
already fills the captured model's context window, the turn reports that capacity
problem before inference instead of attempting an ineffective history compaction.

Compaction merges the previous summary and older messages into one bounded text
handoff, retaining recent complete exchanges and tool-call/result groups in their
original records. The retained tail targets at most 20,000 estimated tokens or
20% of the model window, whichever is smaller; the newest indivisible exchange
can exceed that soft target. The summary is projected before
that tail even though its checkpoint is appended later in storage. Summaries
contain attachment references rather than payloads; transcript summary requests use no tool definitions;
loaded tool declarations are retained only where used or discovered in the tail.
An automatic cut within the current turn carries its user input and attachments.
An overflow recovery may summarize the full visible history while carrying the
latest user request and its pending attachments. Summary requests fit the
model's context budget, capped at 128,000 estimated tokens including the output
reserve, and any known transport byte limit. If the service
reports a context overflow, compaction can retry once with a smaller transcript
that retains the previous handoff. A compaction that cannot produce a usable
summary leaves the original context intact.

When the latest main request of the same provider and model read input from
the provider's prompt cache, and the history still fits with the summary
output, compaction first asks for the handoff in place: the main request's
system prompt, tools, effort and projected history unchanged, followed by the
summary instructions as a user message that quotes the opening of the
retained tail when it begins with text. Thinking, effort and tool choice belong
to the cached prefix on some providers, so this request keeps the turn's
configuration; its output limit grows by a manual thinking budget so the
budget does not change. A tool call, truncated or unparseable output, or a
rejected request falls back to the rendered transcript; rate limits,
authorization failures, exhausted retries and cancellation end the compaction
as before. The transcript request runs at the model's lowest
effort from low upward, or its lowest offered effort when nothing reaches low.

Native compaction is a method of the route the model factory constructs, not
a separate trigger. Sailry keeps one threshold and its overflow recovery and
picks the retained tail as above. A route whose protocol compacts on its own
is asked first, with the main request over the records being replaced. The
compacted checkpoint is projected in their place, followed by the tail.
Anthropic Messages routes, both API key and Claude account, send that request
with the on-demand `compaction: {type: "summarize"}` parameter and the
`compact-2026-09-04` beta header, without context editing, stop sequences, a
forced tool or structured output. They store the returned block's readable
`content` as the handoff. OpenAI Responses routes, including the ChatGPT
account route, post the model, input, instructions and cache key to the
stateless `/responses/compact` instead of using the server's automatic
threshold. They store the returned window, the retained user messages and one
encrypted compaction item, without readable text. Other HTTP protocols,
package translations and agent programs have no native method. Agent programs
keep the in-place handoff and `/compact` described above. A manual compaction
with the user's focus, or an overflow recovery, uses Sailry's handoff.

The native items are replayed unchanged only to the provider, protocol and
model that produced them, as with private reasoning. Each Messages request
that carries the block repeats the beta header. Any other route reads the
handoff text. A Responses window has none, so Sailry writes it with the
transcript request when the history is next sent to another route, or when
Sailry itself must summarize over it. That handoff becomes a new checkpoint
with the same boundary, and the producing route keeps the window. Until then,
the compaction entry shows no summary text. A history that stays on its model
pays one compaction call; a switch adds the transcript request.

A rejected endpoint, parameter, content type or beta flag (HTTP 400 or 422
naming it, or 404) falls back to Sailry's handoff. It is remembered for that
provider, protocol, endpoint and model while the Node runs, until the channel
is edited; a bounded number of routes is kept. So is a completed
response without any compaction item, which a gateway that drops the feature or
a model without it returns as an ordinary reply. A refused or empty summary
falls back without being remembered. A native request resends a provider error
once, even after waits for other causes; one that persists, or an outage, falls
back without being remembered. A rate limit, overload, spent balance or
refused account (HTTP 403) waits within the ordinary retry budget. Failures
that outlast it, balance and authorization failures and cancellation end the
compaction as for the in-place request, since they stop the replacement summary
too. Native calls are
observed and billed as compaction requests; Messages usage sums the reported
`usage.iterations`.

The active history holds stored tool payloads and asset references. A request
hydrates eligible tool results after selecting its visible, answered history.
Hidden history is filtered before materialization. In ordinary main requests,
older tool pages can be shortened at answered batch boundaries when cumulative
output reaches 96 KiB or observed input reaches half the model window. With
exact output recovery available, eligible older tool results use a short receipt
with the original call and revision. File and history pages retain pagination
facts. Successful command receipts retain the exit status and stream references.
After a checkpoint, new batches can also shorten the retained history tail.
The latest batch, errors and media keep their normal projection. A session
overview returns bounded intent and reply excerpts with evidence locations;
turn usage can be read directly without loading tool payloads. A new checkpoint resets this
boundary; summary requests and canonical history are unchanged. A successful
compaction must reach a main request before another threshold compaction.
Provider-reported usage alone enters usage and cost totals; local token estimates
inform context budgeting without inventing billed usage. After a checkpoint,
context estimates count the summary and retained tail until a new main-model
measurement is available. A reply that took several requests, such as an ACP
reply with correction prompts, bills them all but measures the window of its
last request, which an adapter reports as `context_tokens`.

Request observations retain the final serialized body size, attributed text
character counts and excluded opaque payload sizes, never request bodies.
For comparable adjacent Responses attempts, the usage view reports the shared
input prefix, added, removed and reordered input items by wire category, and
changes to instructions, tools and cache settings. Sizes count serialized JSON
value bytes. Temporary keyed digests support the comparison. A completed turn
can hand off its last measured attempt to the next turn in the same session
using a bounded, process-local cache. Upstream cached tokens are reported separately.
Text estimates remain separate from provider-reported tokens. Attempt-relative
timings distinguish HTTP dispatch after preparation, successful request write,
response headers and first model content; unavailable transport facts stay absent.
Full-request throughput divides the measured output token total by the sum of
complete request durations, including time to first output. Each sample contributes
both its reported output and its duration. Generation timing remains available for
clients that display the interval from first model output to completion.

Model request recovery permits up to ten retries and ten minutes of cumulative
waiting. Every transient failure, including rate limits and overload, uses
exponential waits from 300 milliseconds to thirty seconds, with jitter; a
provider that needs a slower pace says so through its wait hint. Provider
wait hints are minimums, never shortened to fit the budget. A model response
that sends nothing for 300 seconds fails as a retryable network error; an ACP
agent, which runs its own tools within a turn, may stay silent for 600 seconds.
Direct model requests share one connection pool that sends HTTP/2 health pings
after 15 seconds without data and retires idle connections after 30 seconds,
so a connection a gateway dropped silently fails fast instead of stalling.
The first retry is a quick resend that most transient failures clear on, so
its record is numbered 0 and the transcript shows no row for it; rows count
the nine retries after it. A first retry that must wait five seconds or more
still shows.
Interrupted main requests clear the failed attempt's provisional draft before retrying the same
model step; tool calls execute only after that step completes. Consumers without
a draft-reset boundary stop automatic retry after emitting content. Summary
requests buffer their output. Cancellation interrupts waits. Permanent failures
terminate the request, and completed tools are not replayed.
Responses and Anthropic streams complete at their explicit terminal event after
final output and usage have arrived; a later HTTP disconnect does not restart
that completed step. Other stream formats retain their required trailing usage
and termination handling.

The Node supplies core conversation tools and captures enabled package tool
declarations. Session commands use the Node's process service and workspace
permission rules. The Commands package provides desktop terminal controls.
Command input is discoverable on demand. The first request exposes everyday
file reads, discovery, search, writes, edits and patches, Git status and diff,
code search and outline, and command run, read and stop. Extra Git subcommands,
web and other lower-frequency tools stay deferred.
`tool_search` discovers other declared tools under the captured permission.
Provider prompt caches key on the declared tools and system prompt before the
history, so loading never changes either: a turn declares its initial tools,
`tool_search` and `call_tool` in capture order, and the same configuration
declares the same list in later turns and after compaction. A loaded tool's
declaration and the package instructions it selects are appended once as
owned turn context, and restored when compaction removes them while the tool
stays loaded. The model calls it through `call_tool` with its exposed name and
arguments; the response boundary turns that call into the tool's own canonical
call before approval, execution and storage, and request projection shows an
undeclared tool's stored call as the `call_tool` call that ran it. Records,
approvals, hooks and presentation therefore name the real tool.
Each tool requiring approval waits for its decision before execution. A command
run with workspace access needs no prompt, and its admitted launch also permits
stopping it or writing to it from the same session. Adjacent concurrent reads can
run together and retain call order in history. Eligible native reads can begin
as soon as the model streams their complete arguments. The Node cancels those
reads on a failed stream and writes their results only after the corresponding
tool calls commit. A concurrent read that repeats an earlier call of the same
reply, with arguments equal as JSON, does not run: its result names that call.
Writes, approvals and tool hooks retain their ordering.
Node owns approval and question deadlines.

Goal reads and updates execute in the Node. The Goals surface and continuation
policy use the same conversation values and revision fences, including the
separate confirmation turn for completion. Goals do not grant tool authority.

Standard mode (`code` on the wire) runs ordinary work. Plan is read-only with a
reviewed transition to Standard. Collaborate (`flagship` on the wire) adds autonomous
coordination under the same permission model. Package modes map to a core mode
without escalating access.
`spawn_agent`, `guide_agent` and `wait_agent` use the same execution and history
path. Collaborate can delegate with a single selected model. A child without
a role inherits that model when team instructions allow roleless delegation.
The Node binds tools to the child's own turn and worktree within the same
project. By default the child inherits the parent turn's captured tools,
including deferred tools and package tools. Child scheduling tools are excluded.
The parent's captured schemas, connection targets and MCP bindings bound the
child's calls. Explicit role tool lists narrow the catalog further. Read-only
roles use Plan, and the child's effects follow its own approval and connection
checks. Assistant selectors prioritize initial tools within this catalog.
Guidance for an active child uses that task's capture. A new task for an idle
child uses the admitting parent's capture and keeps the child's original role.
Team selection supplies named roles and optional model overrides. Code Review
requires a tool-capable captured role for new assignments when the roster is populated.
Team settings control child concurrency, with a default of eight per root session.
In Collaborate, a model's `spawn_agent` call overlaps foreground work with the
parent by default. A model's `guide_agent` call does the same when it starts a
new task in an idle child; package tools bound to delegation keep the
foreground wait. Both tools accept an explicit `overlap` setting for a new
task. The delegation record stores the effective mode, so clients show the
parent as waiting on an overlapping child only after its final reply. After guidance,
`wait_agent` can collect a dependent result. Standard and user-selected spawns keep
their existing wait defaults. A completed overlapping child can reopen a reply
at the next model boundary while other children are still running. The parent
settles after collecting its foreground children. An intentional plugin stop
also collects them and skips further model calls; guidance that arrives during
that wait ends it. When the parent makes no further model call because its
model failed, its plan was reviewed or guidance ended a plugin stop, its
undelivered foreground children detach to background delivery instead of being
cancelled: they keep running and their results arrive as a notice or with the
next input. A stop still cancels them. Automatic child results keep the first
16 KiB of the answer (falling back to text-only assistant content stripped of
tool and thinking blocks if the child hit its turn limit on a tool step) and
mark a cut with `response_truncated` and the full byte count; `wait_agent` with
the exact turn returns all of it. Delegated children do not expose `ask_user` or
`set_session_title`, so ambiguities return to the parent in the child's final
message. `read_session` detail mode omits raw `tool_call`, `tool_result` and
`thinking` blocks by default (`include_tools: true` opts back in), while exact
`entry_sequence` lookups return full entry details. Waiting for a child is
accounted for separately from active tool execution. Child commands may run
in the background while the child works. The child collects any needed results
before its task ends, when the Node stops its remaining commands.

`list_subagents` reads a bounded page of direct child sessions from durable
delegation records across turns. The active parent uses its captured delegation
scope; no caller-supplied session can change the scope. The page keeps exact
session and turn IDs, status, an assigned title and a frozen role when available,
but no task input or result text. An explicit stop is recorded with the exact
child delegation and remains available after request receipts expire.
`stop_agent` lets the parent stop a queued or running child its conversation
owns on the same path as a controller's stop. The stop is recorded, the parent
no longer waits for the child, and its cancelled result is collected with the
call instead of reopening a reply.
Listing does not resume, collect or replay work. An explicit spawn using a
listed child session starts a new child turn with its own admission receipt;
waiting only observes a specific execution.

Input received during a running task is durable guidance. Guidance is admitted
directly with the running turn's frozen settings (ignoring intervening session
configuration or revision changes), so a model or mode chosen meanwhile applies
to later queued turns, and a model removed meanwhile does not reject it. Once
committed, guidance interrupts active model streaming (preserving any streamed
text or reasoning as `interrupted` without writing an error record and dropping
partial `tool_request` blocks), foreground commands, interruptible reads and
interaction waits so the guidance joins at the next boundary. Each boundary
takes up to 16 messages or 128 KiB, at least one message; the rest joins at the
next boundary. Guidance its turn settles without joining becomes an ordinary
queued turn captured again with the session's current settings, keeping the
earlier capture only when the current settings are unavailable. A delegated
child that fails or is stopped holds its own queue, which nothing resumes, so
guidance it settles without joining ends cancelled with it; the parent sees
the child's outcome and can guide it again. Node recovery cancels such guidance
that earlier versions left queued.

While a Desktop conversation is running, Tab queues a task that can start
automatically after the active task settles. The Node keeps any existing pause.
Queue commands without automatic start create held items.

An explicit queue send (`guide_queued_turn`) to a running recipient adopts the
recipient's frozen settings and prioritizes the selected input at its next
boundary rather than stopping the recipient or splitting into a separate queued
turn. Once the queue update commits, the Node requests cancellation of active
model streaming, foreground commands, interruptible reads and interaction waits.
Already admitted noninterruptible effects settle first. Successful and uncertain
results stay in the original turn. A completed recipient can fall back to
starting the selected input in an idle, unpaused queue. A replacement run or a
stop takes precedence over this fallback.

Stop cancels work explicitly. Partial text and reasoning use a bounded
in-memory feed; committed output is canonical. Interrupted streams seal visible
text into history. Background children and commands publish durable completion
evidence. The running turn takes a bounded batch between model steps; an idle
session can take a reserved batch through its normal automatic queue. An
automatic turn can collect later evidence at the same model boundaries. Delivery
and the Node-authored evidence record commit together, without changing user
input or replaying a tool result. New evidence records opt into compact summary
projection at capture time. Foreground child results also belong to their
original row; a real `wait_agent` call still returns its own tool result. Steering
can yield the foreground wait while the parent drains completed child results at
its next safe boundary.
Active follow-ups and automatic deliveries contribute to session activity.
Held results remain available for the next input, and a descendant waiting for
input or approval remains visible when background work is active.

A user stop of a running turn also ends the session's command work, as does
Send now and the stop of a disabled or removed package's automatic turn. The stop
transaction holds the session's running and undelivered command evidence with
the `stopped` reason, and a command receipt committed after the stop is held
the same way. After the commit, the engine has the Node stop every background or
promoted command of the session except those launched with `service: true`, a
background-only flag the model sets for development servers and watchers. A
command that starts while the stop commits finds its operation cancelled and
is stopped too. Held results, including a service that later exits, start no
notice turn even if the queue resumes, report no stall, and join the user's next
message. That message also releases the hold on services still running, so their
follow-up and stall reports resume.

A foreground `run_command` is promoted, keeping its process and ID, when queued
guidance is pending after 30 seconds or when it reaches `timeout_ms`; its result
then tells the model why and to follow it with `read_command` or `stop_command`.
Hook commands keep their timeout as a stop. When the 32 background slots are
full, a timed-out command is stopped instead and its result says so.
Cancellation stops a foreground command, and a command that finishes before the
promotion takes effect returns its ordinary result.
A promoted command switches its completion capture to the most recent bytes of
each stream. Command evidence carries truncation flags and omitted byte counts
when the retained text can be compared with the byte totals. Background commands
in child sessions support polling during the child turn and stop when it settles.
The process service bounds retries for rejected completion writes and retains
the final output in memory when it cannot commit the result. After a refused
write it retries the original completion at a few spaced timer attempts without
an inspection; inspecting the command also retries it. Sandbox
approval for command input and stopping follows the live process identity.

## Tools and resource services

| Service | Implemented boundary |
| --- | --- |
| Commands | Workspace sandbox on macOS and Linux by default; private temporary files, read-only toolchains and no network; full permission grants the full environment |
| Files and Git | Captured paths, ignore rules, bounded reads and revision-checked writes. Change sets stop at the first failure. Prepare resolves an omitted edit, delete or move revision from the conversation's last observation. An omitted whole-file write revision creates the file. Full-permission operations that name an outside path use its volume root, with symbolic links resolved in path ancestors. Mixed mutations keep the session worktree's lock and admission receipt and capture checkpoints for eligible files inside it. |
| Terminals | Node owns the PTY and screen mirror; local and remote controllers attach to that mirror |
| Web | Execution-Node reads and ordered search channels; direct reads pin public DNS on redirects and reject private addresses |
| Browser | Node-owned Chromium process tree and private browser profile per conversation |
| Code index | On-demand gotreesitter queries with optional Node-wide persistence and bounded name-based graph resolution |
| Local models | Verified explicit installation, one CPU inference at a time, interactive priority and idle engine release |
| Knowledge | Shared retrieval service used by package-owned features; citations recheck source identity |
| Python | Bundled runtime; lazy package environments from hash-locked requirements and prebuilt wheels |
| Session documents | Immutable bounded HTML revisions, rendered in an opaque-origin sandbox |
| Hooks | Node scripts captured at admission and executed in the task's worktree under its workspace process policy. Recovery preserves launch claims. |
| Dispatch | Durable schedules driven by Sailry's calendar on standard timers |

Sailry separates capabilities into three layers: **Core** (Node execution runtime, agent loop, protocol/client/link, and desktop shell/kit/conversation), **Modules** (compile-time features selected by each product's own Desktop and Node entry points), and **Plugins** (installable and removable packages in `plugins/`). Common modules live in Harness; developer modules (Git, worktrees, code index, external databases, SSH and interactive terminals) live in Code. External Node modules bind through the public `sdk/native` packages; common modules bind to the same underlying `internal/node/moduleapi` contract. These modules execute directly in Go with zero JavaScript VM dispatch, provide native management commands (`read_memory_catalog`, `read_memory_entry`, `save_memory_entry`, `delete_memory_entry`, `save_memory_settings`, `read/save_computer_settings`, `read/save_external_browser_settings`) and built-in skills (`web-search:research`), and preserve legacy tool aliases (`plugin_<hash>_<name>` and `mcp_<hash>_<server>_<name>`) for historical transcripts, roles, assistants and presentation lookup. Core-adjacent features whose UI and settings are fully native (`code`, `web-search`, `external-browser`, `browser`, `computer`, `memory`) are retired from `plugins/` (`RetiredBundled()`), while companion panel packages (`files`, `git`, `worktrees`, `databases`, `ssh`, `commands`) retain their optional Desktop panels and status badges without duplicating Node tool handlers, and `office` provides bundled Office skills (`excel`, `pdf`, `powerpoint`, `word`).

On-demand code relations validate candidate files under the current worktree
access and ignore rules. A bounded memory cache reuses lexical tests and index
rows for verified file versions, and small result pages for identical selected
inputs. It retains neither source bodies nor open indexes and does not enable
the persistent index.

Hook commands receive one UTF-8 JSON object on standard input followed by EOF.
The fields are `schema_version` (currently 1), `hook_event_name`, `node_id`,
`session_id`, `turn_id`, `worktree_id`, `cwd` and, for a project worktree,
`project_id`. Tool events (`before_tool` and `after_tool`) carry `tool_name` and
the same `tool_use_id` for the two sides of one call, and each hook rule may
optionally restrict `tools` to matching canonical tool names (with legacy plugin
aliases resolved before matching). `after_tool` reports
`status` as `succeeded` or `failed`. `turn_end` reports `completed` or `failed`
for task turns that finish without a stop. Tool arguments, tool output, prompts
and credentials are outside this contract. The `SAILRY_HOOK_*` environment
variables remain available. Exit zero signals success, and a failed
`before_tool` hook blocks the call.

Dispatch handlers, schedules and jobs expose their owning `execution_node`.
New independent reminders and scheduled tasks offer a host choice from the
controller's resource catalog; project tasks stay on the project's Node. Once
created, edits, pause, deletion, run, stop and history use the recorded execution
Node. Queued jobs retain their admitted handler capture even after settings or
the visible host selection change. An unavailable owner produces an unavailable
operation, never a replacement local schedule. Session queues already belong to
their session's Node and do not need another host selector.

Terminal inventory contains live processes only. Exiting or explicitly stopping
a terminal releases its PTY, mirrors and subscriptions and removes its inventory
row. Final events and durable command outcomes remain available for observation
and request recovery; they never restart the process. Node startup retires rows
left by its previous process. Closing a view only detaches it, so a running
terminal remains available to other controllers. A terminal takeover has a durable
request receipt and advances the input revision, including when two mobile
controllers share the same entry Node. Routed mobile input uses the revision
returned by its own creation or takeover.

Agent file operations exclude managed-checkout descendants and protected Git
metadata. Authenticated manual workflows can access registered managed worktrees
through the scoped resource services. Linux anchors the ancestor mounts of
protected paths discovered by the command sandbox. The Node also runs native
Git and its helpers in Bubblewrap, including Git invoked on a newly initialized
workspace. The workspace and verified linked-worktree metadata are mounted
read-only for local reads and writable for admitted mutations. A linked
worktree from a bare repository mounts that repository at its exact path.
Local Git operations have a private network namespace. Explicit remote commands
have network access for transfers. Git receives a private home with selected
commit identity, signing and standard Git LFS settings. The Node account's
credential helpers, SSH agent and private keys stay outside the Git sandbox.
User-created terminals use their login-shell
environment. Zsh starts from the Node account's home unless its launch specifies
a startup directory; parent terminal hooks cannot replace that directory, and
the user's `.zshenv` can select a custom one. Configured HTTP proxies are trusted
to route public hostnames; failure does not silently bypass the proxy. Local model
downloads require an installation action, and an unavailable engine reports that
state.

## Extensions and plugins

Packages use the Agent Plugins manifest and Sailry extension declarations in
`schemas/`. Node owns validation, immutable resources, permissions, private KV,
callbacks, tools, dispatch and credentials. Startup installs missing defaults
without resurrecting removed packages. Bundled updates require strictly newer
SemVer precedence; same-version changes and build metadata do not offer an update.
Online packages are acquired from captured immutable commits. Failed updates keep
the installed package and its configuration.

The execution Node selects the highest compatible official release from the
complete catalog history. It excludes withdrawn records, respects the installed
package's stable or prerelease channel, and offers only strictly higher SemVer
precedence. Catalog presentation retains the market latest independently from
that candidate. Compatibility requirements survive unsupported or invalid
platform extension decoding; an ordinary package without that extension remains
distinct. Desktop inspects the exact recommended release before presenting its
permissions or offering an update, and checks local requirements for packages
with Desktop contributions. The loader checks those requirements again for
remote UI code. Bundled installation also honors withdrawals in the embedded
catalog, including offline fallback and default initialization.

The selected Node searches the community catalog from the [Agent Plugins
Directory verified index](https://agentpluginsdirectory.com/for-agents) (CC BY
4.0), and searches skills through skills.sh and SkillHub. Desktop loads the
next page near the end of the list. Catalog entries carry the directory's
publisher, picture, counts and verified mark; the Node names GitHub owner
pictures only for the default skills.sh directory. Desktop fetches pictures over
HTTPS through the system proxy, with the web read destination checks and bounds
on size and concurrency. It keeps
them only while the shown catalog names them. Package-owned skills appear in their
plugin details. Skill detail reads resolve the selected document at a captured
source revision and preserve the source's error for retry.

Installations keep exact content provenance separately from their ongoing update
source and pinned preference. Official updates continue following the official
catalog after every install. Desktop displays Node recommendations and does not
compute another market update policy. Explicit history switching uses the normal
inspection, immutable acquisition and revision-fenced installation transaction;
downgrades pin the result. Unsupported configuration or declared data constraints
refuse a switch without resetting values. Running admitted turns keep captured
code. Node-local settings changes, disabling and removal retain their existing
cancellation rules. Package retention and collection
still follow actual references, not the history list.

Plugins own their feature UI, state, workflows, prompts and orchestration. Core
exposes shared scoped services through Protocol, Client and the SDK. Desktop
settings pages resolve from the selected owner's installations. The controller
loads the exact declared view from that owner's package revision and binds its
actions to the same Client. Desktop-only settings use the local installation
and settings for a missing or unreachable remote installation show that state.
Workspace content,
navigation and details project into the shell's common columns; shell layout owns
resizing, dividers and visibility. SSH and database workflows use package
surfaces and reusable native components. The official assembly supplies fresh native
workspace and settings contributions to each window. Settings navigation and
search combine these pages with the selected Node's plugin pages. Each source
updates its own entries. The composer reads model and team choices from the
Node regardless of which management pages the product includes. Files and Git
share the core panel editing state; their
full-page resource lists use the shell's details column, width and fold toggle,
while conversation tabs retain their compact adaptive layout. Files and Git
share their own project and worktree selection per window. Their breadcrumb
opens a project's main worktree. Follow current conversation uses the
conversation's worktree. Their package
navigation and panel surfaces are not mounted. Local terminal tabs also
use the shared native terminal, which packages can embed. A package name does not
grant a privileged renderer.

`internal/desktop/plugins` maps JavaScript elements and SDK calls to MyGo and Kit.
The public module names include `gpui-kit`; the implementation is the Go bridge.
Mounted pages retain their VM and drafts across ordinary navigation. Hidden views
pause visibility-bound work; scope changes, package invalidation or closure release
their resources without stopping admitted Node tasks. Native components cover
shared editors, terminals, conversations, file previews and protected inputs.

Both Node scripts and desktop entries use moejs. Node callbacks have a 64 MiB
allocation budget, a 60-second deadline, cancellation and the engine's recursion
bound. Desktop scripts run on the UI owner between frames and are trusted UI code;
the SDK exposes scoped services rather than raw OS access. Plugin memory diagnostics
estimate reachable JavaScript heaps for mounted views, tools, callbacks and model
translations. They do not measure each plugin's complete Go or native allocation.

Plugin assistants mount the shared conversation UI with an immutable package name,
assistant ID and resource binding; each new turn freezes the current enabled,
compatible package version. Mounting alone creates no session; the first send
uses normal admission. They share the ordinary tool catalog, model selection,
mode, permissions, approvals, questions and history. The package adds assistant
context and preferred initial tools; its resource binding selects the default
target without replacing the ordinary conversation capabilities.
These sessions stay in their assistant's history rather than ordinary conversation
navigation; their transcripts remain available to explicit session links.

Model plugins handle their configured providers' messages and credentials under
the installed trust decision. Changed OAuth identity, scopes or flow require new
authorization. Declared programs start without a shell, with a scrubbed environment,
bounded execution and idle cleanup. Service connections inject credentials only
into declared origins, headers, MCP servers or environment targets.
Binding a local CLI verifies the selected account before changing the saved
connection. Switching methods drops the previous method's credentials.

Plugin package files are shared by digest. Configuration and personal KV/search
spaces belong to the authenticated user on each execution Node. Explicit project storage uses the captured accessible project
and follows its visibility. Conversation storage remains session-scoped.
Background indexing and scheduled callbacks restore their admitted user and
original storage principal. Runtime diagnostics filter instances by user.

The official WorkBuddy package contributes its channel brand through the same
declaration contract as other packages. The channel picker groups WorkBuddy
China, WorkBuddy International and CodeBuddy International under one WorkBuddy
entry while preserving each route's preset, authentication and endpoint identity.
Core owns the common picker and credentials; the package owns protocol
translation and its declared account flows.

The Node-owned MCP pool keys connections by user, package digest, settings revision,
server, authorization and launch parameters. Stdio executable names resolve on
the Node's captured PATH. Per-server environment values belong to the child
process. Windows batch launchers use the system command interpreter. The SDK
owns protocol version negotiation. Each connection admits up to eight
read-only calls; writes and catalog reads hold that connection exclusively. The
configured service-level write exclusion also spans its connections. Each call
owns its input-wait timer and cancellation. MCP input uses the SDK's originating
tool-call context; unassociated reverse input requests are rejected rather than
assigned to another session. Cancelling a read preserves the shared connection;
uncertain writes retire it without replay. Failed or uncertain cleanup reports
`OutcomeUnknown`. Skills resolve `SKILL.md` and confined resources from captured
packages and grant no additional permission. Detailed declarations and examples
belong in [sdk/plugins.md](sdk/plugins.md).

## Presentation boundaries

Feature packages expose view models and `Actions`; live and preview implement the
same contracts. MyGo and Kit supply shared controls, semantic themes, focus and
keyboard behavior. CrychicUI components use the adapters where external data or
presentation needs them. Floating surfaces remain Kit's frost; right-click menus
and confirmation-only dialogs use the operating system. Forms remain Kit surfaces.
Toasts display outcomes. Interactive recovery belongs to the original surface.

### GUI resource ownership

A mounted window, pane or route owns its subscriptions and presentation state.
Closing or replacing it releases its listeners, pending reads, decoded assets
and native view handles. An asynchronous completion must still match its owner
and resource revision before it can publish a result. The Node owns committed
history and admitted work. The draft controller owns unsaved edits across
navigation. A view can release its copy independently of either owner.

Long lists render the visible rows and a bounded look-ahead. A scroll anchor,
selection or history cursor can restore navigation after heavier row state is
released. Caches that outlive a view need an owner, an eviction bound and a key
that includes the resource scope. A data change should rebuild only the affected
projection. Transfer immutable snapshots across asynchronous boundaries and
reuse them across unchanged frames. Keep a detached copy where a consumer can
mutate data, retain a borrowed buffer or cross a trust boundary.

The plugin composer binding follows this lifecycle. Each mount owns the current
composer and model projections. A frame checks the source session, revision,
permissions and relevant UI state before building either projection. Unchanged
inputs reuse the previous result. Subscribers receive changes through a cursor.
Closing the binding drops the retained payloads and wakes outstanding readers.

### Conversation layout

Conversations center the messages and composer in the whole main pane. The
transcript's scroll viewport fills that pane, with its scrollbar at the right
edge. Wide panes show an activity card in the transcript's right margin, with a
header button that saves its visibility choice. Narrow or split panes use the
header popover without changing that choice or shifting the transcript. Loading
activity never changes that layout. During sidebar and
detail-panel transitions the transcript is laid out at its destination width and
translated into place, preserving its measured row heights. Direct window and
divider resizing follows the current width without an additional layout animation.

Project-created terminals are independent main-workspace surfaces and may share
split groups with conversations. Opening a conversation (a sidebar row, New
session, `App.OpenConversation` from links, search or notifications) gives it a
normal header tab until it is closed, one per target; one in a split shows its
group. A selection the window falls back to after data changes adds no tab, and
closing a tab leaves the session's tasks running. Main-workspace terminals keep
passing and kept tabs; selecting a split-group tab restores its focused pane.
An enlarged conversation retains its split slot and shares until restored.
The right panel's tabs, shown tab and open state belong to the active pane:
each conversation and each main terminal has its own, and a pane that never
opened the panel shows it closed. A main terminal's panel shows the project and
worktree its shell runs in, not the selected conversation's, and moving a
panel's only terminal to the main workspace closes that panel. The sidebar
selects a main terminal's row only while the conversation page shows it.
Terminal tabs opened in the right panel stay
with that pane and do not join splits. A terminal ID has one surface at a
time; moving it keeps its Node attachment. Closing a main surface releases its
observation while the Node process continues, available through terminal activity.
Explicitly closing a right-panel terminal stops its shell. Project overview lists
active and archived conversations separately, and the running terminals in the
project's worktrees whichever controller started them; opening one attaches a
main surface without starting another shell. Mobile's terminal page and a
conversation's terminal sheet end a shell only through an explicit action;
the sheet lists the terminals of the conversation's worktree, including the
scratch worktree of a chat without a project.

Client restores the previously loaded history range after a new authoritative
conversation snapshot, preserving that target even if another reconnect
interrupts paging. The last observer drops the body and may leave one of 64
range bookmarks. A reopened transcript with no saved reading position starts
at the newest page instead of downloading the previous range. Using the
Desktop transcript's jump-to-latest button releases previously paged history
when the next authoritative snapshot arrives. Earlier history remains available
by scrolling or revealing a search result. A matching history revision does not
make detached pages current. Full-range recovery can request
up to 1,000 entries per turn page; interactive reads and partial-range recovery
retain their smaller entry limit.
Node byte and wire frame limits apply to both. History pages place consumed
guidance with its recipient. The recipient's admission sequence is the
exclusive cursor for the group, and individual turns retain entry cursors.
A display history read can omit captured prompt text from large configuration
records before copying them into Go. The original record sizes still determine
pagination and the stored records remain the source for model replay.
A new subscription sends history first and reuses measured entry lengths when
fitting the enclosing snapshot. Each observed session shares one replay window:
up to 512 frames within a 4 MiB ordinary budget, plus at most one legal large
frame. Observers retain cursors rather than their own queues. A gap requires
that observer to read canonical history again. Closing the last observation
releases the window. Live activity reads runs, queue, interactions and tasks
without projecting entry bodies. Unobserved conversations skip that refresh.
Statistics are computed outside the publication lock. A bounded cache validates
turn membership, append cursors and relevant pricing dependencies in the
current read snapshot before extending completed aggregates. Metadata and
statistics are delivered on the same ordered feed.

Desktop reuses read-only conversation source copies until a Client observation
changes, including pagination and connection state. Across history pages and
streaming drafts the read-only view shares its already detached canonical entries.
Replacement snapshots and removal of a visible entry invalidate their reuse.
Mutable action reads always get independent copies.
Statistics-only changes keep the existing prepared transcript while updating
the usage display. Conversation observers reuse the detached rows and their
derived transcript data; the Mobile bridge still sends full view packets.
Mobile asks the Client to send transcript rows by source part range when no
display override is needed. The bridge carries the original parts once; Flutter
resolves them when it builds a row. Other observations keep inline row parts.
Mounted transcripts prepare Client views, history rows, references and attachment
descriptors outside the UI frame. A shared preparation lane bounds concurrent
work across windows; each mount holds one latest publication, not a snapshot
queue. Taking a publication transfers its mutable rows to one presentation.
A directory refresh transfers unchanged rows to the rebuilt view. Elapsed labels
update without rebuilding history; controls and mutable overlays still refresh.
An unmounted transcript releases its observation, widget list and row frame.
The visible disclosure owns its formatted tool detail and cancels its read when
it closes or scrolls out of view. Desktop can reuse an existing Client detail. An
expansion cache miss reads the captured revision without filling the Client
cache. Transcript attachment sources and inactive plugin card bindings follow
the loaded range. The Node's tool summary includes published file and artifact
descriptors. Older peers can require a detail read to recover those descriptors.
Open file previews retain their own sources, and card authorization recovery
keeps its original request identity.
Desktop requests a small derivative for conversation image cards. The Node
checks the source and access even on a derivative cache hit. It bounds preparation
and retains encoded derivatives by source digest and requested size. The stream's
digest describes the derivative, not the original. An older Node may send the
original for locally bounded decoding. The viewer and original file actions use
the original revision. Transcript tiles retain layout facts separately from pixels
and request their bitmaps when visible. Desktop budgets card bitmaps and their mip
levels separately from explicit previews and graphics-driver memory.
Desktop leaves collection policy to the Go runtime and any explicit `GOMEMLIMIT`:
a fixed process-wide ceiling cannot accommodate both Node tasks and mounted
histories and can force repeated collections when their live heap exceeds it.
Desktop keeps up to 64 lightweight turn-and-pixel scroll bookmarks for reopening
away from the end. Controller-owned drafts and canonical history have their own
lifetimes. Command
admission reads current Client state independently of rendering. Old background
commands open on their original tool row: the controller resolves the stored
call revision and reads output for that exact session and command. Command
cards show the last 100 retained lines. A process exit uses the standard tool
row and shows its captured output on expansion. Copy reads the full retained output
on demand when the captured result was truncated.
Closing the disclosure releases the read, not the process. Historical tool rows retain
revision-scoped summaries. Their open disclosure owns the formatted result and
cancels outstanding reads when it closes or leaves the virtualized list.
Consecutive tool calls share one folded group until text, a subagent, an
interaction or a standalone call breaks the run; the task's progress card keeps
its place without splitting it. A tool's declared `grouping` decides whether it
stands alone: file edits and launching or steering a subagent do, so a turn's
changes read apart from what it inspected. A row's name and icon come from the
call's captured display: packages declare theirs, and the Node captures native
tools' localized names from its core tool catalog, so Desktop and Mobile render
the same label. Desktop keeps its own names only for records captured without
one. The transcript is a conversation's
only scroll: an open group lays out its latest eight rows with Show earlier above
them, and a command output, diff or table shows twelve lines with Show all
opening it in place, bounded at 400 lines. Calls keep their rows as they start, settle or
fail, and the group's header shows the latest call as that call's own row
would. Opening the process of a completed reply leaves its tool groups and
results folded until chosen separately. Those choices live only in the window.
A failed call displays its error message when one is available and falls back to
its original result. A cancelled call is a plain row; a call whose outcome is
unknown is marked interrupted and its body keeps only the tool's own result.
Visible conversation image rows resolve bounded thumbnails. The viewer loads the selected
original on demand and releases it when that image leaves the viewer. Pane
previews release images after their last visible use. Installed themes validate
artwork at admission and decode the full image only when it is rendered. Each
mounted transcript reads checkpoint paths and change counts from its session's
Host, including splits and embedded conversations. The shared details panel
shows the selected session's checkpoints and requests the older inventory when
history or changes opens. Review owns the diff lines.
Open workspace editors retain file bodies and draft revisions. A single-file
diff retains the revision needed for hunk actions. Closing an editor or diff
releases its recoverable file contents while retaining workspace navigation.

Terminal rendering uses MyGo's terminal package and libghostty-vt. Desktop Markdown
and conversation visual blocks share Kit's CrychicUI Mermaid renderer. Replies
use their interactive painter for Mermaid data charts and limu to read them.
Conversation math uses Untie. These render natively and keep source text on failure. File preview draws
text, code, Markdown, notebooks and images; unsupported formats open in the system
application. Conversation image assets open directly in the shared window image
viewer without changing the detail panel. The captured source owner supplies the
read and image actions; closing or replacing the viewer cancels its pending read.
Document previews capture their entry, resource Node, worktree, path and source
revision. Relative Markdown and HTML image references remain confined to the
captured worktree and use the Client file transfer route. Native Markdown can
also load public HTTPS images from the Desktop without credentials or a referrer;
this does not grant the HTML sandbox network access. The open document's bounded
cache retains loaded images while offline; closure, source replacement or access
revocation cancels pending reads and discards stale results. Raster images and
SVG use their respective native renderers. Imported attachments without an
explicit original path cannot resolve local image references from their display names.
Generated-image tool details retain status and parameters while the answer shows
the image. Its preview attachment remains available to reply embeds; the output
inventory identifies native generation by its workspace file, so publishing that
file in the answer does not add another output.
Session HTML uses a sandboxed MyGo WebView with no privileged bridge. The host
inlines available document images as data URLs; the page receives neither a file
base URL nor network access. For a controller read of a `document`, the Node
attaches the captured package's `artifact_view` assets (model tool reads within a
turn keep the fragment alone). Desktop composes the template, styles, pinned
offline libraries and scripts into the opaque-origin frame. The frame posts
strings to a relay in the wrapper page, which Desktop drains by evaluation; it
can call nothing. Desktop accepts only form drafts, required and invalid notices,
and submissions for the exact document, revision and conversation it rendered.
It checks a submission against the fields the stored fragment declares and the
Node's latest visible revision. It then sends the answers as an ordinary user
message through the composer path, queued behind an active run. Drafts last while
the app runs. Inherited and older revisions cannot submit.

Desktop voice capture, speech recognition, voice reading and global selection
shortcuts are controller-local. Mobile has no voice-input controls, recording
dependency or microphone permission; audio and video attachments remain supported.
`--preview` opens no profile and starts no Node. Explicit
acceptance and replay run in isolated processes; their limits and commands are in
[CONTRIBUTING.md](CONTRIBUTING.md#manual-acceptance).

Mobile reserves pairing code `888888` for a local demonstration host with the
ordinary host presentation. Flutter persists this choice separately from Link
trust and supplies in-memory fixture projections through the Dart connection
contract without opening a native controller, Node, model, tool or user file.
Simulated conversations, file edits, Git actions and terminal input reset when the
app closes. Fresh controllers defer native transport until ordinary pairing.
Saved genuine peers reconnect at startup through the normal Client and Link path
and stay listed beside the demo, including when the demo is the saved selection;
the demo never changes their trust records. Mobile pages read a local query
cache before their live observations: the last confirmed host view, message
previews and recently opened transcripts render at once, then the Client's
observation replaces them. The cache keeps decoded values in memory and stores
them in a local SQLite database through drift. The FTS5 trigram index keeps a
bounded amount of recently indexed message text. Offline search limits results after
selecting the eligible conversations for the active host, project and filter.
Cached content never reports a connection or enables writes. The last confirmed
conversation snapshot is saved before a page releases its view. Forgetting a
host or losing access to it retires in-flight reads and deletes the cached
scope. An unavailable database leaves live observations and the in-memory
cache usable. The demo remains selectable but never joins the live
conversation or project catalog. Mobile keeps one host, project and worktree
selection for its top-level pages. Changing hosts clears the project scope.
Choosing a project on another host changes both together. Open conversation, file and terminal routes
retain their captured owner. General resource pickers list registered project
worktrees; explicit conversation routes retain access to their own scratch
directories. A missing workspace does not hide the project selection entry.
Opening a project resources page and switching between its worktrees leaves the
top-level chat selection in place. Project forms retain unresolved writes on the
original host connection and inspect the captured request when reopened.
Desktop and Mobile admit the first message, its attachments and the new
conversation's configuration in one Node transaction. Once admission is
confirmed, opening the conversation uses its returned ID. An unfinished send
in an existing conversation retains its request on the connection while its
page is closed. Reopening queries the original Node outcome.
Mobile preserves the covered task catalog during navigation, resumes preview
reads after the return transition, and bounds list previews without changing
canonical history. Desktop and Mobile use Client's activity lanes for task
status. The Client includes session lanes in each host view so Mobile can filter
them by project and status. The local demo supplies fixture lanes with its
simulated sessions.
Android system alerts reconcile posted notices against the Client inbox for
their original entry and target, including after the app restarts. Each route's
first confirmed inbox establishes which notices are already present.
Mobile registers saved direct hosts from local pairing metadata before completing
startup, then refreshes remote directories in the background. Directory reads
run outside the pairing-change queue, and reads spanning a pairing change cannot
replace the current directory or close newer Clients.
On a network change or return to the foreground, Link verifies each cached
connection with one inspection and drops any that does not answer within three
seconds, then each Client ends its reconnect waits and redials at once. A reply
to a snapshot or history read lost with its connection is Unavailable and is
retried; it never ends observation as an unknown outcome.

## Pairing and operations

Each Node records users independently of device identities. Pairing and the
authenticated hello report whether the peer hosts a Node. A mobile shell uses
the entry's local user; a remote Node retains a separate user whose controller
is that peer. Existing pairings are classified on authenticated reconnection.
Classification preserves captured task owners, stored history and resource
ownership. Invitations do not select a user group or trigger a merge.
Each endpoint commits the pairing relationship and address together. Current
claimants acknowledge an invitation after their local commit. A claimant with
an uncertain outcome can retry the same invitation while it remains valid.

The resource catalog contains the entry itself and its directly connected hosts.
Mobile retains `{Entry, Target}` for clients, projections, notifications and
recovery. A directory member never creates another trusted peer or a recursive
directory import. Commands, subscriptions and byte streams traverse the entry
with the original request identity and actual resource target; each hop negotiates
its own compression and retains frame boundaries and stream half-closes.
Large resource catalog replies use the bounded, assembled paging path.

The controller Node durably stores directory summaries per authenticated access
user and direct source Node. Each summary retains its entry and actual resource
owner, project and worktree identifiers and paths, knowledge directory metadata,
visibility, confirmed source cursor and last successful synchronization time.
It contains no provider keys, file bodies, session history or knowledge indexes.
Startup reads these summaries before remote Clients connect. The shared
observation follows resource and connection changes even when no resource page
is open. Reading the catalog consumes the projection. Complete refreshes persist
the latest synchronization metadata. Changes to directory content or
availability emit invalidations.
A complete successful snapshot can remove absent entries. A temporary disconnect
retains the last summary as offline. Explicit connection removal or access
revocation invalidates disk summaries, in-memory state and open projections,
without deleting resources on their owning Node.

Desktop observes the unified directory and merges by source Node and resource
ID, including sources with no connected Client yet. Offline project rows retain
their layout, expansion, selection and local drafts, show their availability,
and block remote operations. Cached running state is historical rather than a
claim of current execution. Reconnection updates the same resources and restores
operations. Mobile through the controller consumes the same summaries and
availability. Directory summaries carry metadata. Searches read the owning Nodes, and
multi-source searches retain partial failure status.

Interactive controller lists and searches aggregate accessible sources. An
unscoped desktop-owned view can address resources or declared package handlers on
a directly connected host. A host-owned view, workspace or task resolves
knowledge and handlers on its execution Node. A host receives no new access to the controller or to
another host through this channel. Desktop keeps the combined project, model and
knowledge lists; operations use each item's owner and saved revision.
Scoped requests retain the authenticated user, package and workspace. The
destination checks current access before execution and again before returning
a stored receipt. Recovery retains the original command, target, request ID
and digest.

Removing an outbound host removes local trust immediately. If an authenticated
connection is already live, Link retains it only for one background
self-revocation with a ten-second total budget; offline hosts are not dialed.
The controller peer store or Node store retains the exact remote request and its
known or uncertain outcome after local removal. This is diagnostic history
without a retry worker. Explicit re-pairing cancels an outstanding notification
before starting a new relationship and preserves earlier records.

Knowledge lists and queries aggregate accessible sources through the controller,
retaining each source's Node, continuation and any failure. Aggregate paging
reads enough of each source's ranking to continue beyond its first page. Cached
directory entries can remain visible while offline, with operations unavailable;
authentication and access failures clear that source's cached private entries.
Cached entries are not evidence that a read or mutation succeeded.
The source Node rechecks visibility and revision before returning a citation.

Peer trust records an inbound controller, an outbound host, or both directions.
An invitation grants inbound controller access on the inviter and outbound host
access on the claimant. A separate invitation in the opposite direction combines
the permissions. Paired endpoint inspection remains available for device metadata;
commands, observations and byte streams enforce the execution direction. The
authenticated `has_node` classification distinguishes another controller Node
from a shell without merging users. Existing direct relationships are preserved;
historical directory imports do not become direct trust. The connection menu
exposes host-only/controller-only choices and marks bidirectional access.

Link authenticates each peer with its existing Ed25519 Node identity over
libp2p. A paired peer can resume over Circuit Relay v2 while Link tries its
current TCP or QUIC listener on the local network. This LAN attempt is separate
from DCUtR, which can use suitable observed addresses across NATs. A failed
attempt leaves the circuit usable. Once a direct connection is authenticated,
new streams prefer it; streams already on the relay stay there and are not replayed.

No public data relay is selected automatically. Direct and custom are local
choices; the Node loads named relays only from
`https://sailry.dev/catalogs/network-presets-libp2p.json`. A failed refresh keeps
the last valid listing for that Node process, and a valid refresh replaces it.
The website still serves `assets/catalogs/network-presets.json` for Iroh clients;
the app does not embed it. Controllers resolve new shorthand relay inputs before
saving them; Node settings and Link continue to use peer-addressed multiaddrs.

The standalone `sailry-relay` runs the same Circuit Relay v2 service in `start`
and legacy `serve`. `start` validates TCP/TOML configuration before opening the
relay identity; `serve` does not read TOML. Relay admission is bounded public
access or a private PeerID allowlist, not token-based admission. This does not
change trust or credentials on the Nodes whose traffic it carries.

One supervisor owns the relay process. An external manager can run `start`
directly, or the Linux installer can enable a single non-root systemd unit at
boot and configure it to restart after exit. Managed configuration and program
files are administrator-owned; the dedicated service account owns its private
identity directory. Applying an explicit update replaces only the executable,
restarts the service or starts it if stopped, and authenticates the existing
PeerID. If activation fails, the updater restores the previous binary and
attempts to return the service to its prior running or stopped state.

The pairing service only exchanges a six-digit, one-minute code for an opaque
invitation. It does not carry application traffic. The device key, trust and
request recovery remain independent of addresses. Existing Iroh address hints
and relay URLs are retained in storage but cannot dial libp2p peers; pair again
or update the peer address after both endpoints have upgraded.

A pairing code connects to an already-running Node and follows the relay address
in its invitation; the claiming device does not choose or change its own relay
selection. Relay selection belongs to the Node whose address is being published,
including when a Host generates a code, or to an SSH/WSL installation before its
new Node starts. SSH installation supports password, private key, key file and,
where available, the installing Node's SSH agent as authentication methods.

Desktop windows share one Network and its remote Clients. Peer discovery runs on
changes and periodically. Only compatible execution peers become Node Clients;
paired controllers remain devices. Node relay changes use revision-checked durable
commands. Cloudflare and EdgeOne pairing adapters share the ticket-exchange logic.

Host starts with an explicit profile, network configuration and opt-in pairing.
Add Host installs checksum-verified assets over Node-owned SSH, starts systemd or
launchd and pairs through a private bootstrap ticket. Windows controllers deploy a
Linux Host into WSL2. The installed `sailry` command offers an interactive menu;
`link` selects the Host's network before publishing a pairing code. `share` remains
an alias using the current network. Host's `link`, `share`, `submit`, `wait`,
`users` and `devices` management commands use the running owner's private Unix
socket and never create another storage owner.
Both published-package and local-artifact installers roll back the program,
command link and service they created when a fresh installation fails. Only an
empty newly created profile is removed; written data remains for inspection.
SSH updates compare the verified installed version again under the shared
installation lock before replacing the running Host.
Uncertain service cleanup retains recovery files and reports an unknown outcome.

Host metrics use one Node sampler for local and remote reads. macOS disk rates map
APFS volumes to physical device counters; volumes sharing a physical device share
its throughput. Desktop memory diagnostics sample Go runtime classes and plugin
heap estimates while the diagnostics page is visible.

The updater verifies signed `update-v1` catalogs and archives against compiled
publisher keys. Stable selects final versions; the experience channel includes
prereleases. Installation targets the local macOS app. The updater keeps a
backup and commits the replacement in one same-volume exchange.
`internal/shipped` verifies packaged native libraries against their seal.
Release loading is confined to packaged resources.

Computer use runs the bundled, checksum-verified `agent-desktop` helper through
stdio MCP, ordinary approvals and an exclusive lock. It uses semantic accessibility
calls. macOS permissions belong to the separately signed helper application.
The helper requires a graphical login; its absence does not prevent Node startup.

Shutdown rejects new work, closes subscriptions, joins admitted operations,
closes storage and releases the profile lock. Recovery reads durable facts without
silently rerunning uncertain effects. Notifications have one Node-owned store.
Official products share Sentry and Telemetry.sh projects. Both transports include a
build-owned `product` field; `application` identifies the reporting Desktop or Host
independently of product. Automatic error reporting is inactive while its switch is off. Configuration
refusals carry a bounded reason through command admission and completion. Explicit
setup states (such as missing defaults, sign-in required or a disabled provider)
are warnings kept as breadcrumbs, not standalone Sentry errors; Desktop logs every
`not_configured` failure, such as a missing Git executable, as a warning. Missing captured configuration,
missing retained credentials and unclassified failures remain errors.

Two Sentry groups need more evidence than their titles provide:

- `command.guide_queued_turn:conflict` does not by itself identify a stopping
  recipient. Desktop directs guidance only to a running task; the queue or
  recipient can still change before Node accepts it. For failed
  `guide_queued_turn` commands with `conflict` or `revision_conflict`, Node
  counts five exact race reasons separately: `queue_revision_changed`,
  `queue_item_not_pending`, `queue_input_consumed`, `recipient_stopping` and
  `recipient_not_running`. One race is a warning; the tenth of the same reason
  in a five-minute window is eligible for one report with `count=10`. This is
  a reporting threshold, not a limit on user actions. Other queue commands do
  not trigger it. A non-task recipient, a scope mismatch or an unclassified
  conflict remains an error. Initial reports are capped at ten per fingerprint
  per hour, with at most 32 in flight.
- `desktop.workspace_action_failed` names a generic toast, not a cause. The
  `unspecified` code means no error was supplied to the reporter. Read
  the operation, fault code and stage: a refused write records `write_file`
  at `workspace_mutation`; a Save all that cannot start records
  `save_all_documents` at `preflight` with `draft_not_saveable`; an unmapped
  Markdown selection records `edit_markdown_source` at `map_selection` with
  `selection_unmapped`. Failed file searches record the command at `file_search`,
  not the query. Invalid search input and setup prompts are not failures.
  Controller reads of `list_directory`, `inspect_git`, `list_worktrees` and
  `search_files` retain local Node diagnostics without a duplicate automatic
  Node report.
  Desktop conflicts, uncertain outcomes and cancellations are warnings. Plugin
  error toasts keep their stable toast ID and fault code.

For covered queue and workspace commands, Node adds a fresh random UUIDv4
(`report_ref`) to the delivered fault and its log record, not to the stored
receipt. Match that value to the Desktop failure to trace one delivery. It is
not a request, session or user ID, is not reused on redelivery and is not part
of the Sentry grouping fingerprint. Stack frames enter the fingerprint by module,
function and file, not line, so a release does not regroup an unchanged failure. Only allowlisted fields leave the log;
fault text, paths, credentials and conversation contents do not.

Only a failed automatic **operation report** can enter the retry queue; Node
never replays the command. Selected transient network errors and HTTP 408, 429,
500, 502, 503 or 504 qualify. An explicit `X-Sentry-Rate-Limits` quota is not
retried. The minimum delay is five minutes; a valid `Retry-After` can extend it
within the 24-hour lifetime. There are at most three retries after the original
send. The profile's `tmp` holds at most 32 private records of 2 KiB each
(64 KiB total). Each stores the allowlisted classification, event ID and
fingerprint, timestamps, retry count and a binding hash, not an envelope,
stack, breadcrumbs or attachment. Retries keep the original event ID and
fingerprint. Expired, invalid and excess records are discarded. A restart can
recover records left by an interrupted process only for the same installation,
DSN, product, application and release. Legacy retry records belong to Harness. Retry delays start at five minutes, then grow to
at least 30 minutes and three hours. The process reads `SAILRY_SENTRY_DSN` at startup;
a nonempty override replaces the compiled endpoint. Turning off automatic errors,
opting out via `SAILRY_TELEMETRY=off` or `DO_NOT_TRACK=1`, or starting without
a DSN leaves no active retry queue. Disabling reporting or closing normally clears the slots;
changing identity or DSN never sends an old report under the new binding.

## Security and repository resources

The implemented boundaries are authenticated access, captured resource scope,
path confinement, private credentials, revision checks and explicit destructive
actions. Public web reads reject private addresses; browser content and preview
HTML are untrusted. Native permissions remain the operating system's responsibility.

The checkout contains the Flutter controller (`apps/mobile`), native build inputs
and locks (`native`), SDK and schemas (`sdk`, `schemas`), source locale catalogs
(`locales`) and user guides (`docs`). The pairing service ships from the Relay repository. Product copy is generated
into `internal/desktop/i18n`. Imported code and resources retain their notices in
source, `NOTICE` and `third_party_licenses`; the application license does not replace
them. Profiles, credentials, runtime logs and build artifacts are not source.
