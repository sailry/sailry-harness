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
| `crates/decision` | Validated GGUF files and pinned llama.cpp CPU decision inference | Downloads, model selection and Agent orchestration |
| `crates/model-assets` | Verified, cancellable model downloads and directory receipts | Model selection and configuration |
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
dependencies and lockfiles. ADK changes are committed in the pinned fork; all ADK
crates use the same revision. Retained third-party source keeps its upstream
licenses and provenance. Feature modules stay inside their owning crate unless
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

The Node uses the native Turso Rust driver for `storage/node.turso` and
`storage/knowledge.turso`. Each database has one shared native handle; an operation
leases its own connection and keeps related SQL in one transaction. Every
connection explicitly uses full synchronous durability, and both databases use
MVCC. Durable sequence counters advance in the same transaction as their records,
so deleting records does not reuse their identifiers. Existing SQLite files are
left untouched; startup does not import them or fall back to another engine.
External database tools retain their separate SQLx drivers.

Channel presets share one Node-owned directory: bundled declarations, the last
validated partner catalog and enabled package declarations. Listing the directory
does not require a public-network read; refreshing partner recommendations is an
explicit read. Saved channels retain a stable source, preset and route identity,
with user endpoint and credential overrides kept separately. Admission resolves
that identity against the current directory and captures its request policies and
exact package resources. An admitted turn does not consult a newer catalog.
Role providers are captured on each parent turn as well; delegated work inherits
that turn's provider declarations rather than resolving the current package.
Historical turn-role defaults come from their own immutable session revision.

Service connections are package-declared resources owned by the execution Node.
One connection record owns its public fields, private credentials, authentication
method, consent snapshot and revision. Generic OAuth, token exchange, declarative
application registration, MCP OAuth and fixed local CLI sources share this owner;
there is no vendor dispatch. Registration uses the existing private flow runner.
Only declared origins, headers, MCP servers and environment targets receive
credentials. OAuth permission or actual MCP endpoint changes suspend use until
revision-checked consent. Content-only updates from the same publishing owner
preserve authorization. Public editing revisions advance on every save; sent
turns capture a private authorization revision that changes when configured
credentials, authentication targets, consent or enablement change. Unchanged
authorization preserves captured and new calls, while live revocation and consent
remain enforced. Private historical settings captures stay intact; current
authentication reads only connections.
The public disconnect capability is derived from the current private credentials
or CLI binding and login state; it does not expose credentials or add stored state.

Local CLI connections bind a public account identity and read transient tokens
when used. Fixed argv runs without a shell in a minimal environment with bounded
execution and output. Tokens remain in Node memory and protected injection;
account changes or sign-out stop use. Connectors settings and inline cards share
the existing controller authorization presentation and local/remote protocol.
Application registration does not establish publication, document access or MCP
entitlement.

Beginning an ordinary task, including delegated work, freezes its user-facing
input on the existing run record. Conversation projections include that input
only until canonical user history exists, with attachment metadata but no bytes,
role instructions or execution configuration.
Activity summaries remain metadata-only. Logical history pages preserve full
inputs using turn cursors rather than truncation or synthetic history events.

Core Agent tools discover and read conversations on their execution Node without
requiring an explicit mention. Directory, history, search and explicit conversation
references share project visibility: project histories span worktrees, while
unassigned conversations can read other unassigned histories on the same Node.
Each call rechecks its admitted invocation and target; history access grants no
file, command or worktree-switch authority. Model-facing reads use public history
projections with bounded output, revision checks, continuations and explicit content
truncation. Plugin assistants retain their declared tool selection.

New managed project checkouts live under the main worktree's
`.sailry/worktrees/<id>`; existing profile-owned checkouts retain their registered
paths. Git excludes `.sailry/` through `info/exclude`, preserving user ignore files.
Removing a clean linked worktree without active or queued turns keeps the history
of sessions that used it; they cannot admit turns there unless a checkout is
registered again at that path or the conversation moves to another worktree.

Data management backups include both native databases, referenced plugin packages,
attachments and session HTML documents, and registered managed workspaces.
Storage maintenance pauses new transactions and drains active connections. It reads
the backup configuration and resource references in one transaction, checkpoints
and closes both database handles, copies their complete native file families, and
reopens them before copying resources. The native MVCC log replaces the database
extension with `.db-log`; WAL and optional coordination files append `-wal` and
`-tshm`. Restoring verifies the complete file families before publishing a profile.
Referenced session documents remain on disk until copying finishes; session
metadata can still be removed while their physical cleanup waits.
External project paths remain references. Local folders, WebDAV and S3 publish verified
password-encrypted age segments before their manifest; uncertain operations keep
their original request identity. Explicit restore validates the archive into a new empty profile,
relocates only current profile-owned paths and preserves immutable history.
Temporary files and model caches are excluded, and restored
profiles do not start automatically. Export requires a user passphrase; keys and
Node identity remain inside the authenticated encrypted archive. The passphrase
stays in the execution Node's configuration and never appears in public snapshots.
New passphrases require at least eight characters; Desktop confirms a replacement
before autosaving it. Existing passphrases remain usable to recover older archives.
Restore accepts a separate passphrase for older backups and authenticates the
entire stream before publishing any profile. A restored profile retains its Node
identity; only one copy of that identity should run at a time.
Controller-selected exports and file restores use caller-bound, verified Client
transfer streams, so a remote Node's export is saved on the controller device.
Local and online targets can be enabled together and share one capture and encrypted
archive. Automatic backups use the existing Node dispatch service and one private
backup schedule, configured through the same revision-checked settings command.
Only Node configuration can create this native callback; plugin job authority
cannot borrow it. Scheduling requires an online Node and reuses the existing latest
due occurrence recovery; no controller window or additional background service is required.

