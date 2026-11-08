# Architecture

## Purpose and scope

Sailry separates execution, transport, shared controller state and presentation.
Desktop and Host run the same Node implementation. Controllers never run a second
agent engine or write another authoritative session history. This document
describes repository boundaries and runtime contracts. It is not a delivery plan
or a claim of platform acceptance. Coding rules are in [AGENTS.md](AGENTS.md);
build, test and release procedures are in [CONTRIBUTING.md](CONTRIBUTING.md).

## System context

Users control AI conversations and development resources from Desktop or Mobile.
An execution Node owns the work and runs inside Desktop or as Host. External
systems include model providers, SSH hosts, databases, Git repositories, MCP
servers and optional pairing and data-relay services.

```text
Desktop / GPUI Kit ── Client ──┬── in-process ── local Node
                              └── Link / iroh ── remote Node

Mobile / Flutter ── Rust bridge ── Client ── Link / iroh ── Node

Host service ── Node bootstrap + Link endpoint
Node ── Agent loop + scoped tools ── providers / files / Git / PTY / SSH / databases
```

- **Desktop** composes the UI with GPUI Kit and bundles a local Node.
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

| Component | Responsibility | Excludes |
| --- | --- | --- |
| `crates/protocol` | IDs, commands, events, snapshots, faults and capabilities | Runtime, UI and concrete transport |
| `crates/link` | Identity, pairing, authenticated transport, receipt and reconnect | Agent history and business execution |
| `crates/client` | Typed commands, ordered projections, deduplication and snapshot recovery | UI widgets, Node execution and a business database |
| `crates/node-runtime` | Admission, persistence, configuration, Agent execution and resource services | GPUI, Flutter and controller presentation |
| `crates/runtime-core` | Node errors, profile ownership, workspace layout and confined file primitives | Storage, agents and transport |
| `crates/local-models` | Local model registry, Hub discovery, CPU scheduling, OCR and decision services | Node storage, admission and controllers |
| `crates/office` | Office and PDF text extraction | File access and OCR |
| `crates/databases` | External database drivers and SSH tunnels | Stored connections and approvals |
| `crates/ssh` | Verified SSH connection setup | Remote commands, files and terminals |
| `crates/vt` | Ghostty terminal emulation and input encoding | PTY processes and terminal inventory |
| `crates/mobile-bridge` | Client handles, subscriptions and Rust/Dart adaptation | Node, Agent execution, provider transport, PTY, Git execution and tool drivers |
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

Production dependencies are acyclic:

```text
desktop       → client + node-runtime + content + GPUI Kit
content       → protocol + GPUI Kit
host          → client + node-runtime
mobile-bridge → client
client        → protocol + link
node-runtime  → protocol + link + rig-core + Node service crates
local-models  → runtime-core + office + embedding + ocr + decision + model-assets
databases     → runtime-core + ssh
link          → protocol + transport dependencies
```

- Node service crates (`runtime-core`, `local-models`, `office`, `databases`,
  `ssh`, `vt`) depend only on Protocol, each other and external libraries, never
  on Node storage or agents. Node re-exports them under its module paths.
- Desktop does not use Flutter Rust Bridge. The Mobile bridge adapts shared
  Client contracts, handles and async subscriptions only; it holds no business
  rules or persistence.
- Feature modules stay in their owning crate. A new crate needs a concrete
  dependency, binary, platform, compile-time or testing boundary.
- Test fixtures may construct real Nodes without changing the production
  controller boundary.

## Data and ownership

| Owner | Owns |
| --- | --- |
| Node | Sessions, canonical history, configuration, credentials, immutable sent-turn revisions, admission and outcomes |
| Client | Event reduction, ordering, deduplication, snapshot recovery and shared detail caches |
| GPUI / Flutter | Layout, focus, drafts, selection and expansion |
| Controller device | Identity, pairing records and rebuildable caches (not a business database) |

- **One profile, one Node.** A profile has one running Node owner, one Link
  identity and one endpoint. Desktop's local Client reuses it. Desktop and Host
  share bootstrap, recovery and shutdown.
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

### Storage

- The Node uses the native Turso driver for `storage/node.turso` and
  `storage/knowledge.turso`. Each database has one shared handle. Each operation
  leases a connection and keeps related SQL in one transaction, with full
  synchronous durability and MVCC. Sequence counters advance in the same
  transaction as their records, so identifiers are never reused.
