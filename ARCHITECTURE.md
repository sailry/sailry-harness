# Architecture

## Purpose and scope

Sailry separates execution, transport, shared controller state and presentation.
Desktop and Host use the same Node implementation. Controllers do not run a
second agent engine or write another authoritative session history. This document
describes repository boundaries and runtime contracts, not a delivery plan or a
claim of platform acceptance.

## System context

Users control AI conversations and development resources through Desktop or
Mobile. An execution Node owns the work and may run inside Desktop or as Host.
External systems include model providers, SSH hosts, databases, Git repositories,
MCP servers and optional pairing/data-relay services.

```text
Desktop / GPUI Kit ── Client ──┬── in-process ── local Node
                              └── Link / iroh ── remote Node

Mobile / Flutter ── Rust bridge ── Client ── Link / iroh ── Node

Host service ── Node bootstrap + Link endpoint
Node ── ADK + scoped tools ── providers / files / Git / PTY / SSH / databases
```

Mobile connects to the selected Node directly; Desktop is not a required gateway.
Host is a long-running headless service, not an interactive agent CLI. Disconnecting
a controller does not stop already admitted work.

## Components and dependencies

| Component | Responsibility | Excludes |
| --- | --- | --- |
| `crates/protocol` | IDs, commands, events, snapshots, faults and capabilities | Runtime, UI and concrete transport |
| `crates/link` | Identity, pairing, authenticated transport, receipt and reconnect | Agent history and business execution |
| `crates/client` | Typed commands, ordered projections, deduplication and snapshot recovery | UI widgets, Node execution and a business database |
| `crates/node-runtime` | Admission, persistence, configuration, ADK execution and resource services | GPUI, Flutter and controller presentation |
| `crates/mobile-bridge` | Client handles, subscriptions and Rust/Dart adaptation | Node, ADK, PTY, Git execution and tool drivers |
| `crates/speech` | Shared speech capability and native audio integration | Conversation ownership |
| `crates/embedding` | CPU model loading, token offsets and normalized local vectors | Source grants, indexing and controllers |
| `crates/ocr` | Shared CPU image recognition and PaddleOCR decoding | Model downloads, source grants and controllers |
| `crates/model-assets` | Verified, cancellable model downloads shared by speech and retrieval | Model selection and configuration |
| `crates/content` | Reusable GPUI Kit editors, Markdown, diffs and selection | Node, Client, file acquisition and application state |
| `apps/desktop` | GPUI Kit composition, focus, drafts and native integrations | Duplicate history or request recovery engines |
| `apps/host` | Service lifecycle and noninteractive control through Client | Desktop dependencies and a prompt loop |
| `apps/mobile` | Flutter controller presentation | Independent business rules or credential storage |
| `services/pairing-relay` | Expiring short-code ticket exchange | Business commands or agent execution |

Production dependency direction is acyclic:

```text
desktop       → client + node-runtime + content + GPUI Kit
content       → protocol + GPUI Kit
host          → client + node-runtime
mobile-bridge → client
client        → protocol + link
node-runtime  → protocol + link + ADK
link          → protocol + transport dependencies
```

Test-only integration fixtures may construct real Nodes without changing the
production controller boundary. The root Cargo workspace owns pinned Rust
dependencies and lockfiles. Focused patches live in `vendor/` with upstream
licenses and patch notices. Feature modules stay inside their owning crate unless
a concrete platform or dependency boundary requires another crate.

## Data and ownership

One profile has one running Node owner, one Link identity and one endpoint.
Desktop's local Client reuses that owner. Desktop and Host share bootstrap,
recovery and shutdown. Controller identity, pairing records and rebuildable caches
are not another Node business database.

Resources retain Node → project → worktree → session/terminal ownership even when
the worktree layer is hidden in navigation. File and Git actions are confined to
the captured execution worktree. SSH, database and plugin resources also retain
their captured Node and resource identity.

