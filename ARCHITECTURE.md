# Architecture

## Purpose and scope

Sailry separates execution, transport, shared controller state and presentation.
Desktop and Host run the same Node implementation. Controllers never run a second
agent engine or write another authoritative session history. This document
describes repository boundaries and runtime contracts. It is not a delivery plan
or a claim of platform acceptance. Coding rules are in [AGENTS.md](AGENTS.md);
build, test and release procedures are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Implementation status

The Go module and repository are `github.com/sailry/sailry-harness`. Development
uses the source, locale catalogs, SDK declarations and pinned package submodules
in this checkout; no sibling source repository is required.

B0 contains the accepted MyGo desktop preview under `internal/desktop`, the
`cmd/sailry-desktop` entry point and pinned package snapshots. It starts no Node
and performs no backend work. The runtime components and contracts below describe
the target as their assigned backend phases land; their presence here does not
claim implementation or platform acceptance.

B1 supplies the shared v1 protocol and wire codec. B2 adds profile ownership,
confined files and storage initialization. The storage engine is the plan's
SQLite fallback after the required Turso load failed. B3 adds the local Node,
durable command admission, cursor subscriptions, Client projection and live
desktop sidebar, notification centre and controller preferences. Desktop starts
live by default; `--preview` selects the isolated fixture UI without opening a
profile. B4 adds Node-owned conversation execution, captured native API-key
providers, immutable canonical history, compaction, approvals, questions and the
durable queue. Fantasy supplies the per-step model interface; official provider
SDK adapters supply transport after the required Fantasy trial found protocol
gaps. Node-owned bounded conversation feeds carry drafts and committed changes;
Client owns history paging, reduction and shared detail reads for every pane.
B5 connects confined workspace file effects, checkpoints, Git, managed worktrees,
verified binary transfers, Office extraction, sandboxed processes, Web reads and
search, and captured hooks. Native Files/Git/History/Documents surfaces and
conversation assets consume the same Node resources through Client ownership.
B6 adds frozen team and role selection, delegated child execution through the
same loop, exact background completion delivery, captured skill resources and
package mode mapping. B7 adds authenticated iroh Link transport, persistent
pairing, remote Clients, Host service and control commands, Node-owned SSH Host
deployment, metrics and port streams. Desktop windows share one Network and
retain one bound Runtime per execution Node. The preserved Flutter application
uses the shared Client and Link through the Go C ABI and a thin Dart adapter.
B8 adds immutable installed packages, settings, private storage, QuickJS
callbacks and model translation, scoped HTTP and credentials, declared account
and connector flows, MCP pooling and elicitation, Python preparation, durable
dispatch and the installation queue. Live settings, skills, markets and theme
packages consume those owners. The AI CLI launcher includes the minimal
Node-owned terminal/Mirror and desktop Conn prerequisite; the remaining terminal,
browser and visual work keeps its assigned phase. Plugin-owned UI surfaces keep
their UI compatibility phase. Implementation does not imply acceptance on every target;
platform-specific runtime results are recorded separately.

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

| Component | Responsibility | Excludes |
| --- | --- | --- |
| `internal/protocol` | IDs, commands, events, snapshots, faults and capabilities | Runtime, UI and concrete transport |
| `internal/protocol/wire` | JSON framing, compression and snapshot paging | Business execution and UI |
| `internal/link` | Identity, pairing, authenticated transport, receipt and reconnect | Agent history and business execution |
| `internal/client` | Typed commands, ordered projections, deduplication and snapshot recovery | UI widgets, Node execution and a business database |
| `internal/node` | Admission, persistence, configuration, Agent execution and resource services | Flutter and controller presentation; MyGo except the pinned terminal runtime |
| `internal/core` | Node errors, profile ownership, workspace layout and confined file primitives | Storage, agents and transport |
| `internal/localmodels` | Local model registry, Hub discovery and CPU scheduling | Node storage, admission and controllers |
| `internal/localmodels/{embedding,ocr,decision,assets}` | CPU inference, model decoding and verified downloads | Node persistence and controller state |
| `internal/office` | Office and PDF text extraction | File access and OCR |
| `internal/databases` | External database drivers and SSH tunnels | Stored connections and approvals |
| `internal/ssh` | Verified SSH connection setup | Remote commands, files and terminals |
| `internal/speech` | Shared speech capability and native audio integration | Conversation ownership |
| `internal/desktop/kit` | Reusable MyGo controls, editors, Markdown, diffs and selection | Node, Client, file acquisition and application state |
| `internal/desktop` | MyGo composition, focus, drafts and native integrations | Duplicate history or request recovery engines |
| `internal/desktop/live` | Client projections, UI action implementations and local Node startup | Duplicate backend state or execution rules |
| `cmd/sailry-desktop` | Desktop composition and application lifecycle | Business execution outside Node |
| `cmd/sailry-host`, `internal/hostctl` | Headless service lifecycle and noninteractive control | Desktop dependencies and a prompt loop |
| `internal/mobile`, `cmd/sailry-mobile` | Client handles, subscriptions and Dart FFI adaptation | Node, agent execution, provider transport, PTY and Git execution |
| `apps/mobile` | Flutter controller presentation | Independent business rules or credential storage |
| `native/iroh`, `internal/link/iroh` | Pinned iroh C ABI and purego binding | Protocol framing and business execution |
| `bundled.go`, `plugins/`, `themes/` | Embedded, immutable submodule snapshots | Runtime package state |
| `services/pairing-relay` | Expiring short-code ticket exchange | Business commands or agent execution |