Knowledge collections, settings and explicit plugin grants remain in the primary
Node database. `storage/knowledge.turso` contains only derived chunks, native full-text
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

Source watchers request incremental updates; pausing retains existing keyword
results, and explicit rebuilds invalidate only the selected collection's derived
data. An explicit installed collection model overrides the global embedding
selection, including when global vector retrieval is disabled.

One Node-owned local model registry manages OCR, embedding and decision definitions,
selection, downloads, verified receipts and installation state through the same
commands and events. Assets require explicit installation and immutable revision,
size and SHA-256 verification. Existing model IDs, directories and collection
references survive centralized, backed-up current-format updates; derived vector keys
and source scopes remain unchanged. The settings singleton retains its choices
even after all registered models are removed. Committed model configuration
is applied to that registry before command completion. Installation subscriptions
carry progress; catalog events carry definition and installation-state changes.
Knowledge retrieval reads model configuration from the registry. Embedding changes
invalidate derived vectors; OCR selection and installation changes refresh source
extraction. Installed state comes from the verified files and receipts; unchanged
file identities retain their verification across restarts. Earlier valid receipts
are recognized in place without downloading or reinstalling their models.

One CPU runtime lazily loads local model engines and runs one inference at a time.
Interactive retrieval and document inspection take priority over queued indexing
and extraction. The runtime releases idle engines after five minutes and evicts
the least recently used engines against a 2 GiB asset-size estimate. A larger
individual model may load alone. Cancellation reaches queued work and active
engine stages; removal invalidates captures and unloads the engine before deleting
owned assets. Desktop shares one model panel per Node while speech remains local
to its controller. Changing a kind's selection or enable choice cancels only that
kind's inference; its installations continue. Registering definitions and saving
unchanged choices preserve ongoing work. Mirror changes cancel only downloads
whose asset URLs change.

Local decision requests use typed binary, choice and score questions through the
same nondurable local and remote protocol. The `decisions.run` action permits
read-only tools and Node callbacks to call `sailry/decisions`. Requests are bounded
before inference; oversized tokenized prompts fail against the model context.
No decision model is selected, recommended or downloaded automatically. Plugins
own workflows that use these answers; ADK remains the Agent execution engine.

Decision engines use the packaged llama.cpp b11756 C API, checked GGUF metadata
and its `systemone` template. Laya and OpenJev follow that revision's decision
algorithms. One runtime owner performs loading, validation, inference and unloading;
CPU weights and request contexts share the scheduler with OCR and embedding.
Model metadata carries the selected Node's execution availability, so a remote
controller never infers support from its own operating system.

| Target | Decision runtime |
| --- | --- |
| macOS x64 and ARM64 | macOS 13.3 or later; the application minimum remains 13.0 |
| Linux x64 | glibc 2.34 or later |
| Linux ARM64 | glibc 2.38 or later; Host, OCR and embedding retain the 2.34 minimum |
| Windows x64 and ARM64 | Packaged official CPU runtime |

Native availability preflights the exact locked libraries and their dynamic
dependencies, retaining successful handles until restart and retrying failed
loads on the next request. Verified packaged files are cached independently, so a
failed dynamic load does not repeat file hashing. Cold checks run outside async
workers. Models and contexts are still created only by the shared inference
runtime. Installation checks every tensor; later loads reuse the verified assets
without repeating that full scan.

Unsupported decision targets retain OCR and embedding. Ordinary metadata remains
readable, while decision registration, installation and execution report
`Unavailable`; there is no alternate runtime or silent fallback.

