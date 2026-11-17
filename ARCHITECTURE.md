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
                                  ├── Link / iroh ── Node B
                                  └── Link / iroh ── Node C

Mobile / Flutter ── Dart FFI ── Client ── Link / iroh ── Node A

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
Node's authenticated user. Personal plugin KV has a controller-owned source and
an execution-local view; project and conversation storage stay with their
resources. Admitted work and queued jobs retain their captured storage user.
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
| `native/iroh`, `internal/link/iroh` | Native iroh C ABI and its purego binding |
| `internal/client` | Typed commands, projections, ordering, deduplication, request and snapshot recovery |
| `internal/node` | Startup, admission, configuration, execution and resource-service composition |
| `internal/node/agent` | Turn engine, canonical records, prompt, model adapters, tools and streaming feed |
| `internal/node/store` | Turso schema, transactions, durable requests, session records and event log |
| `internal/node/program` | Package-declared executable lifecycle and process isolation |
| `internal/core` | Profile ownership, paths, confined files and common faults |
| `internal/localmodels` | Model registry, verified installation and CPU inference scheduling |
| `internal/office`, `internal/databases`, `internal/ssh` | Document extraction, database drivers and verified SSH connections |
| `internal/speech` | Controller-local speech recognition and capture |
| `internal/shipped` | Packaged resources and native library seals |
| `internal/desktop` | Feature views, view models and action contracts |
| `internal/desktop/kit` | Shared MyGo controls, themes, overlays and CrychicUI adapters |
| `internal/desktop/plugins` | JavaScript desktop runtime, SDK and native component bridge |
| `internal/desktop/live` | Client projections, live actions and scoped plugin bindings |
| `internal/desktop/preview` | Isolated fixture views and simulated actions |
| `internal/acceptance`, `internal/desktop/acceptance` | Explicit conversation acceptance and recorded UI replay |
| `cmd/sailry-desktop` | Window, local Node and shared Network lifecycle |
| `cmd/sailry-host`, `internal/hostctl` | Headless service and local management commands |
| `internal/mobile`, `cmd/sailry-mobile`, `apps/mobile` | Client handles, Dart C ABI and Flutter presentation |
| `services/pairing-relay` | Expiring short-code ticket exchange |

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
is the cgo entry point. Flutter owns presentation, while shared Client code owns
conversation reduction, paging and transfer verification.

## Data and ownership

| Owner | State |
| --- | --- |
| Controller Node | Personal configuration, authorization grants, authoritative personal KV and the unified resource directory |
| Execution or storage Node | Projects, files, sessions, canonical history, knowledge indexes, dispatch, admitted revisions and operation outcomes |
| Executor's user cache | Controller configuration, execution credentials, personal KV base and pending local writes |
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
profiles and SSH key-file credentials preserve that location when their
configuration is cached elsewhere. SSH agents also retain their authentication
Node. A different execution Node rejects the operation with a target fault
instead of interpreting the path or `SSH_AUTH_SOCK` on its own machine. Legacy
file locations resolve against the Node that owns their saved definition.

Usage statistics read each execution Node's accessible project and provider
inventory, independently of recorded usage. Filter selections retain Node plus
resource ID; project and provider queries go to that Node. Client combines
all-project reports and their request pages, preserving partial source failures
and the timestamp, Node and sequence pagination order. The statistics plugin owns
the filters and presentation, without a separate host selection.

Projects have a Node-local user owner and private or host-shared visibility.
New projects default to host-shared visibility; existing private projects keep
their visibility, including when older stored records omit that field.
Projectless resources remain personal. Projects retain their execution Node;
the rail has no global Node switch. Settings Connections opens device monitors
and pairing management without changing execution or configuration scope. A new
projectless draft chooses its execution Node with a separate control beside
the Chats location menu.

