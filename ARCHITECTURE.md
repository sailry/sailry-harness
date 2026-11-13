# Architecture

## Purpose and scope

Sailry separates execution, transport, shared controller state and presentation.
Desktop and Host run the same Node implementation. Controllers never run a second
agent engine or write another authoritative session history. This document
describes repository boundaries and runtime contracts. It is not a delivery plan
or a claim of platform acceptance. Coding rules are in [AGENTS.md](AGENTS.md);
build, test and release procedures are in [CONTRIBUTING.md](CONTRIBUTING.md).
User guides cover [hosts](docs/hosts.md), [models](docs/models.md)
and [projects](docs/projects.md).

## System context

Users control AI conversations and development resources from Desktop or Mobile.
An execution Node owns the work and runs inside Desktop or as Host. External
systems include model providers, SSH hosts, databases, Git repositories, MCP
servers and optional pairing and data-relay services.

```text
Desktop / MyGo ── Client ──┬── in-process ── local Node
                           └── Link / iroh ── remote Node

Mobile / Flutter ── Dart FFI adapter ── Client ── Link / iroh ── Node

Host service ── Node bootstrap + Link endpoint
Node ── Agent loop + scoped tools ── providers / files / Git / PTY / SSH / databases
```

- **Desktop** composes the UI with MyGo and Sailry Kit and bundles a local Node.
- **Mobile** is a Flutter controller. It connects to the selected Node directly;
  Desktop is not a gateway.
- **Host** is a long-running headless service, not an interactive agent CLI. Its
  command line starts and configures the service; conversations and tasks are
  controlled through the shared protocol.
- **Link** authenticates peers and carries commands, receipts and subscriptions.
- **Client** turns Node events into ordered, deduplicated projections.
- **Protocol** defines the shared command, event and snapshot types.
- **Plugins** and **themes** are packages maintained in separate repositories and
  pinned as submodules.

Disconnecting a controller does not stop admitted work.

## Components and dependencies

The Go module is `github.com/sailry/sailry-harness`.

| Component | Responsibility | Excludes |
| --- | --- | --- |
| `internal/protocol` | IDs, commands, events, snapshots, faults and capabilities, as hand-maintained declarations with one decoder | Runtime, UI and concrete transport |
| `internal/protocol/wire` | JSON envelopes, framing, compression and snapshot paging | Business execution and UI |
| `internal/link` | Identity, pairing, authenticated transport, receipt, reconnect and the in-process local transport | Agent history and business execution |
| `native/iroh`, `internal/link/iroh` | Pinned iroh C ABI and its purego binding | Protocol framing and business execution |
| `internal/client` | Typed commands, ordered projections, deduplication, request recovery and snapshot recovery | UI widgets, Node execution and a business database |
| `internal/acceptance` | Explicit manual conversation cases through an existing Client and private evidence reports | Node startup, credentials and UI automation |
| `internal/node` | Admission, persistence, configuration, Agent execution and resource services | Flutter and controller presentation |
| `internal/node/agent` | Turn engine, canonical history, model transport, compaction, prompt and the ephemeral feed | Storage schema and controller state |
| `internal/node/store` | The Node database: one writer, read pool, schema and event log | Execution rules |
| `internal/node/program` | Declared package executables: scrubbed start, process group, bound and idle stop | Protocols, vendors, shells and sockets |
| `internal/core` | Node errors, profile ownership and lock, workspace layout and confined file primitives | Storage, agents and transport |
| `internal/localmodels` | Local model registry, Hub discovery, verified installation and CPU scheduling | Node storage, admission and controllers |
| `internal/localmodels/{embedding,ocr,decision,onnx,assets}` | CPU inference engines, model decoding and verified downloads | Node persistence and controller state |
| `internal/office` | Office and PDF text extraction | File access and OCR |
| `internal/databases` | External PostgreSQL, MySQL and SQLite drivers and SSH tunnels | Stored connections and approvals |
| `internal/ssh` | Verified SSH connection setup | Remote commands, files and terminals |
| `internal/speech` | Offline speech recognition on the controller through sherpa-onnx | Node, transport, UI and conversation ownership |
| `internal/shipped` | Location of packaged files and the native library seal | Loading policy of each library |
| `internal/desktop/kit` | Reusable MyGo controls, floating surfaces, Markdown, code and the CrychicUI adapter | Node, Client, file acquisition and application state |
| `internal/desktop` | MyGo composition by feature: shell, sidebar, conversation, composer, panel, settings, host, transfers, onboarding, plugins, theme, i18n | Duplicate history or request recovery engines |
| `internal/desktop/live` | Client projections and the live implementation of every UI action | Backend state, execution rules and Node startup |
| `internal/desktop/preview` | Fixture data and simulated actions for `--preview` | Profiles, Nodes and any real effect |
| `internal/desktop/acceptance` | Explicit headless conversation interaction checks and a small live UI suite | Node startup, native window acceptance and automatic CI runs |
| `cmd/sailry-desktop` | Desktop composition, local Node startup and application lifecycle | Business execution outside Node |
| `cmd/sailry-host`, `internal/hostctl` | Headless service lifecycle, control socket and noninteractive task commands | Desktop dependencies and a prompt loop |
| `internal/mobile`, `cmd/sailry-mobile` | Client handles, subscriptions and the C ABI for Dart | Node, agent execution, provider transport, PTY and Git execution |
| `apps/mobile` | Flutter controller presentation | Independent business rules or credential storage |
| `bundled.go`, `plugins/`, `themes/` | Embedded, immutable submodule snapshots | Runtime package state |
| `services/pairing-relay` | Expiring short-code ticket exchange | Business commands or agent execution |