Model discovery reads public Hugging Face or ModelScope catalogs through the
selected Node. Each registered definition binds its supported algorithm
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
The Files package can list OCR definitions and explicitly install or uninstall
their assets through the same model services and permission policy. Uninstalling
keeps definitions and settings; it does not select or enable another model.

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
missing or expired observations remain unknown. An installed, enabled model
plugin may also read its current provider's account status through declared
scoped requests using that provider's authorization and display it independently.
Declared account operations use the ordinary durable request ledger; only public
inputs are retained, and the Node injects authorization during execution.
Neither source contributes to session usage statistics. External tool histories
and third-party billing analysis remain outside that boundary.
Context observations pair provider-reported input and output tokens with the
admitted model's frozen context limit; a later model change does not relabel an
old observation. Pending-content estimates can trigger early compaction but are
not provider token measurements. Before another request, compaction can summarize
a completed historical prefix while preserving the current input and complete
tool exchanges. Summary requests use the effective summary output token limit;
input estimates guide prefix selection without rejecting a request. A failed
summary never permits replay.
Eligible prefixes use canonical turn ownership and complete call-ID-paired tool
batches, including prefixes within completed turns. Parallel results are matched
only within their batch; original history and provider-added parts are retained.
Automatic, explicit and model-requested compaction prefer complete prefixes that
fit the estimated summary envelope, with an eligible complete fallback when none
is estimated to fit. A complete nonempty text summary is accepted without a
length comparison against its source. A persisted summary invalidates the old
token observation; estimates can trigger later compaction until the next provider
measurement. Automatic summary failures are reported and retain the prior
context; estimated limits do not reject the ordinary model request.
Request checkpoints are changed only after persistence succeeds and are local
projections, not another history writer.

Account authorization workflows belong to model authentication packages.
The default `openai-auth` package retains ChatGPT OAuth's device-code flow and
account-scoped Codex backend, and ChatGPT SIWC's browser registration flow and
public OpenAI catalog and Responses endpoint. The default `claude-auth` package
retains Claude subscription OAuth's browser or pasted-code flow. Controllers use
the common login interaction; packages supply their endpoints, scopes, polling,
request composition and verified claim mappings. The execution Node runs those
pure steps through its existing private HTTP, PKCE callback, OIDC verification,
registration, cancellation and persistence services. It alone owns rotating-grant
fences and credential revision checks.
Explicit sign-in can replace a channel's account. A changed or unproven identity
receives a new credential reference; current ordinary sessions using the replaced
reference receive a new configuration revision, while admitted turns retain their
original authorization. Automatic renewal still requires the same account and
keeps its existing uncertainty fence. Optional public account emails come from the
authentication adapter and share one credential projection across channel types.
Stable authentication IDs and private credential codecs preserve existing accounts
and immutable sent-turn captures. Private account bindings retain the owning
package and flow across ordinary code updates; changed or ambiguous ownership
fails explicitly. Model catalogs, request adapters and account usage retain their
existing owners rather than being duplicated by authentication packages. Model
capabilities resolve automatically from native and cached metadata; unknown
capabilities use the configured API contract. Regional vendor endpoints and
metered/coding product choices remain explicit provider settings. Editing unrelated
fields does not rewrite an existing endpoint or infer a product from its key.
Media operations try the admitted model and credential first. Only a definite
unsupported capability may use an explicitly configured, session-captured fallback;
authentication, quota, timeout and uncertain outcomes never trigger fallback.

Node reuses native model clients by session and captured configuration. Each
generation refreshes the cache idle lifetime and freezes its authorization,
credential revision, response observer and redaction material through stream completion. Main API-key Responses and ChatGPT
OAuth generations prefer a session-owned WebSocket; helper and compaction clients
are constructed with pooled HTTP. Both transports share request adaptation and response projection.
Stateless Responses requests replay provider reasoning only with its original
opaque `encrypted_content` and the same provider, model and private authorization
ownership. A non-secret origin digest binds the effective endpoint and credential;
verified account identity survives refresh, while unverified identities are bounded
by their credential reference and revision. Unknown old records and foreign opaque
parts are omitted only from outgoing request copies; ordinary text and tool history,
persisted events and their original bytes remain unchanged. Models request encrypted
reasoning only when catalog metadata advertises reasoning or a positive effort is
selected, independently of summaries. Older reasoning without encrypted content
is omitted only from outgoing requests.
Only verified extensions of canonical history use connection-local continuation.
Pre-event WebSocket failures select the same adapted request over SSE once;
absence of an acknowledgement does not prove non-delivery, so this fallback may
generate and bill twice if the provider already accepted the first request;
capability observations stay in Node memory and definitive refusals are remembered
only after SSE succeeds. Destinations matched by proxy policy use HTTP; `NO_PROXY`
bypasses remain eligible for WebSocket.

Turn preparation overlaps a handshake-only preconnection; it sends no generation
or warmup frame. Entry ownership cancels pending handshakes; the SDK closes unused
preconnections within 25 seconds. Idle sockets close after four minutes and expire
after 55 minutes at a request boundary. A change to the final adapted headers
reconnects with the new capture. Constructor-bound API keys still participate in
client cache identity. Session archive/removal and Node shutdown release registry resources.
HTTP uses a 30-second connection deadline; WebSocket handshakes use 15 seconds.
Before the first provider event, WebSocket requests have a 15-second deadline
that control frames do not extend. Any provider event ends SSE fallback eligibility,
including an acceptance event before text arrives. HTTP raw-byte and subsequent
WebSocket frame reads have a 300-second idle deadline, including keepalives. Node
does not regenerate an accepted WebSocket request after a delivery failure.
Other uncommitted failures share one budget of five retries, including at most two
timeout retries and one HTTP recovery after partial output. Recovery buffers the replacement
privately until completion; committed responses and executed tools are never
replayed. A server-requested delay of at most 60 seconds is honored; a longer
delay is reported without an automatic retry. Exhaustion preserves canonical
history for the existing retry and continuation actions.

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