Knowledge collections likewise retain their storage Node, source location and
owning user. Independent collections default to private; shared collections are
readable by other authenticated users on that Node while management remains with
the owner. Worktree and project-storage collections inherit the project's
visibility. Conversation collections retain their session boundary. Visibility
never changes where source files, collection definitions or indexes are stored.
For example, A's private collection stored on B remains A's collection on B, and
a task running on C can reach it through A's resource entry. No additional device
grant or invitation is needed for that trusted route.

Node authenticates callers before every command family, direct subscription and
byte stream; typed resource identifiers are checked centrally and unknown commands
fail closed. Snapshots, catalogs and batch events contain only accessible
resources. Event audiences retain deletion
scope, hidden rows advance the cursor without payload, and visibility changes
invalidate projections. Buffered events and open streams recheck current access.

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

### Personal configuration and credentials

Each controller's local Node owns its personal configuration. Settings pages edit
that source; project hook settings stay on the project's execution Node. Direct
execution hosts retain a separate cache for each controller, using source IDs,
revisions and digests. Packages materialize their exact immutable archives before
dependent settings. Providers, roles, teams, connection definitions and personal
settings resolve through this cache without becoming host-local authoritative
rows. A single configuration registry owns category export, validation, events
and invalidation. Cached records are never published onward as another
controller's source. Composite exports, including package settings references,
retain their own monotonic export revision rather than reusing a connection's
business revision for changing content. This leaves connection edit CAS intact.

Remote session creation forwards frozen provider and role metadata under the
original request identity. The executor resolves their controller cache and
runtime credentials without remapping IDs or importing account grants. Missing
cache revisions reject admission; they do not trigger an on-demand configuration
copy. The source retains the forwarded metadata with its request so an unknown
outcome can be recovered after configuration changes or a restart. Historical
session-private provider records remain stored for existing sessions.

Configuration snapshots travel in bounded pages under one persisted delivery
identity. Receivers retain staged pages and their digests across restarts; the
complete snapshot, absent-record cleanup, events and completion receipt commit
together. Interrupted transfers leave the current cache intact. Page retries
reuse their original identities and completed receipts without entering the
business-effect ledger.

Model providers, channels and credentials belong to their authenticated user.
Machine administrators do not gain another user's configuration access. Legacy
provider-sharing metadata remains readable but grants no cross-user access.
Already admitted tasks retain their provider, credential identity and package
capture when the source changes.

Sign-in and renewal run on the controller. Execution hosts receive static keys
and the current runtime credentials needed for their captured operations, with
the original account identity, generation and real expiry. Refresh tokens, OAuth
client secrets, private registrations and other renewal material remain at the
source. Declared runtime header and connector slots cannot expose those fields.
Machine-bound CLI credentials stay on their own Node. The implementation stores
secrets as plaintext in private profile databases; it has no encrypted vault or
OS keychain. Public projections and diagnostics exclude secret values.

Renewable grants with a known issue time are due at
`issued_at + (expires_at - issued_at) / 4`, respecting any provider-declared
earliest refresh time. Renewal does not shorten the recorded expiry. Legacy
grants without an issue time retain their existing refresh rule; no lifetime is
invented. Refreshes are serialized by credential identity, and an uncertain
rotation is not replayed. Controller timers retry failures that sent no renewal
request, including legacy grants using their existing expiry rule. An early
refresh failure does not invalidate an access token before its real expiry;
uncertain rotations block further refresh attempts while allowing a still-valid
access token to run. Hosts continue using valid cached inputs while the
controller is offline. If a captured model credential expires, the current turn
waits at its next model step with `controller_login` state. A replacement for that
same credential resumes the same turn. An unresolved or rejected renewal keeps
the model step waiting with its reason; it does not replay the rotation.
Completed tools are not rerun to resume it.