The Node owns configuration and immutable sent-turn revisions. Controller
defaults initialize new sessions; an existing session requires an explicit
revision-checked command. Resuming elsewhere preserves its effective configuration.
Configuration and credentials are stored as plaintext in the execution Node's
configuration database, never public events, logs or UI caches. Projects remain
at registered paths; profiles contain Node storage, plugin packages/data and
uploaded attachments.

Manual profile backups use the execution Node's consistent SQLite snapshot,
referenced plugin packages, attachments and session HTML documents, and registered managed workspaces.
External project paths remain references. WebDAV and S3 exports publish verified
password-encrypted age segments before their manifest; uncertain operations keep
their original request identity. Explicit restore validates the archive into a new empty profile,
relocates only current profile-owned paths and preserves immutable history.
Temporary files, model caches and derived indexes are excluded, and restored
profiles do not start automatically. Export requires a user passphrase; keys and
Node identity remain inside the authenticated encrypted archive. The passphrase
stays in the execution Node's configuration and never appears in public snapshots.
New passphrases require at least eight characters; Desktop confirms a replacement
before autosaving it. Existing passphrases remain usable to recover older archives.
Restore accepts a separate passphrase for older backups and authenticates the
entire stream before publishing any profile. A restored profile retains its Node
identity; only one copy of that identity should run at a time.

Knowledge collections, settings and explicit plugin grants remain in the primary
Node database. `storage/knowledge.db` contains only derived chunks, full-text
indexes and quantized vectors; removing that cache never removes registered
sources or configuration. Plugins query their captured private storage or granted
collections through the same Client boundary. Source revisions, tags, paths and
access grants constrain candidates before ranking. Citations resolve through the
execution Node and recheck their source identity before returning original text.
Uninstalled models keep keyword retrieval available. Code discovery continues to
use filename matching, text search and language-server symbols.
Knowledge retrieval is a Node service for plugin-owned features; no standalone
knowledge plugin is bundled. Desktop settings configure vector models. Plugins
such as notes and reminders organize their own collections and expose their
settings, feature switches and conversation tools through shared plugin APIs.

One lazy CPU model owner per Node serves small indexing batches and prioritizes
interactive query work. Optional model assets require an explicit install action
and immutable revision, size and SHA-256 verification. Source watchers request
incremental updates; pausing retains existing keyword results, and explicit
rebuilds invalidate only the selected collection's derived data.

Model discovery reads public Hugging Face or ModelScope catalogs through the
selected Node. Each registered embedding definition binds its supported algorithm
and asset hashes to an immutable model identity; branch labels never substitute
for pinned file revisions. Desktop downloads and selects compatible speech models
locally, and recording retains its selected model until transcription finishes.
Both consumers use the packaged ONNX Runtime image; SenseVoice retains Sherpa's
recognition algorithm. Model updates within supported formats do not require an
application release, while new algorithms require implementation and validation.
The shared model installer resumes verified partial downloads when the source
validator is unchanged, and verifies the complete asset before publishing it.

Desktop and Host bundle Python 3.12.13 in their installation directory. The Node
lazily verifies that runtime and prepares profile-owned plugin environments from
captured, hash-locked requirements using prebuilt wheels. Immutable packages share
environments by runtime and requirements identity. Skills and MCP receive the
owning interpreter; task scripts and additional environments use the private
session directory through ordinary command permissions. The Node's PyPI setting
is independent from model download sources. Python is optional for ordinary
commands and skills without requirements. Dependency preparation runs outside
the shared file mutation queue, with one installer per environment; ready
environments bypass installation. Clients observe transient preparation stages
through the same local and remote subscription boundary.
Linux validation retains at most 2,048 directory subscriptions and 16 native
watchers across the process, evicting the least recently used environments.
Unwatched or oversized environments use full integrity validation on each access;
watched environments also revalidate periodically.