Node shares one bounded installation queue across resource types. Independent
resources can prepare concurrently; attempts for the same plugin, local model
kind or language package stay in admission order through durable completion. Caller-scoped transient snapshots expose real
phases and measured progress through the same local and remote Client subscription.
Confirmed success removes a task; failures and uncertain outcomes remain until
explicitly dismissed. Dismissal changes only the observation, not the command ledger
or active execution. Captured plugin authority stays private. Controllers own installer
visibility and locally planned batches, not another execution queue.
OCR, embedding and decision installation reuse this queue through their shared registry;
native validation uses the same CPU runtime as inference. Language installation
keeps its package driver; removal shares that lane without appearing as an installation.
Speech installation remains with the controller-local speech manager. Public installation
observations require their model or language capability and expose only explicitly
owned requests in their original captured plugin scope, never the controller feed.

### Session HTML documents

The Node stores bounded HTML as immutable revisions under its profile, with the
owning session, turn, source kind and captured plugin reference. The default
document kind sanitizes fragments and freezes the package's declared template,
styles and scripts; later package updates do not change saved documents.
Revision-checked writes are available only to captured
native tools in running main sessions. Plans permit this contained document write
without granting project writes or arbitrary plugin callbacks.

The preview kind preserves self-contained authored HTML, CSS and JavaScript.
Desktop renders it in the existing native preview adapter's opaque-origin
sandbox, not the privileged document surface. It receives no Node, tool,
credential, filesystem or conversation-form bridge. Parent and child CSP restrict
resource loading and navigation; this is not an OS-level zero-egress guarantee.
Preview HTML exports preserve that wrapper and original source, not transient
browser state. Preview PNG export is unavailable; document exports are unchanged.

Forks and rewind backups preserve canonical document receipts with their original
write owner. Reads captured to another live conversation expose only revisions
whose exact receipts remain in its visible history; inherited previews start at
the published revision and cannot submit forms. Rewinding the owner changes
conversation history without rolling back its document inventory. Removing an
owner retains referenced documents until the last live conversation reference is
removed; backups preserve those retained files and their immutable package assets.

Client projects document receipts into persistent conversation cards and the
conversation artifact inventory, including receipts from earlier unloaded turns.
Controllers
read content through the same authenticated local or remote commands and reuse
the conversation subscription for coalesced document invalidation. The package
owns its preview layout and controls; Desktop and Mobile provide native offline
surfaces with semantic theme tokens and pinned chart, diagram and math libraries.
Historical revisions are read-only. Editing a form pins its revision so publication
cannot replace a draft. Form answers enter the normal durable message queue with
a stable request ID and never approve a plan. Document exports capture rendered content and
answers, remove scripts and interactive forms, and include Sailry branding.

### Agent turns and tools

Conversation tool results default to an activity without successful-result
expansion. Captured declarations opt into meaningful details, standard content,
progress or hidden maintenance success. Finite output projections select bodies,
paths, tables and diffs; explicit JSON projections retain business data without
turning protocol metadata into ordinary detail rows. Tool groups remain
collapsible. Individual calls expand only for Client-projected details; canonical
raw results remain captured without a separate debug modal. Actual failed-call
facts share the existing turn observation and canonical history owner, preserving
error inspection without classifying arbitrary business fields as failures.
Model results and durable tool output remain unchanged.

Node freezes configuration, scope and package revisions at turn admission. Tools
use the same approval, cancellation and durable outcome boundaries. Work mode
and permissions are distinct; read-only planning does not authorize side effects.
Delegated work uses the same ADK engine and history path with an explicit
parent/child association. Closing a view stops observation, not admitted work;
stopping a turn is an explicit command.

Node owns each tool approval's five-minute deadline, with ADK explicitly delegating
that bound to its handler. Expiry requests a decision from the same storage actor that
commits approvals and session rules; it never independently denies a call during commit.
A closed or expired wait before the actor's commit claim returns a conflict and leaves no
rule or approved receipt. A commit claimed first settles before expiry is answered.
Timeout denial can continue the same turn; later turn cancellation can still stop an
approved tool before execution.

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
terminals start with the complete login-shell environment. Startup reserves a
terminal slot before launching outside the shared inventory lock; concurrent
opens of the same terminal share the existing result. Stop prevents late starts
from publishing. Closing a local terminal stops its verified PTY session's job
groups before its shell, or its existing Windows Job. If the Unix leader has
already been reaped or its identity cannot be verified, cleanup does not infer
ownership from a recycled session ID. Language services
and configured hooks retain their own execution policies.
The language-service plugin's enable switch controls the Node's service setting
atomically. Disabling it cancels pending starts and queries immediately and omits
language tools from subsequent turns. Server cleanup runs asynchronously outside
Store admission and metadata locks; startup and shutdown remain serialized, and
obsolete cleanup cannot replace current service policy or newly started servers.
Installation remains available while disabled. Enabled services start on demand
and stop after inactivity; installation alone does not launch a server.
Controllers may stop one exact running server without changing its settings or
startup grant; later use can start it on demand again.
Language service settings are Node-wide: catalog, installation and server overrides do not
require a selected project. Disabling a server stops its instances across worktrees.
Execution remains worktree-scoped, with approved on-demand startup and idle shutdown.
The language plugin exposes registry discovery, managed installation, actual worktree
instances and explicit start, stop and removal tools to conversations. Installation or
an available registry descriptor does not imply a running server.
Lifecycle tools reuse exact Node registry descriptors, conversation approvals and durable
resource admission; Plan turns cannot explicitly start, stop, install or remove software.
Removing a managed package stops only its idle owned instances across worktrees;
active installation or use returns Busy before shutdown or deletion. External executables,
project files and service configuration are not removed.
Native controllers configure ordered hook scripts per worktree. Admitted turns
capture each script's order and enabled state; a phase has one durable launch
receipt, and recovery never replays that group after a crash or cancellation.

