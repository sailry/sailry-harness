# Plugin packages and execution

The execution Node owns installed packages, settings, private storage, callbacks,
connectors, MCP and installation outcomes. Controllers use the shared protocol;
the live desktop adapters project those resources without importing preview data.
Plugin-owned desktop surfaces remain assigned to the UI compatibility layer.

`internal/node/plugins` validates Agent Plugins manifests and the Sailry v1
extension. Packages are captured before publication and stored by BLAKE3 digest
under `plugins/packages`. Resource readers verify the complete captured content.
Installation stages files before the existing Node writer atomically publishes
inventory and its durable result. A failed update preserves the previous
inventory, configuration and enable choice.

Bundled defaults are initialized once. Startup refreshes an existing bundled
package only for strictly newer SemVer precedence and respects removal records.
Same versions, build metadata changes and older versions do not replace an
installed package. Application version 0.1.5 matches the reviewed Desktop/Host
source and pinned packages; the protocol remains v1.

Repository discovery freezes a Git commit before inspecting or installing files.
Catalogs, signed registry skills and authenticated one-use uploads share package
validation. Local sources stay inside their captured worktree and outside the
profile. Symlinks, traversal, conflicting archive paths and oversized captures
are rejected. Installed packages are independent of their source checkout.

Settings preserve public values and explicit keep/replace/clear secret updates.
Immutable settings revisions capture declared destinations. Credentials are absent
from public snapshots and settings history. HTTP and MCP inject them only into
declared origins, servers and slots, without forwarding across redirects.

Private KV writes check key revisions. Indexed values update KV and the SQLite
fallback's FTS5 corpus in the same transaction. Conversation values follow
canonical branch history, fork and rewind. Transactions admitting a turn anchor
preceding state writes to that turn.

The task supervisor bounds Node work. The installation queue permits eight
pending preparations and serializes each resource through durable publication.
Caller-scoped topics expose progress, retain failures or uncertainty until
dismissal and remove successes. Disconnection does not cancel admitted work.

Callbacks use `modernc.org/quickjs` and unchanged SDK resources. Each invocation
captures package/settings/resource authority. SDK commands return through ordinary
Node admission, action checks, writer transactions and receipts. Read-only
callbacks cannot admit durable effects. Tools retain the turn's exact packages,
selected declarations, approvals, mode and role.

JavaScript allocation is bounded at 64 MiB. Calls have a 60-second deadline and
honor cancellation. `SetMaxStackSize` uses a fixed 512-slot constant as the
Go-binding counterpart of Rust `STACK_BYTES`, without byte equivalence. Infinite
recursion returns `InvalidRequest` with
`plugin JavaScript execution failed: stack overflow`; the host remains alive.
Callback and translation paths share the engine and error mapping.

MCP keys capture package digest, settings revision, server, launch and
authorization identity. Changed captures retire old owners. Retained leases
check current authorization, credentials and destination after acquiring their
call locks and before each RPC. Connection, tool, descriptor and message sizes are bounded. The
owner retains subprocess/Python resources until cleanup settles. Lost transport
or unconfirmed cleanup returns `OutcomeUnknown`, without replay. Form and URL
elicitation use canonical questions and pause only active elicitation time.

Package behavior stays in the pinned plugin repository's tooling, which can use
the production QuickJS test host. Core tests cover host boundaries and admission;
tooling results do not substitute interactive account or platform acceptance.