Production dependencies are acyclic:

```text
cmd/sailry-desktop → desktop + desktop/live + desktop/acceptance + acceptance + client + link + node
desktop/live       → client + link + protocol + speech + desktop UI packages
desktop UI         → MyGo + CrychicUI + desktop/kit + desktop view models
cmd/sailry-host    → acceptance + node + link + hostctl
hostctl            → client + link + core + protocol
acceptance         → client + core + protocol
mobile             → client + link + protocol
client             → protocol + link
node               → protocol + link + core + Node service packages + model transport
databases          → core + ssh
localmodels        → protocol + its engine packages
link               → protocol + protocol/wire + link/iroh
```

- Service packages (`core`, `office`, `databases`, `ssh`, `localmodels`,
  `speech` and `shipped`) never import `internal/node`. Client depends only on
  Protocol and Link, apart from standard and designated external libraries.
- Only `cmd/sailry-desktop` and `cmd/sailry-host` start a Node. No desktop
  package imports `internal/node` itself. `internal/desktop/live` is the one
  desktop package that uses Node leaf packages, and only for value types and
  pure helpers (`node/git`, `node/knowledge`, `node/plugins`, `node/providers`,
  `node/telemetry`); everything else reaches the Node through Client.
- Preview and live actions are separate implementations of the same `Actions`
  contracts. Live never imports preview.
- Manual acceptance is opt-in and command-only, through `just acceptance` and
  Host's acceptance command. Backend cases use Client commands and scoped
  conversation observations against the selected test Node. The runner journals
  ownership and unresolved request identities, saves bounded evidence, then
  removes only its recorded test conversations. `just acceptance-cleanup` can
  finish interrupted cleanup. UI checks run in a separate headless Desktop
  process because rendering changes process-wide theme state. Live checks attach
  that process to the same Node through its owner-only control socket. Recorded
  conversation snapshots and tool details can be replayed through the interface
  without a Node, model or tool execution; truncated recordings declare their
  limits. Normal startup, demo and CI never invoke real-model acceptance.
- The Node uses two MyGo packages and no MyGo application, window or view:
  `netproxy` for proxy-aware HTTP, and the terminal package (libghostty-vt)
  for the headless PTY and Mirror owner in `internal/node/terminal`. Controller
  presentation stays in Desktop.
- `internal/node/program` starts the executables a package declares and knows
  no protocol. The Agent Client Protocol model route in
  `internal/node/agent/model` is its one user. Packages reach neither from
  script.
- The Mobile adapter consumes only Client, Link and Protocol. Its C ABI is
  consumed through `dart:ffi`, with no independent business rules or
  persistence.
- Go code uses `CGO_ENABLED=0` and purego for native libraries. The only cgo
  package is `cmd/sailry-mobile`, built for Android and iOS.
- Feature modules stay in their owning package. A new package needs a concrete
  dependency, binary, platform, compile-time or testing boundary.
- Node work uses goroutines and `context.Context`. Client callbacks invalidate
  the MyGo window; UI code never blocks on Node work.

## Data and ownership

