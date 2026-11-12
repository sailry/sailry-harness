# Node

`internal/node` owns the execution profile shared by Desktop and Host. `Start`
acquires the profile lock, opens the persisted Link identity and Store, and marks
unfinished admissions unknown before accepting commands. `Stop` rejects new
work, closes subscriptions, joins admitted operations and reads, closes storage,
then releases the profile lock. Cancelling a caller's wait never releases the
profile while admitted work remains active.

The local Link fixes authenticated caller identity at construction and calls the
same Node handler used by other transports. Controllers own observation; Node
owns persistence and execution. The desktop preview starts no Node.

## Admission and outcomes

A durable request is identified by authenticated caller and request ID. Its exact
serialized v1 request is the replay identity. An admission receipt is returned
only after its ledger transaction commits. Work thereafter uses a Node-owned
context and is independent of the controller's observation context.

Completed replay returns its original success or fault without another mutation
or event. Reusing an ID for different content returns Conflict. An admitted or
unknown request returns OutcomeUnknown and never executes again automatically.

Every admitted request ends in one of two final states
(`internal/node/admission`):

- `completed`: it ended, and its success or its fault is stored. A request
  whose whole effect was in the transaction that failed changed nothing, so
  that failure is stored as its result (`Node.unapplied`); a retry gets the
  same fault and the controller starts a new request.