- Existing SQLite files are not imported. External database tools keep their own
  SQLx drivers.
- Durable conversation records use Sailry-owned `Record`, `Message` and `Block`
  types. Binary data is stored once per content digest in the Node database.
  Unknown token counts stay unknown rather than becoming zero.
- `storage/knowledge.turso` holds only derived chunks, full-text indexes and
  vectors. Deleting it never removes sources or configuration.
- Each worktree has a rebuildable code index at
  `cache/code-index/<WorktreeId>/index.turso`. Source stays in the worktree.
- New managed checkouts live under the main worktree's `.sailry/worktrees/<id>`,
  excluded through Git's `info/exclude`.
- Disposable command, browser and transfer staging lives under the profile's `tmp`
  directory. Managed command output is bounded per stream and by a shared Node
  budget.

### Contract and storage versions

Before the first product release, Sailry protocol, wire, schema, storage and
extension contracts stay at `v1` (numeric fields use `1`) and change in place.
There are no legacy wire branches, fallback decoders or versioned migration
framework. Third-party dependency versions, data revisions and event sequences are
independent of this policy.

- Startup adds missing schema objects and fills missing storage fields with their
  defaults.
- An update is validated on an isolated snapshot, saved as a private backup, then
  applied with current-format validation in one transaction. Invalid current-format
  changes fail without applying the update.
- The current development baseline stores `node.storage_generation=1`. Owned Node
  storage without that column is development data: startup removes the Node and
  knowledge databases and creates an empty current database, without migration or
  backup. Generation 1 keeps its data. Unknown generations and other invalid
  current definitions fail without resetting storage.
- A validation failure never authorizes rebuilding a current database.
- Startup validates current configuration and recovery records. Historical
  captures and history are validated when their readers load them.
- Release compatibility is a separate future decision.

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
  access, not loopback networking.
- Link owns transport ordering and receipt, Client owns reduction and recovery,
  Node owns admission and outcomes. Transport receipt is not business success.
- Uncertain effects are reported as uncertain and never replayed automatically
  under a new identity. There is no second durable inbox.
- Conflicts require fresh inspection, never a silent overwrite.
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
  `agent/model`. The `Model` interface covers native Rig transport and declared
  plugin models.
- **Rig.** `rig-core` (pinned to `0.46.0` in the root workspace) is used only for
  provider transport over HTTP and SSE, behind `agent/model/rig`. Rig types never
  cross that boundary. Neither `rig-agent` nor GDK is a dependency. Supported
  provider types include OpenAI Responses and Chat, Anthropic, Google, Azure
  Foundry and OpenAI-compatible endpoints.
- **Provenance.** Retained Goose-derived operations and provider adaptations keep
  their source revisions and Apache-2.0 notices. Rig keeps its MIT notice.
- **Capture.** Admission freezes configuration, scope, model, reasoning effort,
  credential revision and package revisions. Updates affect only new admission.
- **Retry.** Sailry owns request-scoped recovery: up to five more attempts, 60
  seconds apart, after a transient failure. Ordinary requests stop automatic
  recovery once a stream event has escaped. Permanent request, authentication and
  malformed-stream errors are terminal. Completed tools are never replayed.
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
  Additional tools are discovered through `tool_search` without granting
  permissions. Approvals settle before calls run. Adjacent read-only concurrent
  calls run together, up to eight at a time. Results keep request order.
- **Approvals.** Node owns each approval's five-minute deadline.
- **Work modes.** Standard is the default. Plan is read-only with a reviewed
  transition to Standard. Flagship has Standard's permissions plus autonomous
  coordination. Package-declared modes map to a core mode without increasing
  permissions.
- **Delegation.** `spawn_agent`, `guide_agent` and `wait_agent` delegate through
  the same loop and history path with an explicit parent/child association. Child
  concurrency defaults to eight per root session and is set through
  `ReadTeamSettings`/`SaveTeamSettings`.
- **Guidance.** Controllers have one send action. Input sent while a task runs is
  admitted durably as guidance and joins at the next model boundary. Stop is the
  only interruption.
- **Streaming.** Partial text and reasoning use a bounded in-memory feed. Canonical
  messages commit before durable changes publish. Stopped or failed runs seal
  visible text as interrupted canonical output.