Manual sign-in creates a new credential identity. A waiting task exposes an
explicit Restore sign-in action using the existing controller login dialog.
Its durable recovery request binds the session, turn, exact waiting-state token,
old credential and replacement credential. The executor verifies the original
controller and user, captured provider and authentication target, and matching
service-issued account and workspace fingerprints. Email and display names are
not account identities; credentials without a trustworthy stable identity cannot
be substituted. The executor records the accepted reference and recovery receipt
separately from the immutable admission capture, then continues the unsent model
step. A stopped, completed or changed task rejects a stale recovery. Repeated or
unconfirmed requests use their original request identity. Ordinary sign-in,
account switching and logout never replace credentials across existing tasks.
Plugin, MCP and connector callbacks retain their deadlines and effect recovery.

Desktop controller preferences live at
`<profile>/desktop/preferences.json`; UI layout state lives beside MyGo's
`window-state.json` in its application-data directory.

### Storage

`internal/node/store` uses the native Turso engine through the official
`turso.tech/database/tursogo` SDK for `storage/node.turso` and
`storage/knowledge.turso`, retaining the Rust filenames and file families.
Each database has one writer goroutine, MVCC mode and a separate read-only pool.
The writer groups queued operations in a
`BEGIN IMMEDIATE` transaction with a savepoint per operation. Results become
visible to callers after commit. Identifiers and event sequences advance in the
same transaction as their records.

The SDK's default `"node"` KV scope identifies the current controller's personal
namespace. On an execution host, reads use a materialized base plus pending local
writes. A transaction commits its personal writes, dispatch or conversation
changes, events and a durable outbox group in one local SQL transaction. Reads on
that host immediately see its committed writes even while the controller is
offline.

The controller later retrieves each outbox group with its original request ID,
digest, sequence and source revisions. It compares every key against the
authoritative source revision and applies the complete personal write set or
records a conflict. Predecessor receipts connect successive offline writes to
the same keys. Values retain their exact JSON bytes through durable groups,
receipts and snapshot pages. Large groups are read in bounded pages under their
original request identity. Acknowledgements reuse durable configuration paging;
the complete receipt and overlay adjustment commit together. A worker settles
an interrupted source snapshot before acknowledging newer writes so an old page
cannot replace their confirmed base. Acknowledgements update that base without erasing
later pending writes; complete source snapshots refresh the base under undecided
local writes. Each pass drains a fixed pending sequence boundary so continuous
local writes cannot starve source snapshots. Conflict decisions and one notice
commit with their deduplicated receipt. Reconciliation never replays dispatch,
notifications, project mutations or
conversation commands, and never resolves a conflict by last-writer wins.
Project KV stays in the project's Node namespace and conversation KV in its
session namespace.

Every connection uses full synchronous durability. `internal/turso` delegates SQL
and value binding to the official SDK. Its native extension supplies connection
ownership and interruption, which SDK 0.8.3 does not expose. Cancellation joins
the interrupt watcher before reusing a connection. The shipped native library
enables the same full-text index implementation as the Rust database.

Canonical conversation data uses Sailry-owned records, messages and blocks.
Binary content is stored by digest. Unknown usage counts remain unknown.
Knowledge chunks, full-text indexes and vectors are derived data; their source
files and configuration are independent. Worktree code indexes live under
`cache/code-index/<WorktreeId>/index.turso`.

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
Existing Turso files are opened in place. Startup does not move, convert or
automatically select archived databases. Preview `.sqlite` files are not selected,
converted or removed. Legacy plugin records stay
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

The model boundary supports provider SDK adapters, package-declared translations
and Agent Client Protocol programs. Fantasy supplies the per-step
`LanguageModel` stream interface; Sailry runs the loop, tools, approvals,
delegation, retries and persistence. Provider SDK retry layers are disabled.
ACP programs act as models: their file, terminal and MCP capabilities and their
own permission requests are refused. Packages declare executables and protocol
differences; the process service owns their lifecycle.