| Owner | Owns |
| --- | --- |
| Node | Sessions, canonical history, configuration, credentials, immutable sent-turn revisions, admission and outcomes |
| Client | Event reduction, ordering, deduplication, snapshot recovery and shared detail caches |
| MyGo / Flutter | Layout, focus, drafts, selection and expansion |
| Controller device | Identity, pairing records and rebuildable caches (not a business database) |

- **One profile, one Node.** A profile has one running Node owner, one Link
  identity and one endpoint. The owner holds an exclusive OS advisory lock on
  `node.lock`. Desktop's local Client reuses that Node. Desktop and Host share
  bootstrap, recovery and shutdown. A controller profile (one with
  `peers.json`) cannot be opened as a Node profile.
- **Resource ownership.** Resources keep Node → project → worktree →
  session/terminal ownership even when navigation hides the worktree layer. File
  and Git actions are confined to the captured worktree. SSH, database and plugin
  resources keep their captured Node and resource identity.
- **Session configuration.** Controller defaults initialize new sessions. Changing
  an existing session needs an explicit revision-checked command. Resuming from
  another controller keeps the effective configuration.
- **Credentials.** Configuration and credentials are stored as plaintext in the
  execution Node's configuration database. There is no encrypted vault or OS
  keychain. Credentials never appear in public events, logs or UI caches.
- **Profiles.** A profile contains Node storage, plugin packages and data, and
  uploaded attachments. Projects stay at their registered paths. Remote execution
  data stays on the execution Node.
- **Sessions.** `Session` is the shared conversation container for controllers,
  Host automation and plugins. `StartSession` atomically creates a session and
  admits its first turn. An optional immutable association (`namespace`, `kind`,
  `id`) links a session to an external object. It grants no resource authority.
- **Execution.** A turn selects a single model or a team. Team execution freezes
  the team revision, instructions and models with the turn.
- **Controller state.** Desktop keeps controller preferences in the profile's
  `desktop/preferences.json` and UI-only state (`ui-state.json`) beside MyGo's
  `window-state.json`, outside the Node profile.

### Storage

- The Node uses pinned `modernc.org/sqlite` for `storage/node.sqlite` and
  `storage/knowledge.sqlite`. Each file has one writer goroutine that
  group-commits queued operations in a `BEGIN IMMEDIATE` transaction, with a
  savepoint per operation, in WAL mode. Callers receive results after commit.
  Reads use a separate read-only pool. Sequence counters advance with their
  records, so identifiers are never reused.
- External database tools use their own PostgreSQL, MySQL and SQLite drivers,
  separate from Node storage.
- Durable conversation records use Sailry-owned `Record`, `Message` and `Block`
  types. Binary data is stored once per content digest in the Node database.
  Unknown token counts stay unknown rather than becoming zero.
- `storage/knowledge.sqlite` holds only derived chunks, full-text indexes and
  vectors. Deleting it never removes sources or configuration.
- Each worktree has a rebuildable code index at
  `cache/code-index/<WorktreeId>/index.sqlite`. Source stays in the worktree.
- New managed checkouts live under the main worktree's `.sailry/worktrees/<id>`,
  excluded through Git's `info/exclude`.
- Disposable command, browser and transfer staging lives under the profile's `tmp`
  directory, which is cleared at every start after the profile lock is taken.
  Managed command output is bounded per stream and by a shared Node budget.

### Contract and storage versions

Before the first product release, Sailry protocol, wire, schema, storage and
extension contracts stay at `v1` (numeric fields use `1`) and change in place.
There are no legacy wire branches, fallback decoders or versioned migration
framework. Third-party dependency versions, data revisions and event sequences are
independent of this policy.

A controller and a Host are compatible when their protocol versions are equal;
the software version is never compared. Changing a contract in place therefore
means two builds with the same protocol version exchange values the older one
does not declare, and the older one must keep working. One decoding rule covers
all of `internal/protocol`. It depends on who produced the bytes, not on the
type:

| The bytes carry | from a peer, as data (reply, event, snapshot) | from a peer, as a command | from an author |
| --- | --- | --- | --- |
| an unknown object member | ignored | ignored | refused |
| an unknown enum value | kept as sent | refused | refused |
| an unknown union variant | kept whole as `protocol.UnknownVariant` | refused | refused |
| no value for an optional member | the declared default | the declared default | the declared default |
| no value for a required member | refused | refused | refused |