- **Background work.** Background children and commands record durable completion
  evidence. An idle session continues automatically only for completions that opt
  in.

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
- **Web.** Public web reads and search run on the execution Node. Direct reads pin
  public DNS on every redirect and reject private addresses. Configured HTTP(S)
  proxies are trusted to route public hostnames; proxy failures do not fall back to
  direct access. Search channels are tried in saved order.
- **Code index.** A lazy Node service built on the pinned Tree-sitter language pack.
  Parsers ship with the application and nothing downloads at runtime. Graph queries
  are bounded name-based heuristics without type information. The code package owns
  `code_search`, `code_outline`, `code_context`, `code_impact` and `code_map`.
  Neither the editor nor the Agent starts language servers.
- **Local models.** One Node-owned registry manages OCR, embedding and decision
  models: definitions, explicit installation, and verification of immutable
  revision, size and SHA-256. One CPU runtime runs one inference at a time, gives
  interactive work priority and releases idle engines. Decision inference uses the
  pinned llama.cpp C API and reports `Unavailable` on unsupported targets, with no
  fallback. No model is selected or downloaded automatically.
- **Knowledge.** Retrieval is a Node service used by plugin-owned features. No
  standalone knowledge plugin is bundled. Citations recheck source identity on the
  execution Node.
- **Python.** Desktop and Host bundle Python 3.12.13. Plugin environments are
  prepared lazily from hash-locked requirements using prebuilt wheels. Python is
  optional for commands and skills without requirements.
- **Session documents.** Bounded HTML is stored as immutable revisions. Preview
  HTML renders in an opaque-origin sandbox with no Node, tool, credential or
  filesystem bridge.
- **Hooks.** Per-worktree hook scripts are captured at admission, and a launched
  hook group is never replayed after a crash or cancellation.

## Extensions and plugins

- **Repositories.** Official plugins live only in `sailry-plugins`, pinned by the
  `plugins/` submodule. Official themes live in `sailry-themes`, pinned by
  `themes/`. Builds embed the pinned plugin snapshot for offline use. Desktop owns
  the reusable theme loader, importer and renderer.
- **Contract.** Packages use the Agent Plugins manifest and Sailry's v1 extension
  contract. Node owns validation, immutable resources, grants, private KV,
  callbacks, tools and dispatch. Admitted work keeps its captured package and
  settings revision.
- **Updates.** Startup adds missing default packages without resurrecting removed
  ones, and refreshes bundled packages only on strictly newer SemVer (versioning
  rules are in [AGENTS.md](AGENTS.md)). Online marketplaces read the website
  catalog and acquire resources from GitHub at a captured immutable commit. Update
  failures keep installed packages.
- **Ownership.** A plugin owns its pages, panels, dialogs, view state, workflows,
  tool orchestration, model prompts and product validation, including bundled
  plugins. Core provides reusable scoped services through Protocol, Client and the
  plugin SDK: process execution, LSP transport, package installation,
  persistence, search drivers, credentials, permissions and durable admission.
- **No package dispatch.** Core never selects UI or behavior by package name or a
  package-specific renderer ID. Shared lifecycle bindings are validated
  declarations, such as `enabled_service`, which binds one package's switch to a
  Node service without granting capabilities. Schema-only plugins use the shared
  configuration form.
- **Native bridges.** Limited to reusable Kit or platform gaps, such as terminal
  rendering, editor integration and protected credential inputs.
- **Runtimes.** Headless callbacks use a bounded Node-owned QuickJS runtime. Desktop
  entries use GPUI Kit script bindings and host SDKs (see the
  [SDK contract](sdk/plugins.md)). The SDK grants no raw OS access. Desktop UI code
  is trusted, not sandboxed.
- **Model plugins.** An installed, enabled model plugin is trusted with its
  configured providers' messages and login credentials, without separate permission
  checkboxes. Ordinary updates keep that trust. A changed OAuth issuer, client,
  scopes or flow needs new authorization. Account flows (for example `openai-auth`
  and `claude-auth`) are package steps that the Node runs through its private
  HTTP, PKCE, OIDC and credential services.
- **Service connections.** Package-declared connections are owned by the execution
  Node. Generic OAuth, token exchange, MCP OAuth and local CLI sources share one
  owner with no vendor dispatch. Only declared origins, headers, MCP servers and
  environment targets receive credentials.
