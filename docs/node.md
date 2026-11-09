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
recorded outcome without admitting or replaying the original command.

Each database mutation uses a command savepoint. Business failure rolls back its
partial SQL while the completed fault remains durable. Successful business SQL,
outcome and event records commit together. Filesystem preparation occurs outside
the SQL callback, after durable admission, and is never retried by the writer.
An unconfirmed completion is reported as OutcomeUnknown.

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

Only the Node topic is available in this stage. Conversation execution, provider
configuration, plugin invocation authority, terminals and remote services remain
with their owning backend phases. Connection and plugin assistant session
creation is unavailable until those services land. Source-compatible captures
and projections extend in those owning phases without adding another history.

Storage uses the pinned SQLite fallback and the complete native SQLite file
family. Turso profiles are refused; there is no profile importer or direct
cross-engine opening. Platform compilation and runtime acceptance are distinct.