Switching models retains the same canonical session and history. Outbound
projection adapts messages and paired tool results to the selected protocol;
private reasoning is replayed only to its producing provider and model. Reviewed
capability lookup corrections recognize supported model aliases without renaming
the provider's requested model or changing stored records.

API-key model requests identify as `Sailry/<product version>` and carry the
session ID in `x-session-affinity` and `X-Session-Id`; Responses and Chat
Completions also send it as `prompt_cache_key`. A request without a session sends neither.
An API-key provider's custom headers, set in its advanced settings, replace these
defaults of any letter case. Credential and framing headers stay the
transport's. Account sign-ins keep their own client identity.

Anthropic API-key requests carry an opaque, stable session identity so compatible
account pools do not infer a new route when cache breakpoints move. Replies with
context-bound thinking retain a digest of the projected request that produced
them. When that prefix changes or an older block has no verified prefix, private
reasoning is omitted from the outgoing projection; canonical text, tool calls and
results remain intact.

Provider editing can switch a declared protocol route while retaining the
provider identity, credentials and model settings. The editor masks explicitly
read API keys and omits unchanged keys from saves. Image capability editing
distinguishes standalone image models from Responses hosted image tools, so a
conversation model is not sent to the image-model endpoint.

Admission freezes the worktree, permission, configuration, model or team,
reasoning effort, credential identity and package revisions. Renewed runtime
tokens replace an expired token for that captured identity. Explicit same-account
login recovery can record a different effective credential for a waiting run
without rewriting that admission capture. Other updates affect subsequent
admission. Policy, instructions, guides and selected
capabilities form the stable prompt; changing per-turn state is carried as agent
context. Named context sections append only changed values, with execution IDs
separate from stable instructions. Each delta records only its emitted sections;
compaction or visibility changes restore sections no longer present in model-visible
history. Existing context records remain unchanged. Automatic compaction
uses the captured model and defaults to 80% of its context window. Summaries do not
delete original records or raw tool output. Context-length failures allow up to two
reactive compactions.

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
contain attachment references rather than payloads and use no tool definitions;
loaded tool declarations are retained only where used or discovered in the tail.
An overflow recovery may summarize the full visible history while carrying the
latest user request and its pending attachments. The previous summary is never
silently omitted to make the summary request fit. A failed request leaves the
original context intact; there is no background compaction or second summary pass.

Hidden history is filtered before attachment materialization. A successful
compaction must reach a main request before another threshold compaction.
Provider-reported usage alone enters usage and cost totals; local token estimates
inform context budgeting without inventing billed usage. After a checkpoint,
context estimates count the summary and retained tail until a new main-model
measurement is available.

Request observations retain the final serialized body size, attributed text
character counts and excluded opaque payload sizes, never request bodies.
Text estimates remain separate from provider-reported tokens. Attempt-relative
timings distinguish HTTP dispatch after preparation, successful request write,
response headers and first model content; unavailable transport facts stay absent.

Model request recovery permits up to nine retries and ten minutes of cumulative
waiting. Network failures use exponential waits from one to thirty seconds;
rate limits and overload use five seconds to one minute, with jitter. Provider
wait hints are minimums, never shortened to fit the budget. Interrupted main
requests clear the failed attempt's provisional draft before retrying the same
model step; tool calls execute only after that step completes. Consumers without
a draft-reset boundary stop automatic retry after emitting content. Summary
requests buffer their output. Cancellation interrupts waits. Permanent failures
terminate the request, and completed tools are not replayed.
Responses and Anthropic streams complete at their explicit terminal event after
final output and usage have arrived; a later HTTP disconnect does not restart
that completed step. Other stream formats retain their required trailing usage
and termination handling.

Tools come from the captured enabled declarations. Everyday file, command, Git,
code and web tools are initially exposed; `tool_search` discovers other declared
tools without granting permission. Approvals settle before execution. Adjacent
read-only concurrent calls run in groups of at most eight and retain result order.
Node owns approval and question deadlines.