- A peer is the other end of an authenticated Link running the same protocol
  version. The wire reader is the only place that asks for tolerance:
  `protocol.DecodeAs` with `PeerData` for a reply and a snapshot, `PeerCommand`
  for a request. Every other decode, including plain `json.Unmarshal`, is
  `Authored` and exact.
- Authored input is everything else: a package manifest and its validation, a
  plugin's script input and transaction operations, model tool arguments,
  settings files, stored rows, the pairing frames exchanged before a peer is
  authenticated, and the Dart command JSON of the mobile adapter. A value there
  that this build does not declare is a mistake or an attack, never a newer
  contract.
- A Node acts on a command, so it never acts on a value it does not understand:
  it ignores a member it does not declare and refuses an undeclared enum value
  or variant with a fault the sender can show. A controller only displays data,
  so an undeclared value reaches it intact, re-encodes byte for byte, and each
  consumer treats it as no information: an unknown event or frame change is
  skipped while its sequence still advances, an unknown part or kind is not
  drawn.
- Digests and signatures are computed over retained raw JSON, which no
  tolerance level changes.
- The consequence for a contract change: a new member is optional with a
  default that means "as before", a new enum value or variant is sent only
  where its receiver is data, and a required member is never added. A change
  that cannot follow this needs a new protocol version.
- The protocol files are maintained by hand. Their fixtures and tolerance tests
  are the check on a change; there is no generator.

Storage follows the same policy:

- Startup adds missing schema objects and fills missing storage fields with their
  defaults.
- An update is validated on an isolated snapshot, saved as a private backup, then
  applied with current-format validation in one transaction. Invalid current-format
  changes fail without applying the update.
- The baseline stores `node.storage_generation=1`. Startup accepts an empty
  database or owned Node storage at generation 1. Any other state, including a
  missing or unknown generation, a foreign application or another Node's
  identity, fails without changing storage; no startup path deletes a database.
- A validation failure never authorizes rebuilding a current database.
- Startup validates current configuration and recovery records. Historical
  captures and history are validated when their readers load them.
- Database files of another storage engine found in a profile are not opened or
  imported. They move unchanged to `storage/rust-archive/`; the Node identity,
  controller preferences, installed themes and the profile's file trees are
  kept.
- The default profile is `~/.sailry-go` for source builds and `~/.sailry` for
  release builds, which set `core.Default` at link time.

### Backups

Data management backups include both databases, referenced plugin packages,
attachments, session documents and registered managed workspaces. Maintenance
pauses new transactions, checkpoints and closes the databases, and copies their
complete native file families. Archives are password-encrypted (age) and can target
local folders, WebDAV or S3. Restore validates into a new empty profile, keeps the
Node identity and does not start the profile automatically. Only one copy of a
Node identity should run at a time. Automatic backups use the existing Node
dispatch service and require an online Node.

## Command admission and recovery

1. Client submits a typed command with a stable request ID and captured resources.
2. Node validates scope, revisions and permissions. A durable command is admitted
   durably before Link acknowledges safe receipt.
3. Node executes the operation and records its business outcome.
4. Client observes the result and ordered events. Reconnect uses the original
   request ID and snapshot/subscription recovery.

- Local access uses an in-process adapter with the same semantics as remote
  access, not loopback networking. The local Link fixes the authenticated caller
  at construction and calls the same Node handler as every other transport.
- Link owns transport ordering and receipt, Client owns reduction and recovery,
  Node owns admission and outcomes. Transport receipt is not business success.
- A durable request is identified by its authenticated caller and request ID.
  Replaying a completed request returns its stored success or fault without
  another effect; reusing an ID for different content is a conflict.
- A request still admitted when the Node stops is marked unknown at the next
  start and never runs again automatically. `InspectRequest` tells a controller
  what became of a request. Uncertain effects are reported as uncertain and
  never replayed under a new identity. There is no second durable inbox.
- Conflicts require fresh inspection, never a silent overwrite.
- A snapshot and its cursor come from one read transaction; later payloads come
  from the committed event log in cursor order. A cursor the Node can no longer
  serve produces a reset: Client takes a fresh snapshot and never replays
  commands.
- Installations of plugins, local models and language packages share one bounded
  Node queue. Work on the same resource keeps admission order. Failed and
  uncertain installations remain visible until dismissed.

## Agent loop and model boundary

- **Loop.** The asynchronous turn loop in `agent/engine` owns Agent execution. It
  runs ordered operations for approvals, tools, child work, guidance, compaction,
  error handling and inference. Each stage persists its effects before the next
  session reload.
