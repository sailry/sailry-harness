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
slots. Plugin MCP credentials use an existing declared settings form when present;
other plugin MCP configuration uses `ReadPluginMcp` and `SavePluginMcp`. Standalone
MCP definitions use `InstallMcp` for captured definition and secret-slot updates.
This declaration-specific command choice preserves the accepted editor layout.
Saving a server starts its check, and reconnect, probe, consent and deletion report
their actual result through toasts. The application registration action runs the
declared registration workflow, matching the reviewed implementation rather than
the older coverage row's external-link description.

Synthetic tests cover native account admission and cancellation, provider renewal
ownership, connector metadata/credential separation, secret projection, consent
changes, retained MCP invalidation, interrupted attempt recovery and controller
callbacks. Live provider
accounts, external CLI installations and cross-platform native OAuth callbacks
remain manual acceptance boundaries; source fixtures do not claim those results.
