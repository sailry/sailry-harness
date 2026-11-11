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
update and removal; existing Git worktree registration; defaults; explicit
session creation, reading, rename, archive, attention, removal and manual order;
notification publish/read/dismiss; and power and telemetry settings storage.

Project creation registers its main worktree in the same transaction. Duplicate
paths conflict. Registering a removed project restores its identity. Project
updates compare the complete expected Project. Removal hides the project while
retaining its directory and session history. Worktree registration verifies that
the canonical directory belongs to the project's Git repository.

Projectless sessions reserve their own Node workspace without creating files.
Creation requires an explicit configuration or configured Node defaults. Session
configuration revisions are immutable; rename and archive create a new revision.
Attention has its own revision. Batches apply as one command transaction. Removal
retains immutable history and refuses active tasks. Project-scoped ordering is
stored in the committed event log and remains separate from controller pins.

Notifications retain typed details and the newest 200 sequence entries. Plugin
publication requires an installed, enabled, compatible package and an exact
package reference. Telemetry saves preserve installation identity and require a
controller. These settings commands perform storage only; they start no native
wake assertion or telemetry sender.

## Observation

The Node subscription registers for changes before reading its initial snapshot.
All snapshot fields and its cursor come from one pinned read transaction.
Subsequent payloads come from the committed event log in cursor order, in pages
of at most 128 events and approximately 2 MiB. Wake hints can coalesce without
losing events. A missing cursor or unreadable continuation produces
ResetRequired; Client owns fresh-snapshot recovery and never replays commands.

A snapshot reply is a header followed by pages of at most 1 MiB. Each frame
is written under its own Link deadline, and a receiver rejects a header that
declares more than 256 MiB (sixteen maximum frames) before allocating. The
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
- A plugin command that names a turn (`stop_turn`, `start_queued_turn`,
  `edit_queued_turn`, `remove_queued_turn`, `withdraw_guidance`,
  `send_queued_turn`, `read_queued_turn`, `run_command`) must name a turn of the
  plugin's captured session that the session did not inherit from another one.
  `read_tool_output` must name the plugin's captured turn, and so must
  `run_command` when the plugin has one.
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