Workspace commands cannot write captured Git metadata, including linked repository
metadata and common directories. Git writes use complete access. macOS also denies
creating new `.git` paths. Linux protects existing metadata through read-only mounts;
new metadata paths and replacement of symbolic Git pointers remain outside that
mount-based protection.
Repository workspace commands and Agent/headless file operations and discovery
protect their root's `.sailry/worktrees` subtree. Authenticated manual controller
file workflows can access its descendants through the same registered-root
services, preserving Git metadata, managed checkout root and Node-storage
ownership safeguards. Each child retains its own Agent checkout scope.

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
roster. Its conversation tools edit the public Node catalog through the same
revision-checked services as settings. Catalog edits do not change existing
session rosters. Explicit application selects exact catalog revisions for one
captured session and affects future admission only; running and queued turns and
existing children retain their captures. Tool results never expose frozen
provider configuration or credentials. Delegation selects a matching configured role. An omitted or null role
uses the parent's effective model and assistant instructions; an unknown explicit
role is rejected.

Each delegated turn retains its parent tool-call identity; the child session keeps
its founding ancestry. The same parent session may reuse an idle child without
losing history or changing its role, effective configuration or worktree. Idle
reuse captures current packages and settings; initial delegation inherits the
parent's frozen capture, and guidance retains the active recipient's capture.
Explicit guidance can also start a new task in an idle child through the same
durable reuse admission and supervisor, without replaying its completed work.
Delegation observes completion synchronously by default within a bounded tool-wait
window. If the admitted child is still queued or running, the tool returns its
actual session, turn and pending status; observation expiry never cancels or
replays the child. The existing session-owned supervisor retains execution and
automatic completion delivery. Explicit Stop and Node shutdown still cancel
running and queued children. Queue residence does not consume child model or
command execution deadlines.
Ordinary sessions without a project can delegate within their existing private
worktree; resource-bound assistants and nested delegation remain excluded.
The delegation package owns its child concurrency setting and settings page,
defaulting to eight. A public decimal string field marked
`x-sailry-child-concurrency` binds package configuration to the shared execution
service; only one installed package can own that policy. Node validates the
declaration and reads its current value when claiming work. Without a declared
owner, the service adds no product-specific concurrency limit. Configuration
changes affect queued work without rewriting its captured model or permissions.
Running children and FIFO order are scoped to their root parent session, so one
busy conversation does not queue children from another. Children exceeding that
budget remain durably queued with their admitted captures; they do not occupy
root workers.
Lowering the budget leaves existing executions running. Stop cancels queued
work, and restart interrupts owned execution waiters without replaying calls.
Opt-in background children remain
owned by their main session across guidance handoffs; the main agent can guide
and wait for them through Node-owned tools in later turns. A successful main turn
completes after its own execution, hooks and admitted operations settle, without
waiting for background children. Explicit Stop of an active main turn still
cancels its owned children; stopping a completed turn does not. Child views are read-only;
their approvals retain the child identity and are presented in the main session.

Background child and command completions have durable, session-owned evidence.
An idle main session continues through the ordinary queue using its current
configuration when background work completes, without an enable switch.
Busy sessions collect a bounded batch on their next eligible input. Pending
consent, paused queues and removed or archived owners prevent automatic input;
restart preserves evidence without restarting work or automatically continuing.
Reservations become delivered only when their marked input is appended to the
canonical ADK history. Completion input is identified as Node-supplied,
untrusted result data rather than user speech or new authorization. Explicit
model result collection consumes the exact source execution; controller polling
does not. Command summaries and bounded output tails survive restart, while
complete output remains available only while its process cache entry is retained.

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

Successful main-session planning completion ends execution and retains a pending
review in the same durable history, even after a plain answer; a new user input
closes that review without changing mode. Ordinary model questions still wait for
their answers. Explicitly executing the plan atomically admits one coding turn
and changes the session mode while preserving its permission. Continuing planning
keeps Plan mode and does not itself admit another turn; subsequent user input or
an already admitted queue supplies further work.
Delegated planning answers do not create a separate user review.

