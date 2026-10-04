# Reminders package

This ordinary installable package owns reminder validation, to-do state,
scheduling policy and notification content in JavaScript. It consumes the same
public Node SDK locally and over Link, without a desktop view or model.

The package includes a host-scoped Kit desktop page for creating, editing,
completing and deleting reminders. Its drafts use native input and date-time
controls through `sailry/forms`; closing a view releases only presentation state.
Fresh Node profiles install this as an ordinary default package. Updates,
disabling and removal use the same inventory commands as any other package;
restarting never reinstalls a removed package. No native reminder handler,
protocol variant or page remains in the application.

The current v1 profile schema records initial distribution in `node.plugin_defaults`.
Incompatible development profiles are rejected without conversion or deletion.
Private data from the former native package is not imported automatically.

The page observes coalesced invalidation from the existing Client projection
with `nextChange`. This sends no resource data and can include unrelated Node
changes. Reads still use the package-scoped KV API, not a copied session or
subscription state machine. Unknown saves retain their original request ID;
closing the draft does not cancel or replay Node work.

## Callbacks

Use `call_plugin` with the installed package context. `list`, `save` and `remove`
return `{Ok: value}` or `{Err: {code, message}}` inside `plugin_result`. Retain the
original request ID for uncertain outcomes. A new request is a new operation.

- `list({after?})`: at most 32 records and a continuation key
- `save({id?, revision, title, message, due_ms, completed})`: revision is a decimal
  string; `"0"` creates an item. An omitted ID uses the original invocation ID
- `remove({id, revision})`: removes the item and its dispatch configuration
- `notify({id, revision})`: registered as a scheduled handler; verifies the
  current revision, completion, due time and prior delivery before publishing

New scheduled deliveries follow the installed package version. Queued callbacks
retain their exact package and settings through updates and restarts. Disabling
or removing the package prevents new execution; already admitted calls finish
under their original scope. No desktop view is required for delivery.

The private record and schedule changes commit atomically. Delivery marks the
record and publishes the notification in one transaction; a stale callback
cannot publish another notice. Node persists schedules, receipts and generic
KV data. There is no plugin-specific table or second delivery ledger.

`prepareTransaction(operations)` supports up to 64 operations and 256 KiB of
serialized data. Operations are `write`, `remove`, `dispatch` mutations and
`notify`. Each operation requires its ordinary grant and captured resource
scope. Package identity comes from the host, not an operation argument.
Writable revisions should be decimal strings at the SDK boundary. A failure
rolls back the entire transaction and emits no events. Successful events use
the existing ordered Node stream after the transaction commits.

Only Node-owned database changes participate. Files, processes, HTTP, model
calls, arbitrary SQL and nested transactions are not transactional operations.
Do not treat several separate callback requests as one atomic workflow.
