# Scheduled tasks

This ordinary external package owns task definitions and scheduling rules. Its
headless handlers use private KV and bounded host transactions to register
schedules, event handlers, and concurrency groups. The callback invokes the
public `sessions.start` capability, which atomically creates a conversation and
admits its first turn through the existing Node command ledger and Agent queue.

`completion: "turn"` holds the dispatch slot through model execution, tools and
approvals. Closing the plugin page does not stop execution. Removing a task
cancels its unstarted deliveries, not running conversations. History reads the
original admission receipt; unknown effects are never automatically replayed.

Handlers: `list`, `save`, `remove`, `run`, `history`, `queue`, `cancel`, `stop`, `projects`.
Task and queue revisions are decimal strings. New task IDs may be omitted; the
host invocation ID provides a stable identity. A null configuration resolves
Node defaults at execution admission. Project catalog access is explicit and
does not expose conversations or private scratch worktrees.

The desktop entry owns its Kit page, forms and locale resources. Read-only views
share package queries with the headless entry; writes use durable host callbacks.
History opens the existing host conversation route. Stopping a run is confined to
the original turn admitted by this package's dispatch job.

New profiles install this as an ordinary default package. Existing profiles are
not migrated or rewritten, and restarting never reinstalls a removed package.