### Computer use

The `computer` package declares an ordinary stdio MCP server and discovers its
native tools on the execution Node. `${SAILRY_COMPUTER_USE}` resolves only to
Sailry's bundled, pinned Open Computer Use helper. macOS uses a separately signed
`Sailry Computer Use.app` inside Desktop's `Contents/Helpers` or Host's `helpers`
directory. Windows uses `helpers/open-computer-use.exe`, including its PowerShell
bridge. Both require a logged-in graphical session; Linux computer use is not
integrated. Desktop and Host remain usable when the helper is missing. The macOS
helper requires macOS 14, independently of the controller's minimum OS version.

MCP server options belong to the frozen v1 configuration. `exclusive` serializes
non-read-only calls by installed package name and server across sessions, after
ordinary approval. Unconfirmed calls retain that lock through pending-task
cancellation and connection cleanup. `keep_images` reduces only that server's
model-request image projection in batches of five; canonical tool results,
attachments and sent-turn captures remain unchanged. `reuse: false` closes that
server at turn settlement
instead of pooling it. Settlement confirms pending-task cancellation before
closing the wire. Stdio closure sends EOF and waits up to five seconds
for a successful child exit, then kills and reaps when needed. Failed or uncertain
cleanup returns `OutcomeUnknown`; input is never replayed automatically.

Each macOS helper socket connection owns its temporary window state, element
indexes and cursor. Overlapping owners preserve the first original window state
until its final owner releases it. Diagnostic connections do not acquire or clear
another session's state. EOF requires a private session-cleanup acknowledgement;
the app agent can remain alive between sessions. Process shutdown is the only
all-owner restoration path, and failed live restoration remains retryable.

The existing permission SDK reads the same captured helper, namespace and config
as MCP using bounded `doctor --json` diagnostics. Explicit local requests open
its overall permission onboarding and read both grants again. Windows reports its
execution platform without macOS TCC controls. Historical successful computer
command receipts are normalized through the central snapshot, private-backup and
transactional validation path into retained records containing the complete old
payload. Original request identities and fingerprints, approvals, canonical
history, immutable package resources and enable choices are preserved; retired
native computer commands are not executable.

### Pairing and reconnect

The pairing service exchanges a six-digit, 60-second code for an opaque invitation
ticket. Link authenticates the invited endpoint and persists trust. The service
can see tickets and is a short-lived trust boundary, not a business relay or
account system. Already paired peers reconnect without another short code.

An iroh data relay separately carries encrypted traffic when a direct connection
is unavailable. Pairing-service and data-relay URLs are different configuration.
Cloudflare and EdgeOne adapters share ticket-exchange logic, with native storage
adapters. See the [service contract](services/pairing-relay/README.md).

Relay settings belong to the execution Node and apply to its existing Link
identity and endpoint. A revision-checked durable command persists the desired
network before applying it; clients distinguish desired and effective settings
and recover uncertain completion through the original request ID. Pairing on a
selected network does not change the controller's own relay settings. Connection
route and RTT are local Link telemetry, separate from the Node event cursor.

## Extension and presentation boundaries

Official theme source lives in `sailry-themes`, pinned by the `themes/` Git
submodule. Its generated `catalog.json` records manifest metadata and resource
digests for discovery. Theme updates require strictly newer SemVer precedence;
same-version changes and build metadata alone do not offer updates. Digests
verify integrity. Numeric manifest and index formats remain at `1`. Desktop owns
the reusable local theme loader, importer and renderer; repository theme sources are
not embedded or installed automatically. Updating the source Gitlink does not
change a user's selected or installed theme.

The theme marketplace runs on the controller, including when a remote execution
Node is selected. It resolves the official source to one immutable commit and
acquires the strict manifest and its declared assets at that commit. Catalog
resource hints add integrity checks; cached metadata alone does not make an
unavailable package installable. Installation uses the existing staged
publication and rollback path, preserves manual imports, and records verified
official provenance. Updating the active package refreshes its decoded images
and colors without changing the selected appearance mode.