Production dependencies are acyclic:

```text
desktop/live → client + node
desktop UI   → MyGo + desktop/kit + desktop view models
host         → client + node + hostctl
mobile       → client + link + protocol + speech
client       → protocol + link
node         → protocol + link + fantasy transport + Node service packages
localmodels  → core + office + embedding + ocr + decision + assets
databases    → core + ssh
link         → protocol + transport dependencies
```

- Service packages (`core`, `office`, `databases`, `ssh`, `localmodels` and
  `speech`) never import `internal/node`. Client depends only on Protocol and
  Link, apart from standard and designated external libraries.
- Desktop UI never imports Node except through `internal/desktop/live` and
  `cmd/sailry-desktop`. Preview and live actions are separate implementations;
  live never imports preview.
- `internal/node/terminal` uses the pinned MyGo terminal package for its headless
  PTY and Mirror owner. That dependency imports MyGo UI types internally, but the
  Node creates no application, window or view. The verified headless prerequisite
  permits only this terminal dependency; controller presentation stays in Desktop.
- The Mobile adapter consumes only Client, Link, Protocol and Speech. Its C ABI
  uses `dart:ffi`, with no independent business rules or persistence.
- Go code uses `CGO_ENABLED=0` and purego for native libraries. The only cgo
  package is `cmd/sailry-mobile`, built for Android and iOS.
- Feature modules stay in their owning package. A new package needs a concrete
  dependency, binary, platform, compile-time or testing boundary.
- Node work uses goroutines and `context.Context`. Client callbacks invalidate
  the MyGo window; UI code never blocks on Node work. B0 keeps `Stored[T]` and
  `shell.Prefs` after the Nodra prototype failed its code-reduction criterion.
- Test fixtures may construct real Nodes without changing the production
  controller boundary.

## Data and ownership

| Owner | Owns |
| --- | --- |
| Node | Sessions, canonical history, configuration, credentials, immutable sent-turn revisions, admission and outcomes |
| Client | Event reduction, ordering, deduplication, snapshot recovery and shared detail caches |
| MyGo / Flutter | Layout, focus, drafts, selection and expansion |
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
- **Delegation.** Children inherit the admitted worktree, mode and permission;
  read-only roles use Plan. The existing execution owner claims children under
  the current founding session's concurrency budget and FIFO order. Foreground
  children from one response start after its canonical tool batch and settle
  before the parent's next inference. Durable background ownership survives
  the parent tool return.
- **Completion delivery.** Command and child completion evidence is reserved to
  one turn and consumed only with its exact canonical untrusted input. Optional
  automatic continuation uses the ordinary queue and cannot discard a committed
  completion when scheduling fails.
- **Skill resources.** The turn retains exact package digests and complete
  confined UTF-8 resources. Loading a skill grants no additional permission.
  Python preparation and package callbacks use the shared Node runtime owners.

### Storage

- The Node uses the pinned `modernc.org/sqlite` fallback for
  `storage/node.sqlite` and `storage/knowledge.sqlite`. B2's required Turso
  contention load exhausted the prescribed retries. Each file has one writer
  goroutine that group-commits queued operations in a `BEGIN IMMEDIATE`
  transaction, with a savepoint per operation, WAL and full synchronous
  durability. Callers receive results after commit. Reads use a separate
  read-only pool. Sequence counters advance with their records, so identifiers
  are never reused.
- Existing SQLite files are not imported. External database tools use their own
  PostgreSQL, MySQL and SQLite drivers, separate from Node storage.
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
- The selected SQLite fallback cancels release option A. It refuses Turso
  profiles and performs no import. A release profile decision is required before
  packaging; source builds still default to `~/.sailry-go`. Implementer runs use
  `--data-dir .runtime/profiles/<fixed-name>`.
- Schema objects, stored JSON shapes and profile files follow the Rust source;
  the SQLite fallback supplies FTS5 external-content indexes and native SQLite
  file families in place of Turso's index methods and file families.
  The one user-decided Go-only field is `surfaces.blur` in
  `desktop/preferences.json`, in percent with the UI default when absent.
  Theme packages define colors; surface transparency and blur are app settings.
- `ui-state.json` belongs beside MyGo's `window-state.json`, outside the Node
  profile. It stores UI-only `shell.Prefs`, including split layout trees.
- Both engines take the same exclusive OS advisory lock on `node.lock`; they
  never run concurrently on one profile. Controller profiles with `peers.json`
  cannot be opened as Node profiles.

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
  `agent/model`. The `Model` interface covers Fantasy-backed model transport and declared
  plugin models.
