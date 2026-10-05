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
| `apps/desktop` | GPUI Kit composition, focus, drafts and native integrations | Duplicate history or request recovery engines |
| `apps/host` | Service arguments, signals and shared Node lifecycle | Desktop dependencies and a prompt loop |
| `apps/mobile` | Flutter controller presentation | Independent business rules or credential storage |
| `services/pairing-relay` | Expiring short-code ticket exchange | Business commands or agent execution |

Production dependency direction is acyclic:

```text
desktop       → client + node-runtime + GPUI Kit
host          → node-runtime
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
Credentials remain in the execution Node's protected storage, never public events
or UI caches. Projects remain at registered paths; profiles contain Node storage,
plugin packages/data and uploaded attachments.

ADK-Rust is the agent execution engine. Node integrates Runner and SessionService
with one authoritative durable event history. Client and UI projections are
rebuildable derivatives, not additional history writers or runners. Usage reports
cover Sailry's own canonical events; Client aggregates reports across Nodes while
keeping source identity and completeness explicit. External tool histories,
account quotas and third-party billing are outside that boundary.

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

### Agent turns and tools

Node freezes configuration, scope and package revisions at turn admission. Tools
use the same approval, cancellation and durable outcome boundaries. Work mode
and permissions are distinct; read-only planning does not authorize side effects.
Delegated work uses the same ADK engine and history path with an explicit
parent/child association. Closing a view stops observation, not admitted work;
stopping a turn is an explicit command.

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

The application lives in `sailry-harness`; official plugin source lives only in
`sailry-plugins`. The `plugins/` Git submodule pins a reviewed plugin commit.
Builds embed that snapshot as ordinary package assets for offline use; new Node
profiles install the selected defaults once, without resurrecting removed packages.

Packages use the Agent Plugins manifest and Sailry's v1 extension contract.
Node owns validation, immutable resources, grants, private KV, callbacks,
tools and dispatch. Updates affect new admission; admitted work retains its
captured package and settings revision.

Headless JavaScript callbacks use a bounded Node-owned QuickJS runtime. Desktop
entries use GPUI Kit script bindings and host-owned SDKs. Desktop and headless
exports are distinct; see the [SDK contract](sdk/plugins.md). Sailry's SDK does not
grant raw OS access. Desktop UI code is trusted code, not an OS sandbox.

GPUI Kit owns standard controls, semantic themes, focus, keyboard behavior and
overlays. Desktop owns product composition and small native adapters. Ghostty's
terminal renderer is a custom rendering boundary; surrounding controls still use
Kit. Flutter consumes shared Client state rather than another conversation state
machine. UI preview data is isolated from execution and user profiles.

## Deployment and operations

Desktop bundles presentation and a local Node. Host deploys the same Node as a
headless process, using a private profile and explicit network/pairing options.
Only one process may own that profile. Pairing is opt-in for Host; established
peer trust survives restart. The macOS launchd helper generates a user-service
configuration but does not install or start it implicitly. Service lifecycle and
local verification are documented in the [pairing guide](services/pairing-relay/README.md#run-and-verify).

Shutdown cancels invitations and releases shared runtime resources through Node's
lifecycle. A disconnected client is not a shutdown signal. Restore reads durable
facts; it does not silently rerun interrupted side effects. Protected profiles,
credentials and local logs are not distributable source.

Before the first product release, Sailry protocol, wire, storage and extension
contracts remain v1 and change in place. Incompatible profiles are reported
without conversion, reset or deletion. Third-party dependency versions, ordinary
data revisions and event sequences are independent. A release compatibility policy
must be decided before introducing migrations.

## Security and quality constraints

Authenticated Node/resource access, path confinement, protected credentials,
revision checks and honest uncertain outcomes are the primary boundaries.
Destructive actions identify their target and consequence. Imported sources and
resources retain their licenses; root Apache licensing does not replace those
notices. Native OS permissions remain platform responsibilities.

Changed shared behavior requires local and remote-path evidence. UI interaction,
native OS acceptance, real-service integration and performance are separate
results. Isolated fixtures never mutate user profiles. Build success is not device
or whole-product acceptance; local pairing fixtures and bundling do not prove
cloud deployment. Test conventions are in [CONTRIBUTING.md](CONTRIBUTING.md).