OCR is an optional Node-owned CPU service using the packaged ONNX Runtime.
Explicitly installed PaddleOCR definitions retain their selected profile and
immutable asset revisions, sizes and hashes. Image and scanned-PDF inspection,
including knowledge extraction, uses this same cancellable service through the
local or remote Client boundary. Model settings and installation state remain on
the execution Node.

Disposable command, browser and transfer staging lives under the configurable
profile's `tmp` directory, separate from durable worktree outputs and attachments.
Command scratch is scoped to the admitted turn and retained while its commands
still use it. Remote files opened by a controller use that controller's local
profile for their retained copies, not the execution Node's temporary directory.

ADK-Rust is the agent execution engine. Node integrates Runner and SessionService
with one authoritative durable event history. Client and UI projections are
rebuildable derivatives, not additional history writers or runners. Usage reports
cover Sailry's own canonical events; Client aggregates reports across Nodes while
keeping source identity and completeness explicit. Provider settings may separately
display quota observations returned by Sailry's own authenticated model requests;
missing or expired observations remain unknown. These observations are not session
usage statistics. External tool histories, separate account-quota queries and
third-party billing are outside that boundary.

ChatGPT OAuth uses device-code authorization and the account-scoped Codex backend
catalog and Responses endpoint. ChatGPT SIWC uses the official browser flow,
issued client registration and public OpenAI catalog and Responses endpoint.
Their credentials and request adapters are distinct; neither silently changes
authentication contracts or substitutes an API key. Model capabilities resolve
automatically from native and cached metadata; unknown capabilities use the
configured API contract.
Media operations try the admitted model and credential first. Only a definite
unsupported capability may use an explicitly configured, session-captured fallback;
authentication, quota, timeout and uncertain outcomes never trigger fallback.

## Runtime behavior

### Command admission and recovery

1. Client submits a typed command with a stable request ID and captured resources.
2. Node validates scope, revisions and permissions, then durably admits a durable
   command before Link acknowledges safe receipt.
3. Node executes the operation and records its business outcome.
4. Client observes the result and ordered events; reconnect uses the original
   request ID and snapshot/subscription recovery.

Local access uses an in-process adapter, not forced loopback networking, with the
same command semantics as remote access. Link owns transport ordering and receipt;
Client owns reduction, deduplication and snapshot recovery; Node owns admission
and outcomes. Transport receipt is not business success. Uncertain effects remain
uncertain and are not replayed automatically with a new identity. Conflicts require
fresh inspection, not silent overwrites.

### Session HTML documents

The Node stores validated HTML fragments as immutable revisions under its profile,
with their owning session, turn and captured plugin reference. Each revision freezes
the package's declared template, styles and scripts; later package updates do not
change saved documents. Revision-checked writes are available only to captured
native tools in running main sessions. Plans permit this contained document write
without granting project writes or arbitrary plugin callbacks.

Forks and rewind backups preserve canonical document receipts with their original
write owner. Reads captured to another live conversation expose only revisions
whose exact receipts remain in its visible history; inherited previews start at
the published revision and cannot submit forms. Rewinding the owner changes
conversation history without rolling back its document inventory. Removing an
owner retains referenced documents until the last live conversation reference is
removed; backups preserve those retained files and their immutable package assets.

Client projects document receipts into persistent conversation cards. Controllers
read content through the same authenticated local or remote commands and reuse
the conversation subscription for coalesced document invalidation. The package
owns its preview layout and controls; Desktop and Mobile provide native offline
surfaces with semantic theme tokens and pinned chart, diagram and math libraries.
Historical revisions are read-only. Editing a form pins its revision so publication
cannot replace a draft. Form answers enter the normal durable message queue with
a stable request ID and never approve a plan. Exports capture rendered content and
answers, remove scripts and interactive forms, and include Sailry branding.

### Agent turns and tools

