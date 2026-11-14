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
installation. Installed packages and their mutable data live in the Node profile.

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

`Session` is the common conversation container for controllers, Host automation
and plugin assistants. `StartSession` creates a session and admits its first turn
atomically. An optional immutable association (`namespace`, `kind`, `id`) links a
session to an external object without granting resource access. Defaults initialize
new sessions; revision-checked commands change existing configuration. A turn
captures its model or team, configuration, permissions and referenced revisions.
Editing a message updates the visible history and admits its replacement turn in
the same session atomically. Original records remain stored. Explicit forks and
rewind backups copy history into a separate session.
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

Admission freezes the worktree, permission, configuration, model or team,
reasoning effort, credentials and package revisions. Updates affect subsequent
admission. Policy, instructions, guides and selected capabilities form the stable
prompt; changing per-turn state is carried as agent context. Automatic compaction
uses the captured model and defaults to 80% of its context window. Summaries do not
delete original records or raw tool output. Context-length failures allow up to two
reactive compactions.

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
wait hints are minimums, never shortened to fit the budget. Ordinary streaming
requests stop automatic retry after emitting an event; summary requests buffer
their output. Cancellation interrupts waits. Permanent failures terminate the
request, and completed tools are not replayed.

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

Agent file operations exclude managed-checkout descendants and protected Git
metadata. Authenticated manual workflows can access registered managed worktrees
through the scoped resource services. User-created terminals use their login-shell
environment. Configured HTTP proxies are trusted to route public hostnames;
failure does not silently bypass the proxy. Local model downloads require an
installation action, and an unavailable engine reports that state.

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

Plugin assistants mount the shared conversation UI with an immutable package,
assistant and resource binding. Mounting alone creates no session; the first send
uses normal admission. They share model selection, approvals, questions and
history, while the package owns its assistant declaration and selected tools.
These sessions stay in their assistant's history rather than ordinary conversation
navigation; their transcripts remain available to explicit session links.

Model plugins handle their configured providers' messages and credentials under
the installed trust decision. Changed OAuth identity, scopes or flow require new
authorization. Declared programs start without a shell, with a scrubbed environment,
bounded execution and idle cleanup. Service connections inject credentials only
into declared origins, headers, MCP servers or environment targets.

The Node-owned MCP pool keys connections by package digest, settings revision,
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

Terminal rendering uses MyGo's terminal package and libghostty-vt. Mermaid uses
CrychicUI, chart variants use the conversation painter over limu, and math uses
Untie. These render natively and keep source text on failure. File preview draws
text, code, Markdown, notebooks and images; unsupported formats open in the system
application. Session HTML uses a sandboxed MyGo WebView with no privileged bridge.

Voice capture, speech recognition, voice reading and global selection shortcuts
are controller-local. `--preview` opens no profile and starts no Node. Explicit
acceptance and replay run in isolated processes; their limits and commands are in
[CONTRIBUTING.md](CONTRIBUTING.md#manual-acceptance).

## Pairing and operations

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
Linux Host into WSL2. Host's `submit`, `wait` and `share` management commands use
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