Standard mode (`code` on the wire) runs ordinary work. Plan is read-only with a
reviewed transition to Standard. Collaborate (`flagship` on the wire) adds autonomous
coordination under the same permission model. Package modes map to a core mode
without escalating access.
`spawn_agent`, `guide_agent` and `wait_agent` use the same execution and history
path. Children inherit captured scope and permission; read-only roles use Plan.
Team settings control child concurrency, with a default of eight per root session.

Input received during a running task is durable guidance, consumed at a model
boundary. Stop cancels work explicitly. Partial text and reasoning use a bounded
in-memory feed; committed output is canonical. Interrupted streams seal visible
text into history. Background children and commands publish durable completion
evidence, reserved to one turn and consumed with its exact canonical input.
Opted-in completions can continue an idle session through its normal queue.

## Tools and resource services

| Service | Implemented boundary |
| --- | --- |
| Commands | Workspace sandbox on macOS and Linux by default; private temporary files, read-only toolchains and no network; full permission grants the full environment |
| Files and Git | Captured paths, ignore rules, bounded reads and revision-checked writes; change sets stop at the first failure |
| Terminals | Node owns the PTY and screen mirror; local and remote controllers attach to that mirror |
| Web | Execution-Node reads and ordered search channels; direct reads pin public DNS on redirects and reject private addresses |
| Browser | Node-owned Chromium process tree and private browser profile per conversation |
| Code index | Lazy gotreesitter index, using the pinned grammar registry; bounded name-based graph heuristics without language servers |
| Local models | Verified explicit installation, one CPU inference at a time, interactive priority and idle engine release |
| Knowledge | Shared retrieval service used by package-owned features; citations recheck source identity |
| Python | Bundled runtime; lazy package environments from hash-locked requirements and prebuilt wheels |
| Session documents | Immutable bounded HTML revisions, rendered in an opaque-origin sandbox |
| Hooks | Captured worktree scripts; launched groups are not replayed after cancellation or a crash |
| Dispatch | Durable schedules driven by Sailry's calendar on standard timers |

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
terminal remains available to other controllers.

Agent file operations exclude managed-checkout descendants and protected Git
metadata. Authenticated manual workflows can access registered managed worktrees
through the scoped resource services. User-created terminals use their login-shell
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

Plugins own their feature UI, state, workflows, prompts and orchestration. Core
exposes shared scoped services through Protocol, Client and the SDK. Desktop
navigation and settings pages resolve from declarations. Workspace content,
navigation and details project into the shell's common columns; shell layout owns
resizing, dividers and visibility. SSH and database workflows use package
surfaces and reusable native components. Files and Git use the core panels; their
full-page resource lists use the shell's details column, width and fold toggle,
while conversation tabs retain their compact adaptive layout. Their package
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

Plugin package files are shared by digest. Configuration and personal KV/search
spaces belong to one controller user; executor caches remain separate from their
host's own user. Explicit project storage uses the captured accessible project
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
server, authorization and launch parameters. Failed or uncertain cleanup reports
`OutcomeUnknown`. Skills resolve `SKILL.md` and confined resources from captured
packages and grant no additional permission. Detailed declarations and examples
belong in [sdk/plugins.md](sdk/plugins.md).

## Presentation boundaries

Feature packages expose view models and `Actions`; live and preview implement the
same contracts. MyGo and Kit supply shared controls, semantic themes, focus and
keyboard behavior. CrychicUI components use the adapters where external data or
presentation needs them. Floating surfaces remain Kit's frost; right-click menus
and confirmation-only dialogs use the operating system. Forms remain Kit surfaces.

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
split groups with conversations. The header shows a title for one workspace entry
and tabs for multiple entries; selecting a split-group tab restores its focused
pane. Terminal tabs opened in the right panel stay
with that conversation and do not join splits. A terminal ID has one surface at a
time; moving it keeps its Node attachment. Closing a main surface releases its
observation while the Node process continues, available through terminal activity.
Explicitly closing a right-panel terminal stops its shell. Project overview lists
active and archived conversations separately.