Node freezes configuration, scope and package revisions at turn admission. Tools
use the same approval, cancellation and durable outcome boundaries. Work mode
and permissions are distinct; read-only planning does not authorize side effects.
Delegated work uses the same ADK engine and history path with an explicit
parent/child association. Closing a view stops observation, not admitted work;
stopping a turn is an explicit command.

Partial text and reasoning use the Node's bounded, ordered in-memory conversation
feed without entering the storage worker queue. Publication checks active turn
ownership and cancellation. Canonical messages and tool results still commit
through the authoritative ADK history path before publishing durable changes;
reconnect combines that history with any current drafts from the same feed.
When a run stops or fails, remaining visible text and reasoning are sealed and
written as interrupted canonical output before its terminal status is published.
Completed entries are not duplicated, and incomplete tool calls are not retained.

On macOS and Linux, Agent commands use a workspace sandbox by default. It
permits workspace writes and read-only access to installed toolchains, with
private temporary files and no network access. Full permission authorizes
commands requiring the complete environment automatically; Ask and Project
retain explicit approval for that access. The Node's conversation setting can
disable command sandboxing for subsequent calls in existing sessions without
changing captured access for pending or running commands. User-created
terminals start with the complete login-shell environment. Language services
and configured hooks retain their own execution policies.
The language-service plugin's enable switch controls the Node's service setting
atomically. Disabling it cancels pending starts and queries, stops active servers
before command completion and omits language tools from subsequent turns.
Installation remains available while disabled. Enabled services start on demand
and stop after inactivity; installation alone does not launch a server.
Controllers may stop one exact running server without changing its settings or
startup grant; later use can start it on demand again.
Language service settings are Node-wide: catalog, installation and server overrides do not
require a selected project. Disabling a server stops its instances across worktrees.
Execution remains worktree-scoped, with approved on-demand startup and idle shutdown.
The language plugin exposes package discovery, installation and removal to conversations.
Lifecycle tools reuse exact Node registry descriptors, conversation approvals and durable
resource admission; Plan turns cannot install or remove software. Removing a package stops
its instances across worktrees.
Native controllers configure ordered hook scripts per worktree. Admitted turns
capture each script's order and enabled state; a phase has one durable launch
receipt, and recovery never replays that group after a crash or cancellation.

Workspace commands cannot write captured Git metadata, including linked repository
metadata and common directories. Git writes use complete access. macOS also denies
creating new `.git` paths. Linux protects existing metadata through read-only mounts;
new metadata paths and replacement of symbolic Git pointers remain outside that
mount-based protection.

File discovery and content searches run inside Node with confined paths,
ignore rules and bounded result pages, without requiring a system ripgrep.
Multi-edit operations check the original file revision and publish one complete
result. Background commands may use a PTY and accept durably admitted input;
completion, stop and replay retain the original command identity.

Public web reads and search run on the execution Node. Direct web reads validate
and pin public DNS addresses on every redirect. Web reads alone explicitly use
configured environment or system HTTP(S) proxies, honoring proxy exclusions.
A selected proxy receives the original hostname and resolves it remotely; local
DNS is checked when available, ignoring proxy fake-IP answers. This supports
TUN proxies and avoids relying on polluted local DNS, but cannot enforce the
same destination-IP confinement as direct reads: the user-selected proxy is
trusted to route public hostnames. Literal private URLs and private local DNS
answers remain rejected, and each redirect is rechecked. Proxy failures do not
fall back to direct access. Other HTTP clients retain their existing proxy
configuration. Reads retain bounded paged snapshots and supply untrusted content
without browser authentication. Search settings
and credentials belong to that Node. A turn uses provider-native search when
available and preferred, otherwise one configured search tool; transient
failures can try each configured driver once and respect rate-limit cooldowns.
Search keys and manually supplied GitHub credentials are stored as plaintext in
the Node configuration database. Connecting an existing `gh` login stores its
account identity, then reads the current token transiently for MCP requests;
logout, account changes and token rotation invalidate cached connections. Public
GitHub reads do not require a GitHub MCP credential.