- **History.** Node session services are the single canonical history writer.
  Client and UI projections are read-only, rebuildable derivatives.
- **Model types.** Requests, messages and events use Sailry-owned types in
  `agent/model`. One binding covers three per-step transports: provider SDK
  adapters, package-declared model translations and agent programs.
- **Fantasy.** `charm.land/fantasy` is pinned to `v0.46.0` and supplies only the
  per-step `LanguageModel` stream interface. Sailry owns the loop, tool
  execution, approvals, persistence and delegation; Fantasy's agent loop, tool
  runner and retry layer are not used. Requests to OpenAI, Anthropic and Google
  style endpoints go through the providers' official SDKs with their own retries
  disabled.
- **Agent programs.** An installed command-line agent that speaks the Agent
  Client Protocol is used as a plain model through pinned
  `github.com/coder/acp-go-sdk`. Sailry still owns the loop, tools, history and
  compaction; the route offers the agent no file, terminal or MCP capability,
  refuses its permission requests and carries tool calls as text. The route
  names no vendor: a package declares the executable and its differences as
  data. The one such package is `agy` (Antigravity), installed on request.
  There is no other command-line agent channel and no vendor-private protocol.
- **Provenance.** Retained Goose-derived operations and provider adaptations keep
  their source revisions and Apache-2.0 notices. Retained Rig- and omp-derived
  source keeps its MIT notices beside the code in `agent/model`. Imported
  transport libraries retain their own notices.
- **Capture.** Admission freezes configuration, scope, model, reasoning effort,
  credential revision and package revisions. Updates affect only new admission.
- **Retry.** Sailry owns request-scoped recovery. A model request, whether an
  ordinary one or a compaction summary, is sent again up to nine times after a
  transient failure, each time after a fixed 60 seconds that a provider's
  retry hint neither shortens nor lengthens. Ordinary requests stop automatic
  recovery once a stream event has escaped; summary requests are buffered.
  Permanent request, authentication and malformed-stream errors are terminal.
  Completed tools are never replayed.
- **Prompt stability.** Policy, personal instructions, guides and selected
  capabilities form a stable system prompt. Per-turn state goes in an agent-only
  context message that is appended only when session state changes, so each
  request extends the previous one and provider prompt caches stay valid.
- **Compaction.** Automatic compaction defaults to 80% of the captured context
  window, configurable per Node and frozen per turn. Summary requests have no tools
  and use the captured model. Original records and raw tool output are preserved;
  the request loader uses the latest summary. Context-length errors can trigger at
  most two reactive compactions.
- **Tools.** Each turn builds model declarations from its captured tool instances.
  The everyday file, command, Git, code and web tools are offered on the first
  request; the others are listed by name and discovered through `tool_search`
  without granting permissions. The model reads declared tool names; internal
  aliases stay behind the model boundary. Approvals settle before calls run.
  Adjacent read-only concurrent calls run together, up to eight at a time.
  Results keep request order.
- **Approvals.** Node owns each approval's and question's five-minute deadline.
- **Work modes.** Standard (`code` on the wire) is the ordinary mode. Plan is
  read-only with a reviewed transition to Standard. Flagship has Standard's
  permissions plus autonomous coordination. Package-declared modes map to a
  core mode without increasing permissions.
- **Delegation.** `spawn_agent`, `guide_agent` and `wait_agent` delegate through
  the same loop and history path with an explicit parent/child association.
  Children inherit the admitted worktree, mode and permission; read-only roles
  use Plan. Child concurrency defaults to eight per root session and is set
  through `ReadTeamSettings`/`SaveTeamSettings`.
- **Guidance.** Controllers have one send action. Input sent while a task runs is
  admitted durably as guidance and joins at the next model boundary. Stop is the
  only interruption.
- **Streaming.** Partial text and reasoning use a bounded in-memory feed. Canonical
  messages commit before durable changes publish. Stopped or failed runs seal
  visible text as interrupted canonical output.
- **Background work.** Background children and commands record durable completion
  evidence. Evidence is reserved to one turn and consumed only with its exact
  canonical input. An idle session continues automatically only for completions
  that opt in, through the ordinary queue.

## Tools and resource services

- **Commands.** On macOS and Linux, Agent commands run in a workspace sandbox by
  default: workspace writes, read-only toolchains, private temporary files and no
  network. Full permission grants the complete environment. Workspace commands
  cannot write captured Git metadata. User-created terminals start with the full
  login-shell environment.