- **MCP.** A Node-owned pool serves sessions and delegated work. Connections are
  keyed by package digest, settings revision, server, authorization and launch
  parameters. Server options (`tools`, `exclusive`, `keep_images`, `reuse`) are
  part of the v1 configuration. Failed or uncertain cleanup returns
  `OutcomeUnknown` and input is never replayed.

## Presentation boundaries

- GPUI Kit owns standard controls, semantic themes, focus, keyboard behavior and
  overlays. Desktop owns product composition and small native adapters.
- The Ghostty terminal renderer is a custom rendering boundary; the controls
  around it still use Kit.
- Flutter consumes shared Client state rather than its own conversation state
  machine.
- Mermaid diagrams, formulas and non-trivial raw HTML render in one serialized,
  controller-owned hidden WebView (macOS and Windows). Message rows embed no
  WebViews. Failures and Linux keep the original text.
- Voice reading and global selection shortcuts stay local to the controller and
  never start an Agent turn.

## Computer use

The `computer` package declares a stdio MCP server backed by Sailry's bundled
`agent-desktop` helper. The helper is staged from pinned, checksum-verified release
archives and is never downloaded at runtime. It uses semantic accessibility calls
rather than physical input. Every call goes through ordinary approval and an
exclusive lock.

- On macOS, the helper is a separately signed `Sailry Computer Use.app`
  (`dev.sailry.computer`, macOS 13+). Accessibility and Screen Recording grants
  belong to it, not to Desktop or Host.
- Windows uses `helpers/agent-desktop.exe`. Linux uses `helpers/agent-desktop` and
  requires glibc 2.35 and an AT-SPI bus.
- Every platform needs a logged-in graphical session. Desktop and Host keep working
  when the helper is missing.
- The Node owns one cursor-overlay session per helper home. The overlay never moves
  the physical pointer.

## Pairing and reconnect

- The pairing service exchanges a six-digit, 60-second code for an opaque
  invitation ticket. Link authenticates the invited endpoint and persists trust.
  The service is a short-lived trust boundary, not a business relay or account
  system.
- Paired peers reconnect without a new code. Established trust survives restart.
- A separate iroh data relay carries encrypted traffic when a direct connection is
  unavailable. Pairing-service and data-relay URLs are separate settings.
- Relay settings belong to the execution Node and are changed by a
  revision-checked durable command. Pairing on a selected network does not change
  the controller's own relay settings.
- Cloudflare and EdgeOne adapters share the ticket-exchange logic. See the
  [service contract](services/pairing-relay/README.md).

## Deployment and operations

- Desktop bundles presentation and a local Node. Host deploys the same Node as a
  headless process with a private profile and explicit network and pairing
  options. Pairing is opt-in for Host.
- Host archives are released independently of Desktop. The official installer
  verifies tag-bound assets and starts systemd or launchd. Desktop's Add Host
  command runs that installer over Node-owned SSH and pairs through a private
  bootstrap ticket. It never uploads a bundled Host or overwrites a profile.
- The `sailry` management command talks to the running Host over a profile-private,
  owner-only Unix socket with a v1 control envelope. It never opens another Node or
  persistence owner. `submit` and `wait` use ordinary Client commands and
  subscriptions and never grant approvals. Usage is described in
  [Host automation](CONTRIBUTING.md#host-automation).
- Host metrics use one Node-owned sampler, read through the same authenticated
  command locally and remotely.
- Desktop finds updates through GitHub Releases and verifies a signed `update-v1`
  catalog and the archive signature. Stable takes final versions only; the
  experience channel also takes prereleases. Release packaging is described in
  [CONTRIBUTING.md](CONTRIBUTING.md#release-packages).
- Shutdown cancels invitations and releases runtime resources through the Node
  lifecycle. A disconnected client is not a shutdown signal. Restore reads durable
  facts and never silently reruns interrupted side effects.
- Notifications (plugin notices, maintenance failures, session and terminal
  outcomes) live in one Node-owned store that Client reduces the same way locally
  and remotely.

## Security constraints

- The primary boundaries are authenticated Node and resource access, path
  confinement, Node-owned credentials, revision checks and honest reporting of
  uncertain outcomes.
- Destructive actions name their target and consequence.
- Public web reads reject private addresses. Preview HTML and browser content are
  untrusted.
- Native OS permissions remain the platform's responsibility.
- Protected profiles, credentials and local logs are not distributable source.
- Imported sources and resources keep their licenses. The root Apache-2.0 license
  does not replace those notices.