The delegation plugin manages role settings; Node owns and freezes the configured
roster. Delegation selects a matching configured role. An omitted or null role
uses the parent's effective model and assistant instructions; an unknown explicit
role is rejected.

Each delegated turn retains its parent tool-call identity; the child session keeps
its founding ancestry. The same parent session may reuse an idle child without
losing history or changing its role, effective configuration or worktree. Idle
reuse captures current packages and settings; initial delegation inherits the
parent's frozen capture, and guidance retains the active recipient's capture.
Delegation waits synchronously by default. Opt-in background children remain
owned by their main session across guidance handoffs; the main agent can guide
and wait for them through Node-owned tools. Main-session completion waits for
owned children, while explicit Stop cancels them. Child views are read-only;
their approvals retain the child identity and are presented in the main session.

Guidance is durably admitted before execution. While the recipient awaits or
streams a model response, admission interrupts that response and retains its
visible output; once the response's tools start, guidance waits until the whole
tool batch settles, before the next model request. A new ADK invocation continues
the same session history. If a
queued input has a different capture, explicit guidance admits a replacement
using the recipient's capture without modifying the original sent-turn record.
A controller may also guide the running main-session task directly; that input
waits behind any guidance already selected for the same boundary.
Natural completion advances the queue automatically. Explicit
Stop and restart preserve unconsumed input, references and attachments in a paused
queue without automatic replay.

Main-session planning completion requires review even when the model returns a
plain answer instead of requesting review itself. Node records this question in
the same durable history. Executing the plan atomically admits one coding turn
and changes the session mode while preserving its permission. Continuing planning
keeps Plan mode and does not itself admit another turn; subsequent user input or
an already admitted queue supplies further work.
Delegated planning answers do not create a separate user review.

### Pairing and reconnect

The pairing service exchanges a six-digit, 60-second code for an opaque invitation
ticket. Link authenticates the invited endpoint and persists trust. The service
can see tickets and is a short-lived trust boundary, not a business relay or
account system. Already paired peers reconnect without another short code.

An iroh data relay separately carries encrypted traffic when a direct connection
is unavailable. Pairing-service and data-relay URLs are different configuration.
Cloudflare and EdgeOne adapters share ticket-exchange logic, with native storage
adapters. See the [service contract](services/pairing-relay/README.md).

## Extension and presentation boundaries

Official theme source lives in `sailry-themes`, pinned by the `themes/` Git
submodule. Its generated `catalog.json` records manifest metadata and resource
digests for discovery. Official theme package versions stay `0.1.0` before the
first release, with numeric manifest and index formats at `1`. Desktop owns the
reusable local theme loader, importer and renderer; repository theme sources are
not embedded or installed automatically. Updating the source Gitlink does not
change a user's selected or installed theme.

The application lives in `sailry-harness`; official plugin source lives only in
`sailry-plugins`. The `plugins/` Git submodule pins a reviewed plugin commit.
Builds embed that snapshot as ordinary package assets for offline use; new Node
profiles install the selected defaults once, without resurrecting removed packages.
Startup refreshes already installed bundled packages when their shipped versions
increase, preserving their enable choices and settings.
Removed packages and packages from other sources are not changed by this refresh.
The online official marketplace reads the plugin repository's `catalog.json`;
unavailable networks leave the bundled snapshot installable. Invalid indexes are
reported, not treated as an offline result. Official installation and updates
capture immutable repository commits through Node's shared acquisition path.
Preinstalled packages can update from the official repository without an app
upgrade; update failures preserve installed packages and are reported honestly.