- **Fantasy.** `charm.land/fantasy` is pinned to `v0.46.0` and supplies only
  per-step `LanguageModel` streams and provider adapters. Sailry owns the loop,
  tool execution, approvals, persistence and delegation; Fantasy's agent loop,
  tool runner and retry layer are not used. Provider-specific official SDK
  adapters are permitted only as the B4 spike's documented fallback. Supported
  provider types retain the Rust request behavior.
- **Provenance.** Retained Goose-derived operations and provider adaptations keep
  their source revisions and Apache-2.0 notices. Imported transport libraries retain their own notices.
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
- **Code index.** A lazy Node service built on pure-Go
  `github.com/odvcencio/gotreesitter` at `v0.57.0`, subject to its B12 spike.
  Coverage is exactly that version's grammar registry; no languages are added or
  removed. A language outside the registry produces no outline or symbols.
  Nothing downloads at runtime. Graph queries
  are bounded name-based heuristics without type information. The code package owns
  `code_search`, `code_outline`, `code_context`, `code_impact` and `code_map`.
  Neither the editor nor the Agent starts language servers. Go uses its own rule
  fingerprint so each engine rebuilds the other's stale, disposable index rows.
  A failed parsing spike leaves the code index unbuilt; knowledge continues.
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
  optional for commands and skills without requirements. Immutable environments
  retain package, turn, process and MCP leases; no system interpreter is selected.
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
- **Runtimes.** `modernc.org/quickjs` is the one JavaScript engine for bounded
  Node callbacks and desktop plugin entries. The desktop compatibility layer maps
  the preserved v1 element and host SDK contracts onto MyGo/Kit (see the
  [SDK contract](sdk/plugins.md)). The SDK grants no raw OS access. Desktop UI code
  is trusted, not sandboxed. Files and Git use native core panels; the `files`
  and `git` plugins' desktop surfaces, including the Git status badge, are not
  mounted.
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
- **Skills.** Sailry resolves `SKILL.md` and confined resources from exact captured
  package references. YAML front matter uses the pinned YAML library; there is no
  additional Skills execution framework.

## Presentation boundaries

- MyGo and `internal/desktop/kit` own standard controls, semantic themes, focus,
  keyboard behavior and overlays. Desktop owns product composition and small
  native adapters. Accepted layouts and view-model fields remain stable.
- Terminal rendering uses MyGo's terminal plugin and libghostty-vt if its spike
  passes; the specified fallback is a purego binding with Sailry's renderer.
  The controls around the terminal remain Kit controls.
- Flutter consumes shared Client state rather than its own conversation state
  machine.
- Mermaid diagrams and math use limu and Untie through their MyGo adapters if
  their B10 spikes pass; failed rendering keeps source text. They never use a
  WebView. Session HTML uses an opaque-origin sandboxed MyGo WebView without a
  bridge; browser content remains untrusted.
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
- Desktop discovers remembered peers on peer changes and the source's ten-second
  observation cadence. It starts a shared Client only after identifying a
  compatible execution peer; paired controllers remain devices, not Nodes.
  iroh owns transport path recovery.
- A separate iroh data relay carries encrypted traffic when a direct connection is
  unavailable. Pairing-service and data-relay URLs are separate settings.
- Relay settings belong to the execution Node and are changed by a
  revision-checked durable command. Pairing on a selected network does not change
  the controller's own relay settings.
- Cloudflare and EdgeOne adapters share the ticket-exchange logic in
  `services/pairing-relay`. Its preserved source and provenance live in this
  checkout.

## Deployment and operations

- Desktop bundles presentation and a local Node. Host deploys the same Node as a
  headless process with a private profile and explicit network and pairing
  options. Pairing is opt-in for Host.
- Host archives are released independently of Desktop. Add Host runs over
  Node-owned SSH and pairs through a private bootstrap ticket. B7 installs a
  locally built Go Host; B14 connects the release installer feed, verifies
  tag-bound assets and starts systemd or launchd. Installation never overwrites
  an existing profile.
- The `sailry-host` management command talks to the running Host over a profile-private,
  owner-only Unix socket with a v1 control envelope. It never opens another Node or
  persistence owner. `submit` and `wait` use ordinary Client commands and
  subscriptions and never grant approvals. Usage is described in
  [Host automation](CONTRIBUTING.md#host-automation).
- Host metrics use one Node-owned sampler, read through the same authenticated
  command locally and remotely.
- Desktop uses MyGo's updater where it supports the signed `update-v1`
  contract; otherwise B13 ports the Rust catalog verification. It discovers
  GitHub releases and verifies the catalog and archive signatures. Stable takes final versions only; the
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


## Repository resources

`bundled.go` embeds official themes and the plugin snapshot directly from the
reviewed `themes/` and `plugins/` submodules. Runtime state never lives in those
source snapshots. There is no build-time theme copy and no
`internal/desktop/theme/builtin/` directory.

`docs/*.md` defines each UI area's view model and `Actions`. Backend adapters
feed those contracts without restyling the UI or renaming fields. All product
copy stays in `internal/desktop/i18n`; code and public documentation use English.