- **Files.** Discovery and search run inside Node with confined paths, ignore rules
  and bounded pages. Multi-file change sets are validated before one approval and
  applied in order with per-file revision checks, stopping at the first failure.
  `apply_patch` accepts the Codex patch format.
- **Managed worktrees.** Agent and headless file operations stay inside their own
  checkout and cannot reach `.sailry/worktrees`. Authenticated manual controller
  workflows can access those descendants through the registered-root services,
  still protecting Git metadata, the managed checkout root and Node storage.
- **Terminals.** The Node owns each PTY and its screen mirror; controllers
  attach to the mirror locally or over Link.
- **Web.** Public web reads and search run on the execution Node. Direct reads pin
  public DNS on every redirect and reject private addresses. Configured HTTP(S)
  proxies are trusted to route public hostnames; proxy failures do not fall back to
  direct access. Search channels are tried in saved order.
- **Browser.** The managed external browser is a Node-owned Chromium process tree
  per conversation session with its own user data directory. It never attaches
  to a browser the user runs.
- **Code index.** A lazy Node service built on pure-Go
  `github.com/odvcencio/gotreesitter` at `v0.57.0`. Coverage is exactly that
  version's grammar registry; a language outside it produces no outline or
  symbols. Nothing downloads at runtime. Graph queries are bounded name-based
  heuristics without type information. The code package owns `code_search`,
  `code_outline`, `code_context`, `code_impact` and `code_map`. Neither the
  editor nor the Agent starts language servers.
- **Local models.** One Node-owned registry manages OCR, embedding and decision
  models: definitions, explicit installation, and verification of immutable
  revision, size and SHA-256. One CPU runtime runs one inference at a time, gives
  interactive work priority and releases idle engines. Decision inference uses the
  pinned llama.cpp C API and reports `Unavailable` on unsupported targets, with no
  fallback. No decision model is selected automatically; embedding initially
  selects `granite-multi`. Model downloads require explicit installation.
- **Knowledge.** Retrieval is a Node service used by plugin-owned features. No
  standalone knowledge plugin is bundled. Citations recheck source identity on the
  execution Node.
- **Python.** Desktop and Host bundle Python 3.12.13. Plugin environments are
  prepared lazily from hash-locked requirements using prebuilt wheels. Python is
  optional for commands and skills without requirements. No system interpreter
  is selected.
- **Session documents.** Bounded HTML is stored as immutable revisions. Preview
  HTML renders in an opaque-origin sandbox with no Node, tool, credential or
  filesystem bridge.
- **Hooks.** Per-worktree hook scripts are captured at admission, and a launched
  hook group is never replayed after a crash or cancellation.
- **Dispatch.** Schedules are durable Node records driven by Sailry's own
  dispatch calendar on standard timers. Automatic backups and package schedules
  use it.

## Extensions and plugins

- **Repositories.** Official plugins live only in `sailry-plugins`, pinned by the
  `plugins/` submodule. Official themes live in `sailry-themes`, pinned by
  `themes/`. Builds embed the pinned snapshots for offline use. Desktop owns
  the reusable theme loader, importer and renderer.
- **Contract.** Packages use the Agent Plugins manifest and Sailry's v1 extension
  contract, published under `schemas/` and described in the
  [SDK contract](sdk/plugins.md). Node owns validation, immutable resources,
  grants, private KV, callbacks, tools and dispatch. Admitted work keeps its
  captured package and settings revision.
- **Updates.** Startup adds missing default packages without resurrecting removed
  ones, and refreshes bundled packages only on strictly newer SemVer (versioning
  rules are in [AGENTS.md](AGENTS.md)). Online marketplaces read the website
  catalog and acquire resources from GitHub at a captured immutable commit. Update
  failures keep installed packages.
- **Ownership.** A plugin owns its pages, panels, dialogs, view state, workflows,
  tool orchestration, model prompts and product validation, including bundled
  plugins. Core provides reusable scoped services through Protocol, Client and the
  plugin SDK: process execution, package installation, persistence, search
  drivers, credentials, permissions and durable admission.
- **No package dispatch.** Core never selects UI or behavior by package name or a
  package-specific renderer ID. Shared lifecycle bindings are validated
  declarations, such as `enabled_service`, which binds one package's switch to a
  Node service without granting capabilities. Schema-only plugins use the shared
  configuration form.