Packages use the Agent Plugins manifest and Sailry's v1 extension contract.
Node owns validation, immutable resources, grants, private KV, callbacks,
tools and dispatch. Updates affect new admission; admitted work retains its
captured package and settings revision.
All packages own their pages, panels, dialogs, view state, workflows and model
prompts. Search and language-service settings are packaged Kit scripts using the
same captured Client commands for local and remote Nodes. Schema-only plugins
use the shared configuration form. Core never selects a feature page by package
name or a package-specific native renderer ID. It owns reusable execution,
persistence, transport and protected credential services; native UI adapters
cover documented Kit or platform gaps, without plugin business workflows.
Controller settings actions require a declared settings surface and capability;
Agent calls retain their existing tool approvals. An optional `enabled_service`
binds one installed package's switch to a Node service. The binding is exclusive,
grants no capabilities, and keeps service shutdown atomic with enablement changes.
Knowledge retrieval has no standalone package or conversation entry; feature
plugins use its shared service.

Headless JavaScript callbacks use a bounded Node-owned QuickJS runtime. Desktop
entries use GPUI Kit script bindings and host-owned SDKs. Desktop and headless
exports are distinct; see the [SDK contract](sdk/plugins.md). Sailry's SDK does not
grant raw OS access. Desktop UI code is trusted code, not an OS sandbox.

GPUI Kit owns standard controls, semantic themes, focus, keyboard behavior and
overlays. Desktop owns product composition and small native adapters. Ghostty's
terminal renderer is a custom rendering boundary; surrounding controls still use
Kit. Flutter consumes shared Client state rather than another conversation state
machine. UI preview data is isolated from execution and user profiles.

Desktop Markdown charts use native Kit components; Mobile uses Flutter charts
and a native Sankey painter. Mermaid diagrams and formulas share pinned local
assets in a serialized controller-owned hidden WebView. Desktop renders validated
SVG as GPUI images; Mobile renders formula SVG and bounded Mermaid PNG images to
preserve diagram CSS and markers. Message rows contain no embedded WebViews.
Prepared visuals follow source and theme changes; unsupported platforms and
rendering failures retain the original selectable text. The Desktop native adapter
supports macOS and Windows; Linux retains that source fallback.

Browser element annotations retain the selected page, tab and controller view.
The browser package owns annotation review, note drafts and submission feedback.
Scoped native services capture the page and image, validate the selection token,
and transfer approved text and the protected viewport image to the captured
composer through its existing Client attachment upload. Navigation or view
changes cancel selection.
System voice reading and global selection shortcuts remain controller-local.
Selection insertion rechecks the captured Node, worktree and conversation before
changing a draft; neither reading nor selection starts an Agent turn.

## Deployment and operations

Desktop bundles presentation and a local Node. Host deploys the same Node as a
headless process, using a private profile and explicit network/pairing options.
Only one process may own that profile. Pairing is opt-in for Host; established
peer trust survives restart. Host release archives are independent from Desktop;
the official installer verifies versioned assets and starts systemd or launchd.
Desktop's Add Host command invokes that installer through Node-owned SSH, then
pairs through a private bootstrap ticket. It never uploads a bundled Host or
overwrites an existing profile. Desktop still includes its own local Node.
Office inspection uses Rust. Desktop previews original Office files and PDFs
offline in its sidebar WebView; it does not require conversion or provide a native
PDF export command. Document skills use the execution Node's bundled Python and
prepared plugin environments through ordinary command permissions.
Model execution uses ADK's native OpenAI, Anthropic, Gemini, DeepSeek and Azure
adapters or shared compatible endpoints. Sailry does not enable the Bedrock or
Vertex SDKs or provide their IAM/ADC credential flows. Supported compatible
endpoints can use the ordinary API-key configuration. HTTP clients use the
shared ring TLS provider.