The application lives in `sailry-harness`; official plugin source lives only in
`sailry-plugins`. The `plugins/` Git submodule pins a reviewed plugin commit.
Builds embed that snapshot as ordinary package assets for offline use. Startup
adds missing selected defaults without resurrecting removed packages.
Startup refreshes already installed bundled packages only when their shipped
semantic version has strictly newer precedence, preserving enable choices and
settings. New official packages start at `0.1.0`; changed package content increments
the version, ordinarily its patch component. Same-version content changes, build
metadata alone and older versions do not offer updates. Digests and captured Git
commits remain immutable resource identities and installation integrity checks,
not plugin update detectors. Standalone imported skills retain their repository
commit update identity. This package policy does not declare a product release or
change Sailry's v1 contracts.
Removed packages and packages from other sources are not changed by this refresh.
The online official marketplace resolves the plugin repository's `main` to an
immutable commit and reads its `catalog.json` and skill sources at that commit.
The execution Node shares its validated index across plugin and skill listings,
source lookup and update checks for fifteen minutes. Explicit update checks and marketplace retries
refresh it; only successful validation replaces the Node-lifetime cache.
Unavailable networks leave the bundled snapshot installable. Invalid indexes are
reported, not treated as an offline result. Official installation and updates
capture immutable repository commits through Node's shared acquisition path.
Preinstalled packages can update from the official repository without an app
upgrade; update failures preserve installed packages and are reported honestly.
Official skill entries come from actual package skill frontmatter, not a separate
curated history. Older catalogs without a skill inventory use the real bundled
inventory; those entries remain identified as bundled and resolve their public
source through ordinary discovery before installation. Plugin listings choose an entire newer bundled entry over an older online version,
including its actual installation source; metadata is never grafted onto an older
remote package. Public skills.sh and Skillhub lists use Node-owned read-only pages,
search and bounded caches keyed by source, query and page. Identical network reads
share acquisition without serializing other keys. Signed registry details reuse
verified immutable documents; installation independently verifies the archive.
Verified registry imports project legacy metadata into the standard skill format
and retain the original document as `SOURCE-SKILL.md` alongside their provenance.
Web-hosted entries remain visible with their actual origin and
directory link when no supported installation source is known. Skill details read
real SKILL.md text from the resolved Git path, signed registry archive or shipped
package; inspection neither installs the skill nor executes its instructions.
Skills.sh single-character filtering is explicitly page-local; its
search does not invent pagination. Community details validate real repository
manifests rather than synthetic index documents. Confirmed external Codex or Claude
Code packages can expose their actual metadata and immutable standalone skills;
unsupported integrations and hooks are not executed. Invalid declared portable
manifests still fail validation. Package category, icon
tint and localized names are presentation metadata. Release notes are plain text
for the exact package version; displayed access categories do not grant permission.

Packages use the Agent Plugins manifest and Sailry's v1 extension contract.
Node owns validation, immutable resources, grants, private KV, callbacks,
tools and dispatch. Updates affect new admission; admitted work retains its
captured package and settings revision.
Installed, enabled model plugins are trusted to access their configured providers'
messages and login credentials without separate permission checkboxes. Ordinary
code updates retain that trust and existing accounts. Changes to the actual OAuth
issuer, client, scopes or flow require the corresponding authorization; package,
provider, preset and revision ownership and protected credential injection remain
enforced. Historical approval records are retained as data.
All packages own their pages, panels, dialogs, view state, workflows and model
prompts. Search and language-service settings are packaged Kit scripts using the
same captured Client commands for local and remote Nodes. Schema-only plugins
use the shared configuration form. Core never selects a feature page by package
name or a package-specific native renderer ID. It owns reusable execution,
persistence, transport and protected credential services; native UI adapters
cover documented Kit or platform gaps, without plugin business workflows.
Package-owned conversation options use the same revision-checked configuration
and immutable turn capture. Composer contributions select their own behavior;
initializers receive only the owning package's captured option namespace.
Retired expanded instructions remain private captured metadata conditioned on
their exact available tools, without replacing frozen package resources.
Controller settings actions require a declared settings surface and capability.
Standard OS permission contributions use a separate declared capability scope and
package-owned read-only check and explicit request callbacks; they do not require
a settings page. Settings aggregates one row per permission type while retaining
the execution device and authorization subject. Only missing subjects are requested,
with shared onboarding grouped per explicit operation. Core registers permissions
it actually uses; enabled plugin declarations supply additional subjects and are
removed from the directory on disablement, uninstall or device switch.
Agent calls retain their existing tool approvals. An optional `enabled_service`
binds one installed package's switch to a Node service. The binding is exclusive,
grants no capabilities, and keeps service shutdown atomic with enablement changes.
Knowledge retrieval has no standalone package or conversation entry; feature
plugins use its shared service.