- `unknown`: the Node stopped while the request ran. Startup sets this for
  every request still `admitted`, and it never changes again. The Node cannot
  tell which of them took effect, so it does not guess. The one exception is
  a request that another Node holds under the same identifier
  ([Session forwarding](#session-forwarding)): that Node's answer replaces an
  outcome stored as not known (`admission.Resolve`).

`admitted` therefore always means "running, or its result is being stored".
A result that could not be stored with its transaction is written again in a
transaction of its own every second (`admission.Settle`) until it is stored or
the Node stops; a stopping Node makes one last attempt. Only a commit that the
store could not confirm leaves a request admitted until the next start.

A workspace command (a file, Git, terminal or process effect, and the tool
calls of a turn that run them) stores its resource metadata with its result.
When that transaction fails, `recordWorkspaceEffect` settles the request by
what was lost: an operation that failed, or one with only its result to
store, keeps that result; an effect outside the database whose metadata was
in the transaction is stored as `outcome_unknown` "operation completed but
its resource metadata could not be recorded; inspect the target before
retrying"; a command whose whole change was in the transaction is stored
with that failure, as above.

`InspectRequest` answers "what became of request X" for the controller that
sent it: `not_admitted`, `admitted`, `completed` with the stored result, or
`unknown`. Any paired or local controller may ask; a Node that forwarded a
request to another Node asks that Node the same way, with
`admission.Digest` of the serialized request. Controllers resolve every
unobserved outcome through `Client.Settle`, described in
[Unobserved outcomes](#unobserved-outcomes).

Completed and unknown requests are deleted `admission.Retention` (7 days)
after their admission, at startup and once a day. A retry later than that is
a new request. Requests that a turn, a queued turn, an approval or a dispatch
job refers to are kept, because those rows read their input and result from
the ledger; they go when their owner is removed. The `requests.at` column
(Unix seconds, added in place) holds the admission time; a row written before
it existed is dated at the first pass that sees it.

`InspectRequest` checks a lowercase 64-character BLAKE3 digest and reports the
recorded outcome without admitting or replaying the original command. The digest
covers the request as this engine serializes it: Go writes a union's content
before its tag and escapes `<`, `>` and `&`, so a digest or replay identity
computed by the Rust engine for the same request does not match and is answered
with Conflict. Decoding accepts either order.

The ledger keeps `credential-v1:<BLAKE3 hex>` of the serialized request instead
of its body for every durable command whose payload can hold a secret
(`admission.DigestOnly`): `put_credential`, `save_provider`,
`complete_provider_login`, `import_session`, `save_ssh`, `save_database`,
`save_search_settings`, `save_plugin_settings`, `save_plugin_mcp`, `install_mcp`,
`start_connector_login`, `submit_connector_input`, `update_connection`,
`save_backup_settings`, `restore_backup`, `restore_backup_file`, `dispatch` and
`plugin_transaction`. The set is every durable command that reaches a
`protocol.Secret`, directly or through a command carried by a schedule
callback, plus `save_plugin_mcp`, whose environment and headers are free-form;
`TestCommandLogClassification` derives it from the protocol source and fails
for a command stored raw. Replay, Conflict, OutcomeUnknown after a crash and
`InspectRequest` compare that digest and behave as for a raw body. Results are
stored unchanged; no output carries a secret. A turn's approved `save_database`
or `save_ssh` is kept on its approval (`agent_approvals.prepared`) without the
password or credential, which is what proves that the turn created a
connection.

A database written before this rule holds such requests raw. Startup digests
them through the in-place update: the rehearsal runs on a copy, the
pre-update backup is digested too and its write-ahead log folded, and the live
rows are rewritten with `PRAGMA secure_delete` on, which overwrites the freed
content with zeros; the update is refused with "existing data was not changed"
when secure deletion is unavailable. The write-ahead log is then checkpointed
and truncated. The update is idempotent and the contracts stay v1. What it
cannot reach: backups, exports and file copies made earlier (including
`node-before-upgrade-*` backups of earlier updates), the untouched files of a
Rust release, and blocks the filesystem or an SSD has already released or
remapped. Connections saved by a turn before the update no longer count as
created by that turn.

### Unobserved outcomes

A controller that did not see the result of a durable request (the reply was
lost, or the Node answered a duplicate with OutcomeUnknown) has one way to
resolve it, `Client.Settle` in `internal/client/resolve.go`. It executes the
request once and, on an unknown outcome, asks the Node by request ID:

| Node answers | Settle returns | The caller |
|---|---|---|
| `completed`, success | the stored output, no error | treats it as the success it is |
| `completed`, fault | the stored fault | reports the failure |
| `completed` with an OutcomeUnknown fault, or `unknown` | OutcomeUnknown, `client.Released` | drops the request, says once that the result could not be confirmed, starts a new request next time |
| `not_admitted` | Unavailable "did not reach the Node", `client.Released` | drops the request, reports a failure; nothing changed |
| `admitted`, or the Node cannot be asked | OutcomeUnknown, `client.Retained` | keeps the request; the same request settled again resolves it |

The Desktop's `Runtime.execute` uses it for every command it sends that way
(sends, queue actions, settings saves, provider and package commands, local
model installations, plugin status actions) and keeps a request only while
`client.Retained` holds. A workspace action offers "Check result", and a
transfer shows as uncertain, only for such a kept request; a released one is
toasted once as uncertain and its next attempt is a new request. Flows that
keep their own request (Host installation, attachment and file uploads,
package installations, SSH transfers, a card's sign-in, a package's worktree
handoff, the Retry of an approval or an answer) ask with `Client.Resolve` and
read the answer with `Resolution.Result`, which is what `Settle` returns: a
stored failure or an outcome the Node can never tell ends the request, so no
flow asks about the same request forever.

The one request kept although its outcome is final in the ledger is a
forwarded session creation ([Session forwarding](#session-forwarding)): the
Node that forwarded it resolves it only when the same request is sent again,
so the Desktop keeps it with its draft until the target answers.

The Mobile adapter's `execute` is `Client.Settle`, and its `resolve` (the
phone's "check result") is `Client.Resolve`, which never sends the request.
Both mark a failure that ends its request with `released`; see
[Mobile](mobile.md#uncertain-outcomes).

Differences from Rust, deliberate:

- Rust has no final state: an `admitted` request whose result write failed
  stays admitted until restart, and an `unknown` one answers OutcomeUnknown to
  every retry, which the Desktop kept sending. Go retries the result write,
  stores the failure of a request that was not applied, and releases a request
  the Node can never answer.
- Rust never deletes a ledger row. Go deletes settled, unreferenced rows after
  7 days.
- Rust confirms an unobserved settings save in the Desktop
  (`settings/requests.rs`); Go does it in the Client for every command.

Protocol and stored JSON is decoded in one pass by `protocol.Decoder`
(`encoding/json/jsontext` tokens, per-type member tables built once). The wire
and storage format and the accepted inputs are unchanged. A raw JSON value kept
from inside a decoded object is still stored compact with `<`, `>`, `&`, U+2028
and U+2029 escaped, so digests computed over retained values do not change.

### Values from another build

The rule is in [ARCHITECTURE.md](../ARCHITECTURE.md#contract-and-storage-versions).
A decode is exact unless the caller names its source with `protocol.DecodeAs`:

| Level | Caller | Unknown member | Unknown enum value | Unknown variant |
| --- | --- | --- | --- | --- |
| `Authored` | every plain `json.Unmarshal`, the default | refused where the type refuses it | refused | refused |
| `PeerCommand` | `wire.Reader` for an `Envelope` | ignored | refused | refused |
| `PeerData` | `wire.Reader` for a `Reply`, snapshot assembly, the Client's decode of a value a Node sent | ignored | kept | kept as `protocol.UnknownVariant` |

A missing optional member takes its declared default and a missing required
member is refused at every level.

- The level follows a value through nested types: the decoder lends the level
  to the exact bytes it hands a nested `UnmarshalJSON`, for the length of that
  call. Four shapes are not followed and stay exact, which is the behavior
  before this rule: a fixed-size array of protocol types, an interface-typed
  field, a struct with embedded fields, and a map whose key is neither a string
  nor text. An externally tagged union still admits exactly one member.
- An enum encodes any non-empty value, so a kept value survives a relay or an
  edit-and-save, and refuses the unset value. `MarshalUnion` writes an
  `UnknownVariant` back unchanged.
- Strictness stays, deliberately, for: commands (`PeerCommand`), a response
  that is neither `Ok` nor `Err`, pairing and rendezvous frames, package
  manifests (`plugins.ValidateManifest`), plugin transaction operations and
  script input, tool arguments, settings and stored rows, and required members.
- Consumers: `client.Projection` and the conversation reducer skip an event or
  frame change of an undeclared kind and advance its sequence; a draft part of
  an undeclared kind is valid and not drawn; `live` reads an undeclared queue
  pause reason as no halt and skips a resource part of an undeclared type; no
  controller reads a background task's `hold` yet; the file picker names a
  place of an undeclared kind by its path.

A build made before this rule cannot be changed. With the same protocol version
it fails to decode these values, all added on November 12, 2026:

| Value | Sent by | What the earlier build does |
| --- | --- | --- |
| `PluginExtension.continuation_markers`, `ToolHandler.approval` | Node, in any plugin list containing the bundled `goals` package | controller fails the plugin read |
| `ConversationBackgroundTask.hold` | Node | controller fails the conversation frame |
| `BackupSettings.keep`, `BackupSettings.last`, `BackupRecord.skipped` | Node | controller fails the backup read |
| `BackupSettings.keep` inside `save_backup_settings` | controller | Node refuses the save |
| `FaultDetails` kind `continuation` | Node | controller fails the reply instead of showing the fault |
| `FileBrowserLocationKind` `applications`, `FileBrowserLocation.medium` values | Node | controller fails the location list |
| a `ConversationQueue.pause_reason` value added later | Node | controller fails the frame; the member itself is ignored |
| `PluginTransactionOperation` `continue` members `marker`, `limit`, `user` | a package | Node refuses the operation; a package needing them sets `min_sailry_version` |
| any Node event or conversation frame change of a new kind | Node | controller faults its projection |

An earlier build already accepts `ConversationContinuation.kind`, `limit` and
`user`, `KnowledgeProgress.failed` and `failures`, `KnowledgeFailure`, the
`output_limit` resource part and `CommandContinueTurnPayload`. The protocol
version is unchanged: from this build on, each of the values above decodes.

The Flutter controller has no decoder of its own. Everything a Node sends is
decoded by the Go Client inside the adapter, at `PeerData`, and reaches Dart
as JSON that Dart reads as plain maps by key: an unknown member is ignored and
an unknown enum value or variant falls to a default branch. So a newer Node
does not break the phone at decode; what the phone lacks for the values above
is display only ([Mobile](mobile.md#known-display-gaps)). The one exact decode
is `Authored`, of the commands Dart builds (`prepare` in
`internal/mobile/connection.go`); Dart and that decoder ship in one build.

Each database mutation uses a command savepoint. Business failure rolls back its
partial SQL while the completed fault remains durable. Successful business SQL,
outcome and event records commit together. Filesystem preparation occurs outside
the SQL callback, after durable admission, and is never retried by the writer.
An unconfirmed completion is reported as OutcomeUnknown.

Admission never waits for a worker. Admitted jobs that prepare outside the SQL
callback run in order on one of two lanes: project effects (create, update,
register a worktree, remove), which may clone a repository, and all other such
commands (turns, provider commands). A slow clone therefore delays neither
reads, nor admission, nor a turn. Background tasks that outlive their request
(sign-in, connector sign-in, plugin commands) each run on their own goroutine;
the Rust limit of 64 shared tasks, which answered Busy to unrelated work once
one source filled it, is not kept. The supervisor only waits for them at Stop.

## Current commands

The Node provides snapshots and request inspection; local project creation,
registration, update and removal; existing Git worktree registration; defaults;
explicit session creation, reading, rename, archive, attention, removal, manual
order and change of worktree; notification publish/read/dismiss and controller
diagnostics; and power, telemetry and media role settings storage.

Project creation registers its main worktree in the same transaction.
`register_project` is the same creation from a name and a path, with the default
appearance. Duplicate paths conflict. Registering a removed project restores its identity. Project
updates compare the complete expected Project. Removal hides the project while
retaining its directory and session history. Worktree registration verifies that
the canonical directory belongs to the project's Git repository.

Projectless sessions reserve their own Node workspace without creating files.
Creation requires an explicit configuration or configured Node defaults. Session
configuration revisions are immutable; rename and archive create a new revision.
Attention has its own revision. Batches apply as one command transaction. Removal
retains immutable history and refuses active tasks. Project-scoped ordering is
stored in the committed event log and remains separate from controller pins.

Removal reclaims what only removed sessions can reach, in the removing
transaction: artifacts with no revision still published in a remaining
conversation, and the checkpoint text and attachment bindings of turns that a
removed session owns and no remaining session lists. A fork shares its source's
turns, so they keep their checkpoints and attachments until the last fork that
lists them is removed. An attachment row goes with its last binding; an upload
that no turn bound stays with its composer. After the commit the Node deletes
the artifact directories of removed sessions that keep no artifact and collects
the attachment files left without a row. Archiving and rewinding reclaim nothing:
a rewound turn stays restorable until its session is removed. Canonical records
and their blobs are kept.

`move_conversation` and `fork_conversation_at` share one target rule: the
conversation is a root conversation at the expected revision, it belongs to a
project, the worktree belongs to that project and is not removed, no turn is
queued, running or stopping, and the worktree directory still exists. A move to
the current worktree answers with the unchanged session and no event; any other
move creates a new session revision.

Notifications retain typed details and the newest 200 sequence entries. Plugin
publication requires an installed, enabled, compatible package and an exact
package reference. Telemetry saves preserve installation identity and require a
controller. These settings commands perform storage only; they start no native
wake assertion or telemetry sender.

`report_diagnostic` stores a controller's system report as a notification.
Plugin callers are refused, a cancellation stores nothing, and a repeated
incident answers with its first notice without another event. The Desktop
sends it when a package's metadata read (`plugin_metadata`, operation
`metadata_read`) or a view's initialization (`plugin_initialization`,
operation `view_initialize`) fails (`live/plugin_diagnostics.go`): once per
failure the view shows, not for a cancellation or while disconnected, and
its result is ignored. The package is named only when the snapshot gives its
version.

The Node writes its own notices in the transaction of the change they report
(`internal/node/store/notifications_system.go`), as Rust's
`store/notifications/system.rs`:

- A finished run of a main session writes one `task` notice: success for
  `completed`, error for `failed` and `interrupted`, with the session's title,
  the run's times and its fault code. `FinishTurn` writes it, so restart
  recovery reports the runs it interrupts. A `cancelled` run is one its user
  stopped and has no notice; a delegated run reports to its parent and has
  none. A failure is not reported twice: a compaction failure that stopped the
  turn, or a `system` diagnostic of the same incident and turn, is its notice.
- A conversation entry the system writes with a compaction failure writes one
  `diagnostic` notice naming that entry. The store writes it for any such
  entry; the Go engine does not record compaction failures as entries yet (a
  failed compaction fails its run, which the `task` notice reports), so this
  notice has no producer in the engine today.
- A turn that a dispatch job started and that begins to wait for an approval
  or an answer writes one `task` notice whose status is still `running`:
  warning for an approval, info for input. A scheduled run usually has no
  connected controller to derive that attention, so the Node keeps it. The
  notice has no finish time while the wait is open and gets one when the wait
  ends; each later wait of the turn is a new notice. An answer or a stop marks
  it read. A request that expired (the engine cancels an approval, and times
  out a question other than plan acceptance, after 300 seconds) or that an
  interruption closed stays unread, so its user finds it on return. Turns a
  controller started have none: the controller derives their attention
  itself, and it skips that derivation while the Node's notice is open, so a
  wait is told once. Rust has no such notice.
- A dispatch job that ends `failed` or `unknown` writes one `system`
  diagnostic (`plugin_callback`, incident = the job's request, with the
  package and handler), including a job refused when it is claimed. A job
  whose callback admitted a turn is reported by that turn's `task` notice
  instead, and a cancelled or skipped job has none. Rust has no such notice.

`read_media_settings`, `list_media_models` and `save_media_settings` store which
configured provider model serves the vision, image, video and speech purposes,
and `synthesize_speech` reads text aloud with the assigned speech model. The
settings appear in the snapshot and are captured with each admitted turn.
Capability tags, routing, channels and limits are in [media.md](media.md).

`save_network_settings` checks the expected revision, makes the selection the
Link's effective one and only then stores it, so a selection whose relays
could not be reached changes nothing and is not the one the next start uses.
A selection that was applied but could not be stored is taken back; when the
commit itself could not be confirmed the effective selection stays. Rust
stores the selection before applying it and keeps it when the relays fail
(`store/network.rs`); this order is the deliberate difference.

`count_feature {feature}` counts one use of a desktop feature in the usage
window and answers `feature_counted`. It is a v1 extension, not durable,
accepted only from the controller on the Node's own device and only for an
identifier of the closed list in [settings.md](settings.md#usage-tables).

`list_associated_sessions` pages the sessions bound to one association, newest
first, with a limit of 1 to 100 and a `before` cursor that must belong to the
same association. A plugin scope with a worktree confines the page to it, and a
plugin may list only its own namespace from a scope without a session or turn.

### Session forwarding

`create_session_at` whose target is another Node creates the session there with
this Node's configuration. Only the local controller may forward; any other
caller is refused with `permission_denied`. `store.PrepareSessionForward` checks
that the listed provider still has the expected revision, validates the
selection, and packages the provider, the selected credential's secret and
expiry, the captured roles and each role provider's secret into `import_session`
under the same request ID. The explicit credential selection is authoritative: a
configuration without a credential forwards no secret. API keys and Copilot
grants are transferable. Any other sign-in is refused with `not_configured`
"Claude sign-in is required on the execution Node" or "ChatGPT sign-in is
required on the execution Node".

The request is admitted durably first; its ledger body holds no secret. A
refusal, a Node without a Link, or more than 128 forwards in flight completes
the admission with its fault (`busy` "Node operation queue is unavailable" for
the last two). The transfer then runs on its own goroutine, outside both command
lanes, for at most 30 seconds. An unpaired target answers `not_found` "target
Node is not paired". Once dispatch begins, a failed dispatch, a lost completion,
the limit or Node shutdown records `outcome_unknown` "remote session creation
could not be confirmed; send the same request again to resolve it".

Replay of any other recorded outcome returns it and never sends again. Replay of
an unknown outcome, recorded or left by an interrupted Node, packages the import
again and sends it under the same request ID. The target admits by that ID, so
it answers an import it already holds from its own ledger and creates the
session only if the earlier one never arrived; the controller receives that
answer. The outcome stays unknown while the forward is still in flight, when the
provider, its secret or the roles changed so the import cannot be rebuilt or the
target answers `conflict` for different content, and when the target cannot be
reached or is no longer paired: only the target's own answer settles it. That
answer, a session or a refusal, replaces the unknown outcome in the source
ledger (`admission.Resolve`), so a later replay and `inspect_request` read it
there and nothing is sent again. The Desktop keeps the request of an unknown
creation with its draft and sends it again with the next Send.

`import_session` is accepted only from the paired caller named as its source,
never from the local controller. The bundle must be consistent: the
configuration names the bundled provider and credential, the credential belongs
to the caller, and a secret is present exactly when a credential is. The
provider and every role provider are stored unlisted under new provider and
credential identities, the configuration and role models are rewritten to them,
and the session revision records a profile with the source Node and the captured
provider. A plugin assistant binding requires the same package digest installed
here. Turns of such a session run on the profile's provider and keep the
forwarded roles without refreshing them. The ledger keeps only the digest of
the request, so the secret is stored only in the private provider row. The
operation log names the command, the
request and a worktree identifier only.

Removing a forwarded session deletes its private providers and their secrets
once no remaining session revision refers to them.

`use_external_browser` drives the session's managed browser on this Node. It
uses only a Chrome, Chromium or Edge installed on the machine and answers
`not_configured` without one; its checks, bounds and session lifetime are in
[browser-external.md](browser-external.md).

One protocol command has no route and answers `not_configured` "command is
unavailable": `use_media` is an agent resource effect that runs only as a turn
tool.

| Deviation from Rust | Reason |
|---|---|
| `create_session_at` whose target is this Node still creates the session locally | The Desktop creates every session through it; Rust refuses a local target |
| A replayed forward with an unknown outcome sends the identical import again, and the target's answer is stored in place of the unknown outcome | Rust records the unknown outcome as final and its replay never sends, so a draft could not create its session after one lost answer; the target deduplicates on the request ID, which makes the resend safe |
| Private providers are deleted with their session | Rust retains them; a removed session should not leave a forwarded secret behind |
| Removal deletes the checkpoint text and bound attachments that only removed sessions reach | Rust prunes artifacts only and never reclaims checkpoints or attachments; a deleted conversation should not keep file contents and uploads of up to 64 MiB each |
| `save_backup_settings`, `save_search_settings`, `restore_backup`, `restore_backup_file`, `dispatch` and `plugin_transaction` are kept by digest | Rust stores them raw although they can carry a secret; the other digested commands match Rust |
| File and command bodies are stored raw | Rust also keeps `file-v1:` and `command-v1:` digests for file writes and command executions; they hold no credential field |

## Observation

The Node subscription registers for changes before reading its initial snapshot.
All snapshot fields and its cursor come from one pinned read transaction.
Subsequent payloads come from the committed event log in cursor order, in pages
of at most 128 events and approximately 2 MiB. Wake hints can coalesce without
losing events. A missing cursor or unreadable continuation produces
ResetRequired; Client owns fresh-snapshot recovery and never replays commands.

A snapshot reply is a header followed by pages of at most 1 MiB. Each frame
is written under its own Link deadline, and a receiver rejects a header that
declares more than 256 MiB (sixteen maximum frames) before allocating. A
frame's buffer grows with the bytes that arrive, in 64 KiB steps, so a declared
length alone reserves no more than one step. The
Client has one observation owner, which reconnects after a transient loss with a
250 ms to 5 s backoff and ends only on a permanent fault. The first observation
is retried the same way: `Client.Start` returns the first attempt's failure, and
after Unavailable or Busy the owner keeps trying until the Client is closed, so
a controller opened while it or the Node is offline connects when the route
returns. The Desktop's remote peers close such a Client and inspect the peer
again every 10 s, so only one of the two retries runs. `Refresh` keeps the
existing observation when the new subscription cannot be opened; when the new
subscription's first read fails, the Client is disconnected and the same
reconnection continues. `Client.Revision` advances with each change of the Node
projection, so a consumer copies one `Snapshot` per change.

Only the Node topic is available in this stage. Conversation execution, provider
configuration, plugin invocation authority, terminals and remote services remain
with their owning backend phases. Connection and plugin assistant session
creation is unavailable until those services land. Source-compatible captures
and projections extend in those owning phases without adding another history.

## Shutdown

`Node.Stop` refuses new work, cancels every worker and closes the Node in
dependency order: command lanes, plugin status, agent turns, logins, terminals,
network, port forwards, supervised tasks, the knowledge index, local models,
external browsers, plugins and MCP servers, requests in flight, telemetry,
commands with transfers and watchers, then storage and the profile lock.

The whole sequence has the Rust Node's five-second deadline. Supervised tasks
(logins, plugin commands) may finish until that deadline and are cancelled at
it. When the deadline passes, `Stop` logs the stage that did not stop and the
stages it never reached, closes each database that no read or write still
uses, releases the profile lock and returns `core.ErrShutdownTimeout` naming
the stage. The caller exits; the Desktop updater continues with the
installation, because the profile is released.

| Deviation from Rust | Reason |
|---|---|
| The deadline covers the whole shutdown, not only supervised tasks | Rust aborts its tasks at the deadline and then closes the rest; a goroutine cannot be aborted, so every wait needs the bound or one worker that ignores cancellation keeps a windowless process holding the profile lock |
| Stages after the one that did not stop are not run | They would wait for the same worker. What they would have released is what a crash leaves, and the next start clears it: terminals end with their pseudo-terminal, recorded command groups are killed, external browsers are closed, temporary files are removed |
| A database still in use at the deadline is left open | Closing it would wait for the worker; SQLite recovers its log at the next start |
| No collection starts after the deadline | The abandoned sequence goes on in its goroutine if the stage ends before the process does. A collection of package versions, attachments or artifacts deletes the files no row names, and the released profile may already have its next owner, so `Node.abandoned` stops each of them; the next owner's own collections do the work |

### Leftovers of an earlier run

`OpenProfile` removes them right after it takes the profile lock and before
the identity, the databases or any component is opened (`sweepProfile`), so
nothing it removes is in use by this process and no other process owns the
profile:

- everything in `tmp`: database copies and staged files of a backup, the
  unpacked staging of a restore, sealed archives, staged inputs, command
  scratch and retained output, transfer and browser scratch. The one exception
  is the `command-group-*` records, which the process service reads at its
  start to kill the command groups a crashed Node left and then removes
- `storage/.upgrade-*`, `storage/.schema-reference-*` and
  `storage/backups/.upgrade-*`, the scratch of a schema check or update. The
  kept pre-update copies (`storage/backups/node-before-upgrade-*`) stay
- `plugins/packages/.staging-*` and `python/staging/environment-*`, a plugin
  package or Python environment whose installation ended before it was
  published. Published packages and environments stay

A link is removed, never followed; a read-only tree is made writable first. A
leftover that cannot be removed is logged and does not stop the start.

| Deviation from Rust | Reason |
|---|---|
| The scratch space is cleared at every start | Rust removes nothing (`runtime-core/src/profile.rs`), so a backup or restore that was killed leaves plain copies of the database, stored credentials included, until someone deletes them by hand |
| Not swept: `.sailry-restore-<uuid>` beside a restore target, `desktop/updater/download-*` | The first lies outside the profile at a path the Node no longer knows; the second belongs to the Desktop updater |

## Store

`internal/node/store` has one writer goroutine that groups waiting jobs into one
transaction, each under its own savepoint, and a pool of read connections that
keeps up to eight idle.

- A job whose callback fails keeps its own fault and rolls back to its savepoint.
  When SQLite ends the transaction itself (a statement interrupted by its
  caller's cancellation, or one that cannot complete), nothing in the batch was
  committed: the interrupted job receives its cancellation, every other job of
  the batch fails with the retryable `busy` fault "storage transaction was rolled
  back; retry the operation", and the writer continues with the next batch. Jobs
  are not replayed, because a callback may have changed state outside SQL.
- A `COMMIT` that fails is decided by whether the connection still holds a
  transaction, not by the error of the `ROLLBACK` that follows. SQLite rolls
  the transaction back itself when a commit cannot be written (a full disk, an
  I/O error) and the `ROLLBACK` then only reports that none is active; a
  refused commit that leaves the transaction open is rolled back. Both are known
  to be uncommitted and answer the retryable `busy` fault; a full disk answers
  `busy` "storage is full; free disk space and retry the operation", also when
  a write statement meets it. Only when the connection can neither roll back
  nor begin does the batch answer `outcome_unknown`. The writer then closes
  that connection, which discards any transaction it held, and opens another
  that must take the write lock; later batches run on it. While no connection
  can be opened each batch answers `unavailable` and tries again, so no state
  needs a restart.
- Startup accepts an empty database or owned storage at generation 1. Any other
  state is refused with "existing data was not changed" and left untouched; no
  path removes or resets a database. A database that was just created, updated
  or restored is checked table by table with `PRAGMA foreign_key_check` and
  refused with an orphaned row. A start on a current database does not repeat
  the scan: every connection enforces foreign keys, so the scan could only find
  a row written behind them, it would lock the user out for it, and it reads
  every child row (measured: 0.38 s per million rows with the file cached),
  which grows with history.
- File checkpoints draw their row number from the `file_checkpoints` counter. A
  profile written before this rule has checkpoints above its counter; startup
  raises the counter through the in-place update (rehearsal, backup, apply).
  The same update digests secret-bearing ledger rows stored raw
  ([Admission and outcomes](#admission-and-outcomes)).
- A plugin command that names a turn (`stop_turn`, `start_queued_turn`,
  `edit_queued_turn`, `remove_queued_turn`, `withdraw_guidance`,
  `send_queued_turn`, `read_queued_turn`, `run_command`) must name a turn of the
  plugin's captured session that the session did not inherit from another one.
  `read_tool_output` must name the plugin's captured turn, and so must
  `run_command` when the plugin has one.
- `start_queued_turn` starts a queued turn, or retries one that failed, was
  cancelled or was interrupted before its input reached the conversation. A
  running or stopping turn answers `conflict` "turn input has already been
  consumed": its pending input row stays until the input is recorded, so the row
  alone never makes a turn restartable. The Agent Runtime keeps one owner per
  turn; a claim that finds an earlier owner waits for it and runs only when the
  turn is still running afterwards, so an input is never sent twice.
- A turn loads its history once. Each later step decodes only the records
  appended since the previous step (`ExtendAgentState`); history grows only
  within one `history_revision`, and a rewind forces a complete load. Loaded
  records are shared between steps and are read-only. Tool discovery reads the
  materialized context without the history.
- Backups made before an update (`storage/backups/node-before-upgrade-*`) are
  never pruned, as in the Rust engine.

Storage uses the pinned SQLite fallback and the complete native SQLite file
family. There is no profile importer or direct cross-engine opening: the
database files of a Rust release (`node.turso`, `knowledge.turso`, the older
`node.db` and `node.sqlite3`, and their side files) are moved, unopened and
unchanged, into `storage/rust-archive/` at the first start, and the data in them
does not appear. The set found at one start stays together: every target is
checked before the first file moves. When the archive already holds one of the
names, because a Rust build ran in the profile again and created a new
`node.turso`, the whole set goes to the next numbered sibling that holds none
(`storage/rust-archive-2/`, `-3`, ...). Nothing is replaced or deleted and the
Node never refuses to start over an archive. Platform compilation and runtime
acceptance are distinct.