The `sailry` management command controls the installed service. Sharing uses a
profile-private Unix socket to request an invitation from the running Host; the
management process does not open another Node or persistence owner. The socket
uses a v1 control envelope and owner-only access. Disconnecting the sharing
command cancels that invitation without revoking already established peer trust.
The `submit` and `wait` subcommands use that socket to reach the same Node through
Client commands and conversation subscriptions. New tasks use atomic session and
first-turn admission; existing sessions use revision-checked turn admission.
Request IDs retain the Node's durable replay identity. Management output reports
canonical turn status and pending approvals or questions as JSON. Disconnect,
timeout and observation cancellation leave admitted execution running, and task
management never grants approval or creates another Node or history writer.
The `acp` management subcommand adapts the official ACP SDK's stdio protocol to
that socket's typed Request/Topic forwarding and an ordinary shared Client.
It reuses the Node's sessions, immutable turn captures, canonical history and
approval decisions; ADK's separate ACP server and Runner are not enabled.
Editor lifecycle and presentation state are local to this adapter. Closing
stdio or cancelling an RPC drops observation; explicit ACP session cancellation
or session closure sends the Node's StopTurn command. Workspace registration, MCP installation and
credential configuration remain explicit Node operations outside this adapter.
The lower-level macOS launchd helper still only generates a service definition.
Service lifecycle and local verification are also documented in the
[pairing guide](services/pairing-relay/README.md#run-and-verify).

Shutdown cancels invitations and releases shared runtime resources through Node's
lifecycle. A disconnected client is not a shutdown signal. Restore reads durable
facts; it does not silently rerun interrupted side effects. Protected profiles,
credentials and local logs are not distributable source.

Before the first product release, Sailry protocol, wire, storage and extension
contracts remain v1 and change in place. Startup adds missing schema objects and
fills missing storage fields with their defaults. Updates remain centralized and
simple, with no historical state inference or versioned migration framework. Each
update is validated on an isolated snapshot, backed up privately and applied with
current-format validation in one transaction. The backup retains original records;
startup does not reset or delete user data. Third-party dependency versions, ordinary
data revisions and event sequences are independent. Release compatibility remains
a separate decision.

Plugin configuration follows its declared settings surface. Plugins with settings
pages open their existing panels from configuration actions and
settings navigation. Other configurable plugins use the shared schema form dialog,
with the Node and package captured when opened. MCP authorization remains in details.
Plugins without configuration fields or a settings surface have no configuration action.
Scripted settings pages preload up to four validated resource bundles per captured
Node, warming on settings entry and navigation hover. Preloading never executes
scripts or reads configuration values. Returning to a page reuses those files but
creates a fresh view and reads current data; leaving releases the view and its
runtime. Leaving the settings area releases all prepared resources. Recent resource
entries are bounded and invalidated by package/settings revisions, disconnects and
Node changes. Workspace plugins declare `navigation_options.retention`: `none`
releases on navigation, `always` retains until window close, and `idle` expires
after a declared number of seconds away. The host owns these lifetimes, cancels
expiry on return, and invalidates stale packages regardless of retention. Omitting the
policy defaults to `none`; bundled games, editors and connection views explicitly
use `always` to preserve their interactive state.

Conversation export reads canonical history through Client. Per-reply actions select
one turn's user message and final reply; session exports include all user messages
and final replies.
Tool activity, reasoning and diagnostics are excluded. Markdown retains source syntax;
HTML bundles offline formula, diagram and chart rendering. Desktop's existing native
WebView boundary renders PNG in bounded tiles on macOS and Windows. Export captures
Node/session identity, supports cancellation and preserves changed destination files.
HTML and images include Sailry branding and the static website QR code.

## Security and quality constraints

Authenticated Node/resource access, path confinement, Node-owned credentials,
revision checks and honest uncertain outcomes are the primary boundaries.
Destructive actions identify their target and consequence. Imported sources and
resources retain their licenses; root Apache licensing does not replace those
notices. Native OS permissions remain platform responsibilities.

Changed shared behavior requires local and remote-path evidence. UI interaction,
native OS acceptance, real-service integration and performance are separate
results. Isolated fixtures never mutate user profiles. Build success is not device
or whole-product acceptance; local pairing fixtures and bundling do not prove
cloud deployment. Test conventions are in [CONTRIBUTING.md](CONTRIBUTING.md).
