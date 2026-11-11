# Declared authorization

The Node owns connections and provider-account authorization. Packages declare
workflows, endpoints, HTTP operations, credential slots and callback policy.
Core interprets declarations without dispatching by package name.

`internal/node/authorization` supplies bounded HTTP, PKCE, device/polling,
script workflow effects, token exchange, OIDC verification and issued grants.
Scripts use the bounded moejs translation engine. HTTP and redirects stay
within captured origins and cannot carry credentials to another destination.

Attempts belong to their initiating controller. Login topics expose that
controller's captured attempt. Start, input and cancellation use ordinary request
identities; prepared effects are released after their durable result commits.
Loopback/manual callbacks retain declared state and PKCE ownership.

OIDC checks issuer, signature, audience, subject, timestamps, nonce and authorized
party. JWKS stay on the declared issuer origin. Grants are validated against
their declared authentication kind. Renewal fences prevent silently repeating
uncertain exchanges. An OAuth connection with no refresh declaration or no
refresh token sends nothing: it answers `expired` "connector authorization
cannot be refreshed" and keeps no uncertain marker.

Public connection projections contain metadata, status and consent requirements.
Credentials stay in Node configuration storage and are absent from public events,
UI caches and MCP settings projections. HTTP/MCP resolve only declared
destinations. Provider account status stays separate from Sailry session usage.

Connections use the existing `plugin_connections`, `turn_connections`,
`oauth_hosts` and `oauth_registrations` storage. Metadata revisions are distinct
from authorization revisions, so ordinary label edits preserve turn authority.
Credential edits, disconnects and permission acceptance change that authority.
Interrupted persisted login attempts recover as disconnected and are not replayed.

Consent compares source, origins, scopes, client, flow, injection targets and
effective MCP launch targets. The Node captures current targets outside SQL,
projects the differences and verifies the capture again when the controller
accepts. HTTP and MCP reject credentials for an undeclared or changed target.

Local CLI declarations run fixed arguments with bounded output and minimal
environment. Manifest validation refuses a command that is not a bare lower-case
name, more than eight variables, the names `PATH`, `HOME`, `USERPROFILE`,
`APPDATA` and `LOCALAPPDATA`, placeholders other than `{account}` and bounds
outside 30 seconds or 64 KiB; the search path and home variables are always the
Node's own. Token reads verify the selected account before and after execution.
Their token cache is private to the Node and uses the declaration's bound.
The cache also retains the exact CLI authentication declaration and account
identity, so a changed source cannot reuse an earlier credential.
Generic OAuth, application exchange and MCP OAuth use the same Node lifecycle;
MCP OAuth uses the pinned SDK's discovery and authorization-code handler.

Retained MCP leases validate authorization immediately before a tool RPC, after
serial and exclusive admission. The live check resolves current package settings,
server permission, declared destinations and connector authority outside the RPC.
It compares the captured identity and injected credentials, including sealed
stdio tokens. Disabled or revoked accounts, changed credentials and withdrawn
server permission cannot use an existing lease. Credential acquisition and refresh
remain outside SQL.

Desktop projects live connector declarations, account state and MCP facts through
Client subscriptions. Installed package declarations are read in one pass at a
time; an update during a pass asks for one more pass, a failed read keeps the
package's shown declaration and each pass reports its first failure once. Its editor retains its opening revision and masked secret
slots. The `ConfigureMCP` action opens a declared settings form when present and
otherwise edits plugin MCP configuration through `ReadPluginMcp` and
`SavePluginMcp`; the row menu no longer calls it, as the reviewed menu has no
such item. Standalone
MCP definitions use `InstallMcp` for captured definition and secret-slot updates.
This declaration-specific command choice preserves the accepted editor layout.
Saving a server starts its check, and reconnect, probe, consent and deletion report
their actual result through toasts: a failure carries the Node's reason under its
title, and a conflict and an uncertain outcome keep their own wording.

## Server status and row actions

`read_plugin` reports each MCP server's availability and, once the pool holds a
connection for it, its runtime. Availability follows the reviewed implementation:
an incompatible package is `unavailable`, a disabled one `package_disabled`, a
withdrawn permission `server_disabled`, a missing required setting
`configuration_required`; a server bound to a connector by its injection or its
MCP OAuth method takes the connection's state (`connection_required`,
`connection_disabled`, `authorization_expired`, `consent_required`,
`unavailable`). A standalone definition, and a connector whose only method is a
secret with optional fields alone, needs no connection. The computer server
whose signed helper is absent is `unavailable` with a failed runtime carrying
that fault, since it cannot start. The helper is resolved from
`SAILRY_HELPERS_DIR` or the `helpers` directory of the installed application,
so a run from source without that variable reports the helper as missing.

Desktop shows that state, never a failure that did not happen:

| Server | Row status |
| --- | --- |
| not allowed | Server disabled |
| availability other than available | its problem: Plugin disabled, Configuration required, Not connected, Account disabled, Expired, Needs confirmation, Unavailable |
| available, no runtime | Not connected |
| checking or connecting | Connecting |
| connected | its tool count |
| failed | Connection failed |

The reason of a failed or unavailable server is the tooltip of its status and a
line of its details. The pool keeps no state for a server that never connected,
so Desktop keeps the reason of the last failed check for the package content and
settings it checked, until a check succeeds.

The row menu lists only the actions the row has now, and the button is disabled
when it has none:

- a server added by hand: Configure, Delete;
- an account: its edit action (Confirm, Bind, Connect, Reauthorize or
  Configure), Check while it is enabled without servers, Delete;
- a package's servers without an account: Connect when a server waits for the
  connection, otherwise Add credentials, only while the package declares a
  connector with a method and is enabled; either opens that connector's editor;
- Reconnect while a server is allowed, available and not connecting, and the
  account, if any, is enabled;
- Details while the service has servers.

Package settings stay in the plugin's own settings form on the Plugins page; the
Connectors row does not open it.

`check_plugin_mcp` is a durable command answered from its recorded result: it
connects outside SQL, lists the tools and stores the answer for the request. A
check changes nothing to recover, so Desktop sends a new request after an
uncertain one. Closing a kept connection whose endpoint is gone cannot be
confirmed and reports an uncertain outcome; the next check fails outright.
Transport errors are reported as `MCP server is unavailable`; the Node does not
forward the transport's own text. The application registration action runs the
declared registration workflow, matching the reviewed implementation rather than
the older coverage row's external-link description.

Synthetic tests cover native account admission and cancellation, provider renewal
ownership, connector metadata/credential separation, secret projection, consent
changes, retained MCP invalidation, interrupted attempt recovery and controller
callbacks. Live provider
accounts, external CLI installations and cross-platform native OAuth callbacks
remain manual acceptance boundaries; source fixtures do not claim those results.

`manage_mcp` lists MCP packages and installs, updates or removes standalone
`mcp-*` packages from a turn through `install_mcp` and `remove_plugin`. Its
results carry transport and credential slot names only, never values; an update
must keep the transport and slots and may clear named slots. Approval and Plan
behavior are in [Plugin packages](plugins.md#agent-management-tools).