Headless JavaScript callbacks use a bounded Node-owned QuickJS runtime. Desktop
entries use GPUI Kit script bindings and host-owned SDKs. Desktop and headless
exports are distinct; see the [SDK contract](sdk/plugins.md). Sailry's SDK does not
grant raw OS access. Desktop UI code is trusted code, not an OS sandbox.
Finite tool flows may re-read and recompute a private storage transaction after
the value owner confirms its key-CAS rejection committed no effects. Each attempt
retains the captured invocation, rechecks authority and has its own durable
request. Other failures and uncertain effects cannot authorize another step.
Packages own reconciliation of changed business data; Node does not replay
external tools or replace a failed operation with a success result.

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
the official installer verifies tag-bound assets and starts systemd or launchd.
Initial Desktop publication waits for full main Checks to complete successfully
at the tag's exact commit, then validates Desktop readiness. Desktop first
publishes explicitly named ad-hoc macOS arm64 and x64 packages, then starts independent formal signing and
Host packaging for that same tag. Formal signing reuses the preserved native
artifacts, adds signed and notarized canonical packages, and publishes the
release's signed `update-v1.json` catalog. Apple signing failures leave the
initial public packages available. Host appends macOS and Linux arm64 and x64
archives independently. Published tags and existing asset bytes are preserved;
release immutability remains disabled while these assets are appended. Host
installation is available once its assets appear. Mobile distribution remains manual.
The release-only `sailry-release` tool owns Desktop archive and catalog signing;
it is not linked into Desktop, Host or Node. Desktop discovers published releases
through GitHub's API and selects the highest compatible SemVer in its channel
with complete formal assets and a signed catalog. Stable excludes SemVer prereleases;
experience includes stable and prerelease versions. Desktop verifies the exact
v1 catalog payload, version and tag-bound archive URL, then the archive's
filename-bound signature. Ad-hoc packages are not automatic update candidates.
Desktop's Add Host command invokes that installer through Node-owned SSH, then
pairs through a private bootstrap ticket. It never uploads a bundled Host or
overwrites an existing profile. Desktop still includes its own local Node.
Office inspection uses Rust. Desktop previews original Office files and PDFs
offline in its sidebar WebView; it does not require conversion or provide a native
PDF export command. Document skills use the execution Node's bundled Python and
prepared plugin environments through ordinary command permissions.
Document attachment reads use the executing conversation's visible history,
including completed, failed and cancelled turns with no pending execution.
Queued, running or stopping inputs in other turns are excluded, even if their
admission happened earlier. Fork and rewind membership remains authoritative.
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
startup does not reset or delete user data. Normal startup validates current
configuration and records used by recovery, including the latest saved session
order. Immutable historical captures, receipts and history are validated when their
authoritative readers load them; unrelated archive damage therefore fails that
read instead of blocking startup. Upgrades and backup acceptance still validate
the complete stored data before applying changes. Third-party dependency versions, ordinary
data revisions and event sequences are independent. Release compatibility remains
a separate decision.

Core Settings owns connector, plugin, standalone skill and appearance management.
The native Market provides discovery, search, details, installation and available
package updates. Marketplace information uses the existing side pane for plugins,
skills and themes; Appearance retains its theme dialog. Plugin configuration opens
from core management actions and follows its declared settings surface. Declared
settings pages join the Settings menu at their declared group and order, including
pages rendered by the shared schema form. Configuration actions select the same
menu entry and retained page owner. Other configurable plugins use the
shared schema form dialog, with the Node and package captured when opened. MCP authorization is available
from connection cards and plugin details.
Plugins without configuration fields or a settings surface have no configuration action.
Scripted settings pages preload up to four validated resource bundles per captured
Node, warming on settings entry. Preloading never executes
scripts or reads configuration values. Workspace and scripted settings navigation
retain Kit's actual `ScriptView` and `ShellRoot` entities under their scoped owners.
First use loads a page; later selections display the same instance and run its
normal entry refresh without discarding drafts or briefly unmounting the page.
Hidden pages pause visibility-bound work. This uses GPUI's retained ownership,
not a separate KeepAlive, render-description cache or expiry policy.
Leaving the settings area releases prepared resources, not mounted page owners.
Prepared resources remain bounded, and package/settings revisions, removal,
invalid scopes and Node changes invalidate the relevant runtime. Window close
releases all page owners; already admitted Node tasks remain Node-owned.

Conversation export reads canonical history through Client. Per-reply actions select
one turn's user message and final reply; session exports include all user messages
and final replies.
Tool activity, reasoning and diagnostics are excluded. Markdown retains source syntax;
HTML prerenders formulas, diagrams and charts as inline static SVG and embeds
attachments in one scriptless file. Desktop's existing native WebView boundary
renders static HTML and bounded PNG tiles on macOS and Windows. Export captures
Node/session identity, supports cancellation and preserves changed destination files.
HTML and images include Sailry branding and the static website QR code.

The execution Node persists plugin notices, context-maintenance failures, and
session or terminal completion outcomes in one notification store. System notices
are committed with their source transition; diagnostics reference canonical event
and turn identities and contain safe classifications, not model content or provider
response bodies. Client reduces the same ordered notification events for local and
remote connections. Read and delete-read commands target each notice's execution
Node and do not change the underlying session, task, or terminal. Desktop exposes
durable history through the bell popover's notification-center entry, with optional
typed details and source navigation. Live approval and input reminders retain their
existing transient attention behavior in the popover.

## Security and quality constraints

Authenticated Node/resource access, path confinement, Node-owned credentials,
revision checks and honest uncertain outcomes are the primary boundaries.
Destructive actions identify their target and consequence. Imported sources and
resources retain their licenses; root AGPL licensing does not replace those
notices. Native OS permissions remain platform responsibilities.

Changed shared behavior requires local and remote-path evidence. UI interaction,
native OS acceptance, real-service integration and performance are separate
results. Isolated fixtures never mutate user profiles. Build success is not device
or whole-product acceptance; local pairing fixtures and bundling do not prove
cloud deployment. Test conventions are in [CONTRIBUTING.md](CONTRIBUTING.md).
