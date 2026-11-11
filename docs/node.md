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

Protocol and stored JSON is decoded in one pass by `protocol.Decoder`
(`encoding/json/jsontext` tokens, per-type member tables built once). The wire
and storage format and the accepted inputs are unchanged. A raw JSON value kept
from inside a decoded object is still stored compact with `<`, `>`, `&`, U+2028
and U+2029 escaped, so digests computed over retained values do not change.

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
incident answers with its first notice without another event.

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

`read_media_settings`, `list_media_models` and `save_media_settings` store which
configured provider model serves the vision, image, video and speech purposes,
and `synthesize_speech` reads text aloud with the assigned speech model. The
settings appear in the snapshot and are captured with each admitted turn.
Capability tags, routing, channels and limits are in [media.md](media.md).

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
could not be confirmed; automatic replay is disabled". Replay returns the
recorded outcome and never sends again; the controller inspects the target with
the same request ID.

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
| Private providers are deleted with their session | Rust retains them; a removed session should not leave a forwarded secret behind |
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
250 ms to 5 s backoff and ends only on a permanent fault. `Refresh` keeps the
existing observation when the new subscription cannot be opened; when the new
subscription's first read fails, the Client is disconnected and the same
reconnection continues. `Client.Revision` advances with each change of the Node
projection, so a consumer copies one `Snapshot` per change.

Only the Node topic is available in this stage. Conversation execution, provider
configuration, plugin invocation authority, terminals and remote services remain
with their owning backend phases. Connection and plugin assistant session
creation is unavailable until those services land. Source-compatible captures
and projections extend in those owning phases without adding another history.

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
  are not replayed, because a callback may have changed state outside SQL. The
  writer stops with OutcomeUnknown only when `COMMIT` fails and the rollback
  cannot confirm that nothing was written.
- Startup accepts an empty database or owned storage at generation 1. Any other
  state is refused with "existing data was not changed" and left untouched; no
  path removes or resets a database. Every start checks each owned table with
  `PRAGMA foreign_key_check` and refuses storage with an orphaned row.
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
does not appear. A name the archive already holds is never replaced: the Node
refuses to start and names the archive. Platform compilation and runtime acceptance are distinct.