- **Native bridges.** Limited to reusable Kit or platform gaps, such as terminal
  rendering, editor integration and protected credential inputs.
- **Runtimes.** `github.com/MoeForge/moejs v0.1.0` is the one JavaScript engine,
  for bounded Node callbacks, model translation and desktop plugin entries. A
  callback has an allocation budget, a deadline and cancellation, and a
  recursion fault never crashes the host. The desktop compatibility layer in
  `internal/desktop/plugins` maps the v1 element and host SDK contracts onto
  MyGo and Kit. The SDK grants no raw OS access. Desktop UI code is trusted, not
  sandboxed. Files, Git and local terminals use native core panels; a package's
  desktop surface for them is not mounted.
- **Model plugins.** An installed, enabled model plugin is trusted with its
  configured providers' messages and login credentials, without separate permission
  checkboxes. Ordinary updates keep that trust. A changed OAuth issuer, client,
  scopes or flow needs new authorization. Account sign-ins for Claude, ChatGPT
  and Copilot are the `claude-auth`, `openai-auth` and `copilot-auth` packages:
  package steps that the Node runs through its private HTTP, PKCE, OIDC and
  credential services. `workbuddy` is installed by default alongside them.
- **Declared programs.** A package may declare executables, under the
  `process.run` action the user sees. The Node starts them without a shell,
  with a scrubbed environment, on demand, bounded and stopped when idle.
  Packages get no script-level process API.
- **Service connections.** Package-declared connections are owned by the execution
  Node. Generic OAuth, token exchange, MCP OAuth and local CLI sources share one
  owner with no vendor dispatch. Only declared origins, headers, MCP servers and
  environment targets receive credentials.
- **MCP.** A Node-owned pool serves sessions and delegated work. Connections are
  keyed by package digest, settings revision, server, authorization and launch
  parameters. Server options (`tools`, `exclusive`, `keep_images`, `reuse`) are
  part of the v1 configuration. Failed or uncertain cleanup returns
  `OutcomeUnknown` and input is never replayed.
- **Skills.** Sailry resolves `SKILL.md` and confined resources from exact captured
  package references. The turn retains package digests and complete resources.
  Loading a skill grants no additional permission, and there is no separate
  Skills execution framework.

## Presentation boundaries

- MyGo and `internal/desktop/kit` own standard controls, semantic themes, focus,
  keyboard behavior and overlays, with one application theme and semantic
  tokens. Desktop owns product composition and small native adapters. Each
  feature package exposes a view model and an `Actions` contract; preview and
  live fill the same contract.
- CrychicUI is the second UI library, after MyGo's controls and before Kit or
  custom drawing ([AGENTS.md](AGENTS.md) lists the verified components). Its
  sound is off, data Sailry does not author passes through the adapter in
  `internal/desktop/kit/crychic`, and floating surfaces stay Kit's frosted
  ones: CrychicUI supplies their content, never the surface.
- Terminal rendering uses MyGo's terminal package on libghostty-vt. The
  controls around the terminal remain Kit controls.
- Mermaid diagrams are drawn by CrychicUI's `diagram.Mermaid`, xychart and pie
  by the conversation's own painter from limu's parsed model, and math by
  Untie through its MyGo adapter. Failed rendering keeps source text. None of
  them uses a WebView.
- Session HTML uses an opaque-origin sandboxed MyGo WebView without a bridge;
  browser content remains untrusted.
- The right panel draws text, code, Markdown, notebooks and images natively.
  Formats MyGo cannot draw, such as PDF and Office files, have no preview: they
  open in the system application.
- Flutter consumes shared Client state through the Dart FFI adapter rather than
  its own conversation state machine. Client owns projections, history paging
  and transfer verification; Dart owns Flutter view state.
- Voice reading, speech recognition and global selection shortcuts stay local
  to the controller and never start an Agent turn.
- `--preview` shows fixture data without opening a profile or starting a Node.

## Computer use

The `computer` package declares a stdio MCP server backed by Sailry's bundled
`agent-desktop` helper. The helper is staged from pinned, checksum-verified release
archives and is never searched on `PATH` or downloaded at runtime. It uses
semantic accessibility calls rather than physical input. Every call goes through
ordinary approval and an exclusive lock.
On macOS the helper is a separately signed `Sailry Computer Use.app`, and the
Accessibility and Screen Recording grants belong to it, not to Desktop or Host.
Every platform needs a logged-in graphical session; Desktop and Host keep
working when the helper is missing. The Node owns one cursor-overlay session per
helper home, and the overlay never moves the physical pointer.