Desktop reuses read-only conversation source copies until a Client observation
changes, including pagination and connection state. Completed history projections
are reused within the visible view model; controls and mutable presentation
overlays still refresh. Active runs and streaming drafts keep their timing and
update path. Leaving a conversation releases its presentation cache, and command
admission reads current Client state independently of rendering.

Terminal rendering uses MyGo's terminal package and libghostty-vt. Mermaid uses
CrychicUI, chart variants use the conversation painter over limu, and math uses
Untie. These render natively and keep source text on failure. File preview draws
text, code, Markdown, notebooks and images; unsupported formats open in the system
application. Conversation image assets open directly in the shared window image
viewer without changing the detail panel. The captured source owner supplies the
read and image actions; closing or replacing the viewer cancels its pending read.
Generated-image tool details retain status and parameters while the answer shows
the image. Its preview attachment remains available to reply embeds; the output
inventory identifies native generation by its workspace file, so publishing that
file in the answer does not add another output.
Session HTML uses a sandboxed MyGo WebView with no privileged bridge.

Voice capture, speech recognition, voice reading and global selection shortcuts
are controller-local. `--preview` opens no profile and starts no Node. Explicit
acceptance and replay run in isolated processes; their limits and commands are in
[CONTRIBUTING.md](CONTRIBUTING.md#manual-acceptance).

Mobile reserves pairing code `888888` for a local demonstration host with the
ordinary host presentation. Flutter persists this choice separately from Link
trust and supplies in-memory fixture projections through the Dart connection
contract without opening a native controller, Node, model, tool or user file.
Simulated conversations, file edits, Git actions and terminal input reset when the
app closes. Fresh controllers defer native transport until ordinary pairing; a
saved demo selection also defers it. Removing the demo restores existing genuine
peers through the normal Client and Link path without changing their trust
records.

## Pairing and operations

Each Node records users independently of device identities. Pairing and the
authenticated hello report whether the peer hosts a Node. A mobile shell uses
the entry's local user; a remote Node retains a separate user whose controller
is that peer. Existing pairings are classified on authenticated reconnection.
Classification preserves captured task owners, stored history and resource
ownership. Invitations do not select a user group or trigger a merge.

The resource catalog contains the entry itself and its directly connected hosts.
Mobile retains `{Entry, Target}` for clients, projections, notifications and
recovery. A directory member never creates another trusted peer or a recursive
directory import. Commands, subscriptions and byte streams traverse the entry
with the original request identity and actual resource target; each hop negotiates
its own compression and retains frame boundaries and stream half-closes.

The controller Node durably stores directory summaries per authenticated access
user and direct source Node. Each summary retains its entry and actual resource
owner, project and worktree identifiers and paths, knowledge directory metadata,
visibility, confirmed source cursor and last successful synchronization time.
It contains no provider keys, file bodies, session history or knowledge indexes.
Startup reads these summaries before remote Clients connect. Existing shared
Client observations update them even when no resource page is open; only a
complete successful snapshot can remove absent entries. A temporary disconnect
retains the last summary as offline. Explicit connection removal or access
revocation invalidates disk summaries, in-memory state and open projections,
without deleting resources on their owning Node.

Desktop observes the unified directory and merges by source Node and resource
ID, including sources with no connected Client yet. Offline project rows retain
their layout, expansion, selection and local drafts, show their availability,
and block remote operations. Cached running state is historical rather than a
claim of current execution. Reconnection updates the same resources and restores
operations. Mobile through the controller consumes the same summaries and
availability. A knowledge directory is not a queryable copy: executor-local
collections remain local, while collections that depend on an offline controller
return a source fault and multi-source searches retain partial failure status.

Execution Nodes use the limited resource channel to reach their captured
controller's knowledge view or the calling package's declared host handler. The
controller can serve locally or forward once to a directly connected storage or
execution Node. Forwarded calls explicitly select local handling and use the
authenticated controller's user. Plugin calls retain the verified original
package and scope; foreign turn IDs are provenance, not IDs to resolve in the
receiving database. Requests bind the actual target and original ID before
dispatch. Recovery additionally binds the exact original command and digest.
The channel cannot carry arbitrary reverse commands. Scoped knowledge requests
validate at their captured execution Node before using the controller's resource
route. A workspace UI uses the same validation path even when the selected
collection lives elsewhere: its transport route identifies the execution Node,
while its unchanged request identifies the storage Node. Stored receipts record
foreign plugin provenance separately from local resources, and recovery checks
the recorded user and the collection's current visibility. Historical sponsorship
and identity-merge records remain readable but are not replayed or used to grant
new access.

Removing an outbound host first attempts remote self-revocation, then removes
local trust even when the host is unavailable. The controller peer store or
Node store retains the exact remote request and its known or uncertain outcome
after local removal. This is diagnostic history without a retry worker; another
explicit pairing starts a new attempt and preserves earlier records.

Configuration caches and personal KV reconciliation follow the same direct host
relationships. Each host has one serial reconciliation worker, so an offline
host does not block another host. Machine settings, project data, knowledge
source files, sessions and dispatch queues remain with their owning Node.
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

Link uses iroh endpoint identity for authenticated transport. The pairing service
exchanges a six-digit, one-minute code for an opaque invitation ticket; Link
authenticates the endpoint and persists trust. Reconnect reuses that trust. An
iroh data relay carries encrypted traffic when direct transport is unavailable;
the pairing service and data relay are distinct services.

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

Host metrics use one Node sampler for local and remote reads. macOS disk rates map
APFS volumes to physical device counters; volumes sharing a physical device share
its throughput. Desktop memory diagnostics sample Go runtime classes and plugin
heap estimates while the diagnostics page is visible.

The updater verifies signed `update-v1` catalogs and archives against compiled
publisher keys. Stable selects final versions; the experience channel includes
prereleases. The implemented updater installs macOS packages and does not replace
remote Hosts. `internal/shipped` verifies packaged native libraries against their
seal; release loading is confined to packaged resources.

Computer use runs the bundled, checksum-verified `agent-desktop` helper through
stdio MCP, ordinary approvals and an exclusive lock. It uses semantic accessibility
calls. macOS permissions belong to the separately signed helper application.
The helper requires a graphical login; its absence does not prevent Node startup.

Shutdown rejects new work, closes subscriptions, joins admitted operations,
closes storage and releases the profile lock. Recovery reads durable facts without
silently rerunning uncertain effects. Notifications have one Node-owned store.
Telemetry is inactive while its switch is off. Configuration refusals carry a
bounded reason through command admission and completion. Explicit setup states
(such as missing defaults, sign-in required or a disabled provider) are warnings
kept as breadcrumbs, not standalone Sentry errors. Missing captured configuration,
missing retained credentials and unclassified failures remain errors. Remote
reports include only allowlisted configuration reasons and command stages, never
the fault message, credentials or conversation contents.

## Security and repository resources

The implemented boundaries are authenticated access, captured resource scope,
path confinement, private credentials, revision checks and explicit destructive
actions. Public web reads reject private addresses; browser content and preview
HTML are untrusted. Native permissions remain the operating system's responsibility.

The checkout contains the Flutter controller (`apps/mobile`), native build inputs
and locks (`native`), SDK and schemas (`sdk`, `schemas`), source locale catalogs
(`locales`), user guides (`docs`) and the pairing service. Product copy is generated
into `internal/desktop/i18n`. Imported code and resources retain their notices in
source, `NOTICE` and `third_party_licenses`; the application license does not replace
them. Profiles, credentials, runtime logs and build artifacts are not source.
