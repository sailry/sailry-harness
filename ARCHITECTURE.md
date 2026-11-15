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
Desktop / MyGo ── Client ──┬── in-process ── local Node
                           └── Link / iroh ── remote Node

Mobile / Flutter ── Dart FFI ── Client ── Link / iroh ── Node

Host service ── Node + Link endpoint
Node ── agent loop + resource services ── models / files / Git / PTY / SSH / databases
```

Node owns execution, configuration, credentials and durable conversation history.
Link authenticates peers and transports requests and observations. Client reduces
those observations into ordered controller state. MyGo and Flutter own view state.
Mobile connects directly to a Node; Desktop is not its gateway. A controller
disconnect does not stop admitted work.

Official plugins and themes are separate repositories, pinned by the `plugins/`
and `themes/` submodules. `bundled.go` embeds those exact snapshots for offline
installation. Installed packages and their mutable data live in the Node profile. Immutable
package content is shared by digest; installation choices, settings revisions,
connections, writable package directories and reusable runtimes belong to the
Node's authenticated user. Default plugin KV is personal to that user and Node.
Packages may declare project collections shared across authorized project users;
conversation storage retains its session scope. Admitted work preserves its
captured storage user independently of later identity merges. Dispatch handlers,
schedules, events and jobs use the live user namespace; every queued job also
retains its original storage user. The shared scheduler restores both principals
for callback execution and recovery. Native automatic backups belong to the local
Node owner.

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
| `internal/node/store` | SQLite schema, transactions, durable requests, session records and event log |
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
| Node | Sessions, canonical history, configuration, credentials, admitted revisions and operation outcomes |
| Client | Ordered projections, recovery state and shared detail caches |
| Desktop / Flutter | Drafts, focus, layout, selection and expansion |
| Controller device | Identity, paired peers, preferences and rebuildable caches |

Each Node profile has one owner holding the OS advisory lock on `node.lock`, one
Link identity and one endpoint. Desktop's local Client uses that owner. A
controller profile with `peers.json` cannot be opened as a Node profile.

Resource identity is Node → project → worktree → session or terminal. A hidden
navigation layer does not change that scope. File and Git operations retain their
captured worktree; SSH, database and plugin surfaces retain their Node and resource
identity. Registered projects remain at their original filesystem paths.

Projects have a Node-local user owner and private or host-shared visibility.
Projectless resources remain personal. Node authenticates callers before every
command family, direct subscription and byte stream; typed resource identifiers
are checked centrally and unknown commands fail closed. Snapshots, catalogs and
batch events contain only accessible resources. Event audiences retain deletion
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

Credentials are stored in the execution Node's configuration database as plaintext.
The implementation has no encrypted vault or OS keychain. Public projections and
diagnostics exclude credentials. Desktop controller preferences live at
`<profile>/desktop/preferences.json`; UI layout state lives beside MyGo's
`window-state.json` in its application-data directory.

### Storage

`internal/node/store` uses SQLite for `storage/node.sqlite` and
`storage/knowledge.sqlite`. Each database has one writer goroutine, WAL mode and a
separate read-only pool. The writer groups queued operations in a
`BEGIN IMMEDIATE` transaction with a savepoint per operation. Results become
visible to callers after commit. Identifiers and event sequences advance in the
same transaction as their records.

Writer connections attempt automatic checkpoints once the WAL reaches 256 pages,
spreading checkpoint I/O across commits while retaining full synchronous durability.

Canonical conversation data uses Sailry-owned records, messages and blocks.
Binary content is stored by digest. Unknown usage counts remain unknown.
Knowledge chunks, full-text indexes and vectors are derived data; their source
files and configuration are independent. Worktree code indexes live under
`cache/code-index/<WorktreeId>/index.sqlite`.

Managed checkouts use the main worktree's `.sailry/worktrees/<id>` directory,
excluded through Git's `info/exclude`. Command, browser and transfer staging uses
the profile's `tmp` directory, cleared after acquiring the profile lock at startup.
Command output is bounded both per stream and across the Node.

### Contract and storage versions

The current Protocol, wire and plugin contracts use version 1. Link compatibility
compares protocol versions, not product version strings. The hand-maintained
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
JSON input. These are current decoding semantics, not a restriction that future
contracts must remain at version 1.

Current Node storage uses `node.storage_generation=1`. Startup accepts an empty
database or owned storage with the expected identity and generation. Missing or
unsupported generations and foreign databases fail without replacing the database.
Schema additions are rehearsed on an isolated snapshot, backed up and applied
transactionally with current-format validation. Configuration and recovery records
are checked at startup; history and captured turns are checked when read.
Recognized database files from another storage engine are moved unopened into
`storage/rust-archive/`; they are not imported or deleted. Before moving them,
startup copies the existing plugin packages and attachments into the same archive.
The active directories remain available to any current database, whose normal
collection can then remove unreferenced resources without losing the archived copy.

Source builds default to `~/.sailry-go`. Release packaging sets `core.Default` to
`~/.sailry` at link time. Development checks use isolated profiles as described in
[CONTRIBUTING.md](CONTRIBUTING.md#setup).

### Backups

Backups include both databases, referenced packages, attachments, session documents
and registered managed workspaces. Maintenance pauses transactions, checkpoints
and closes databases, then copies complete file families. Password-encrypted age
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
new public requests always resolve current membership. A group merge canonicalizes
authorization while retaining immutable turn and plugin storage principals, so
equal revision numbers cannot substitute another user's credentials. Recovery
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
reasoning effort, credentials and package revisions. Updates affect subsequent
admission. Policy, instructions, guides and selected capabilities form the stable
prompt; changing per-turn state is carried as agent context. Automatic compaction
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

Model request recovery permits up to nine retries and ten minutes of cumulative
waiting. Network failures use exponential waits from one to thirty seconds;
rate limits and overload use five seconds to one minute, with jitter. Provider
wait hints are minimums, never shortened to fit the budget. Interrupted main
requests clear the failed attempt's provisional draft before retrying the same
model step; tool calls execute only after that step completes. Consumers without
a draft-reset boundary stop automatic retry after emitting content. Summary
requests buffer their output. Cancellation interrupts waits. Permanent failures
terminate the request, and completed tools are not replayed.

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
package navigation and panel surfaces are not mounted. Local terminal tabs also
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

Plugin package files are digest-shared. Installations, enablement, settings,
connections, dispatch handlers and personal KV/search spaces belong to one user.
The existing node storage scope is personal; explicit project storage uses the
captured accessible project and is shared only through that project's visibility.
Conversation storage remains session-scoped. Background indexing and scheduled
callbacks restore their admitted user and original storage principal. Runtime
diagnostics filter instances by user.

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
split groups with conversations. Header number buttons switch those groups and
restore each group's focused pane. Terminal tabs opened in the right panel stay
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

Each Node records local users independently of device identities. The local user
is an administrator; existing paired devices migrate into that user without
losing access. The sharer chooses another user or my device before issuing an
invitation. Another user creates a separate group for an unknown inbound device;
my device joins the sharer's group. Repeat pairing preserves existing membership.
The first separate paired user also becomes an administrator. Node resolves each
authenticated caller to this local principal before resource and configuration
access. Device revocation preserves data; removing a user's last inbound device
requires explicit confirmation and leaves an orphan an administrator may assign.

A source sponsors its own explicit my-devices on directly paired outbound hosts.
An unknown device can join the sponsor's group; a device already in another group
gets a pending proposal and must confirm after its own controller verifies its
authenticated my-device relationship with that source. Confirmation merges the
whole original group, devices and owned resources in one transaction. Target
personal conflicts win, source originals remain archived, and immutable admitted
plugin settings retain their original storage principal. The local administrator
flag can move to the surviving group; legacy singleton and search namespaces keep
their original owner rather than following that flag.

The source directory contains itself and directly paired outbound hosts, with
their last authenticated display names. Link retains those names with peer
metadata so an introduced offline host can be identified before it connects. A
verified my-device controller imports those hosts as direct outbound connections,
deduplicated by endpoint identity. Direct pairs and other-source origins survive
source removal; explicit local removals are not undone by refresh. Source
revisions discard stale directory replies. Durable sponsorship and revocation
deliveries retain exact request IDs across offline recovery, and membership
generations prevent delayed grants or revocations restoring removed access.
Directories and sponsorship do not propagate another inbound user's namespace
through a Node's single Link identity, and they do not form a mesh.

Removing an outbound host first attempts remote self-revocation, then removes
local trust even when the host is unavailable. The controller peer store or
Node store retains the exact remote request and its known or uncertain outcome
after local removal. This is diagnostic history without a retry worker; another
explicit pairing starts a new attempt and preserves earlier records.

Personal configuration follows a single selected source over direct authenticated
host connections. Per-host switches control configuration and static secrets
independently. Manifests reconcile versions, deletions and digests in dependency
order; exact package archives materialize before their settings. Source-marked
copies are read-only, local conflicts remain intact, and independent account
authorization is listed as pending rather than copying renewable login tokens.
MCP servers can retain explicit local overrides. Disabling follow keeps the last
usable configuration; detaching converts copies to editable local records.
Captured tasks continue using their frozen revisions. Machine settings and plugin
business data do not travel with configuration.

Peer trust records an inbound controller, an outbound host, or both directions.
An invitation grants inbound controller access on the inviter and outbound host
access on the claimant. A separate invitation in the opposite direction combines
the permissions. Paired endpoint inspection remains available for device metadata;
commands, observations and byte streams enforce the execution direction. Legacy
Desktop peers retain both directions until narrowed; authenticated inspection can
narrow a legacy nonexecution endpoint to controller without changing membership.
The existing connection menu exposes host-only/controller-only choices and marks
legacy bidirectional access. Headless Host peers migrate to controllers.

Link uses iroh endpoint identity for authenticated transport. The pairing service
exchanges a six-digit, one-minute code for an opaque invitation ticket; Link
authenticates the endpoint and persists trust. Reconnect reuses that trust. An
iroh data relay carries encrypted traffic when direct transport is unavailable;
the pairing service and data relay are distinct services.

Desktop windows share one Network and its remote Clients. Peer discovery runs on
changes and periodically. Only compatible execution peers become Node Clients;
paired controllers remain devices. Node relay changes use revision-checked durable
commands. Cloudflare and EdgeOne pairing adapters share the ticket-exchange logic.

Host starts with an explicit profile, network configuration and opt-in pairing.
Add Host installs checksum-verified assets over Node-owned SSH, starts systemd or
launchd and pairs through a private bootstrap ticket. Windows controllers deploy a
Linux Host into WSL2. Host's `submit`, `wait`, `share`, `users` and `devices` management commands use
the running owner's private Unix socket and never create another storage owner.

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
Telemetry is inactive while its switch is off.

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