## Pairing and reconnect

- Link transport is iroh, pinned in `native/iroh` and loaded through purego.
  Peers authenticate by endpoint identity.
- The pairing service exchanges a six-digit code, valid for one minute, for an
  opaque invitation ticket. Link authenticates the invited endpoint and persists
  trust. The service is a short-lived trust boundary, not a business relay or
  account system.
- Paired peers reconnect without a new code. Established trust survives restart.
- Desktop windows share one Network. It discovers remembered peers on peer
  changes and on a ten-second cadence, and starts a shared Client only after
  identifying a compatible execution peer; paired controllers remain devices,
  not Nodes. iroh owns transport path recovery.
- A separate iroh data relay carries encrypted traffic when a direct connection is
  unavailable. Pairing-service and data-relay URLs are separate settings.
- Relay settings belong to the execution Node and are changed by a
  revision-checked durable command. Pairing on a selected network does not change
  the controller's own relay settings.
- Cloudflare and EdgeOne adapters share the ticket-exchange logic in
  `services/pairing-relay`.

## Deployment and operations

- Desktop bundles presentation and a local Node. Host deploys the same Node as a
  headless process with a private profile and explicit network and pairing
  options. Pairing is opt-in for Host.
- Host archives are released independently of Desktop. Add Host runs over
  Node-owned SSH, installs the release that matches the controller's product
  version from checksum-verified assets, starts systemd or launchd and pairs
  through a private bootstrap ticket. Installation never overwrites an existing
  profile. On Windows, a Linux Host is deployed into a WSL2 distribution.
- The `sailry-host` management command talks to the running Host over a
  profile-private, owner-only Unix socket with a v1 control envelope. It never
  opens another Node or persistence owner. `submit` and `wait` use ordinary
  Client commands and subscriptions and never grant approvals. Usage is
  described in [Host automation](CONTRIBUTING.md#host-automation).
- Host metrics use one Node-owned sampler, read through the same authenticated
  command locally and remotely.
- `internal/desktop/updater` checks, downloads, verifies and installs signed
  `update-v1` releases: it verifies the catalog and archive signatures against
  the publisher keys compiled into it. Stable takes final versions only; the
  experience channel also takes prereleases. A controller update never replaces
  or shuts down a remote Host. Release packaging is described in
  [CONTRIBUTING.md](CONTRIBUTING.md#release-packages).
- `internal/shipped` locates packaged files and verifies native libraries
  against the package seal written after signing. A release build loads the
  iroh and capture libraries only from its package and only when the seal
  records them.
- Shutdown rejects new work, closes subscriptions, joins admitted operations,
  closes storage and then releases the profile lock. A disconnected client is
  not a shutdown signal. Restore reads durable facts and never silently reruns
  interrupted side effects.
- Notifications (plugin notices, maintenance failures, session and terminal
  outcomes) live in one Node-owned store that Client reduces the same way locally
  and remotely.
- Telemetry sends nothing and runs nothing in the background while its switch
  is off.

## Security constraints

- The primary boundaries are authenticated Node and resource access, path
  confinement, Node-owned credentials, revision checks and honest reporting of
  uncertain outcomes.
- Destructive actions name their target and consequence.
- Public web reads reject private addresses. Preview HTML and browser content are
  untrusted.
- An agent program's own tools and permission requests are refused.
- Native OS permissions remain the platform's responsibility.
- Protected profiles, credentials and local logs are not distributable source.
- Imported sources and resources keep their licenses. The root Apache-2.0 license
  does not replace those notices; `NOTICE` and `third_party_licenses/` index
  them.

## Repository resources

`bundled.go` embeds official themes and the plugin snapshot directly from the
reviewed `themes/` and `plugins/` submodules. Runtime state never lives in those
source snapshots, and there is no build-time copy of either.

Beside the Go packages above, the checkout holds the Flutter application
(`apps/mobile`), the pinned native libraries with their lock files and staging
tool (`native/`), the v1 plugin SDK declarations and published contract schemas
(`sdk/`, `schemas/`), the source catalogs of the desktop i18n generator
(`locales/`), the pairing service (`services/pairing-relay`), the public user
guides (`docs/`) and the index of third-party notices
(`third_party_licenses/`). All product copy stays in `internal/desktop/i18n`;
code and public documentation use English.
