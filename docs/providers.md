# Model providers

`internal/node/providers` owns native provider metadata, account grant headers,
endpoint policy, model discovery, the reference catalog, partner presets and
model scores.
`internal/node/agent/model` snapshots one admitted provider selection and exposes
a per-step Fantasy `LanguageModel`. The selected official SDK adapters preserve
request and stream behavior after the Fantasy provider trial found concrete
schema, optional usage and completed-response gaps. Sailry owns execution and
retry policy; no SDK or Fantasy agent loop runs.

## Ownership and storage

Provider settings and credentials share the existing `providers.body` encoding.
Each provider record contains its public provider configuration, listing state,
authorizations keyed by credential ID and optional quota snapshot. Credential
metadata includes its authentication, revision and expiry; the secret never enters
a provider event or controller snapshot. Native account authorizations also retain
their declaring package's authentication target in the existing `auth` field.
Reading a key is an explicit API-key-only settings command. Removing a provider
clears listing state and retains its authorizations and historical captures.

`store.PrepareProviderCommand` captures current provider, revision and credential
facts inside the SQL owner. `providers.Run` performs explicitly requested HTTP work
outside SQL. `store.ExecuteProviderCommand` checks the captured revision again,
applies returned metadata and returns events for publication after commit. This
final step also applies to non-durable metadata commands. A local read with no
network work uses `store.ReadProviderCommand`.

Each Node owns one partner `Directory`. Ordinary preset listing reads the saved
suggestion cache. An explicit partner refresh uses a 15-minute successful or
30-second failed cache; a failed refresh retains saved suggestions and reports
`partners_unavailable`. Public feed data cannot replace executable builtins or
request policy. Partner policy is frozen when the editable provider is saved;
`store.CaptureProvider` freezes native preset injection and policy for a fresh turn.
Inherited captures remain independent of later settings and feed changes.

Installed model provider presets join the same directory. Their saved bindings
capture package, settings, authentication flow and optional model adapter. Native
account aliases are selected by declared authentication, with ambiguous aliases
rejected. Core has no package-name authentication dispatch.

`node.provider_login` owns controller-scoped attempts and captured declarations.
Start, callback and cancel effects begin after their durable command result
commits. The shared authorization owner runs declared script, device, PKCE and
token exchange effects, including OIDC verification where declared. The controller
owns its loopback listener and browser actions. Login success attaches the new
credential and publishes metadata; account grants never become editable API keys.

Renewal runs under Node ownership and coalesces concurrent requests for one
credential. Every claimed account turn reaches the engine's `AuthorizeProvider`
boundary before immutable model capture. It uses the admitted provider and
credential reference, including an account that expired while queued, rather than
the provider's later selected account. SQL claims perform no HTTP. A stopped turn
drops its observer while the Node task retains ownership of the credential write.
A rotating exchange writes `renewal_pending` before HTTP; cancellation
or restart cannot silently replay an uncertain exchange. A plugin grant leaves
that state in two definite cases. When no request got past connection setup the
previous grant returns unmarked and a later use renews it again. When the
provider refused the grant (401, or an OAuth error such as `invalid_grant`) the
previous grant returns marked `renewal_rejected` and every later use answers
`permission_denied` until the user signs in again. Any other failure keeps the
uncertain marker. A changed client, scope,
flow, issuer or destination requires sign-in again, while an implementation-only
package update retains the same account owner. OAuth header and catalog overrides
are saved with the provider and explicit discovery captures.

## Native routes and metadata

Supported native routes are OpenAI Responses, OpenAI-compatible Chat Completions,
Anthropic Messages, Gemini, DeepSeek, OpenCode Go/Zen, Azure OpenAI and Azure AI
Foundry. Azure deployments require explicit model IDs. Foundry project lookup keeps
pagination on the configured origin and uses the source five-second lookup bound;
model generation has no added deadline. API-key headers and Azure API version
queries follow the captured provider. Provider endpoints permit HTTP and HTTPS
without embedded credentials, queries or fragments; redirects are refused.

Account generation is confined to the grant's authorized destination. ChatGPT
direct use requires its two plan scopes; Codex uses the captured catalog identity
and account header, Claude uses its OAuth transport, and Copilot uses its
authorized service endpoint. Native discovery preserves visible ChatGPT catalog
order, reported names, limits, reasoning and service tiers. Copilot catalogs filter
by the selected chat or Responses endpoint. Plugin adapters use their declared
discovery translation.

Declared provider HTTP resources capture package, provider revision, credential
revision, fixed endpoint and headers. Native account resources expose only their
allowed account origin; plugin resources use their captured flow. Read caching is
bounded to 32 entries and scoped to credential revision. Overlapping acquisitions
share a response, mutations are never cached, and protected grant values cannot
escape through the response.

Quota snapshots retain the exact enabled ChatGPT provider, credential and
credential revision. Changes that invalidate that owner remove the public
snapshot. Desktop clamps remaining share to zero and hides an observed window
once its reset time arrives. One controller timer invalidates at the next reset;
it performs no provider polling. The reviewed source's observation writer is
test-only, so this phase preserves stored-snapshot projection without inventing a
provider usage endpoint.

All HTTP clients use MyGo netproxy. Discovery keeps its 30-second total and
15-second request bounds, at most 20 pages, 2 MiB per page, 8 MiB total and 1,000
models. The models.dev reference refresh uses 30 seconds and 32 MiB. Model scores
use 30 seconds, 4 MiB and conditional ETags. Partner suggestions use 15 seconds and
1 MiB. These operations run no generation, validation or title model request.

Automatic capabilities come from provider metadata and the stored reference
catalog. A model saved with `custom_capabilities` keeps its saved `vision`,
`tools`, `reasoning` and `generates` tags through refresh and discovery
([media.md](media.md)). Refresh preserves configured token limits, custom effort choices and
compaction overrides. Catalog lookups merge shared IDs conservatively across
providers. Scores cover published model benchmarks and are separate from Sailry
session usage; no provider account history is collected.

## Model requests

Model capture copies provider, session configuration, secret and session identity.
The engine supplies a stable system prompt, canonical records, tool declarations,
original declaration JSON and any hydrated assets. `Request.ToolSchemas` preserves
schema key order across Fantasy's map-based declaration type. Provider request
layouts use concrete wire structs and raw schema fields; Chat adapts only the
source JSON-schema constructs while preserving property and definition order. Request projection applies visibility and the latest
summary, preserves explicit tool call/result identities and skips capability-only
or diagnostic content. Signed, redacted and encrypted reasoning stays within its
captured provider/model scope. Provider metadata conversion is owned by
`ApplyStreamMetadata`; `PartIndex` keeps canonical content slot identity.
Tool calls without a result and results without a call are omitted from the
request, for SDK and plugin transports alike.

Account credentials are authorized before every inference and before compaction.
A credential that is still valid is read locally; an expired one is renewed
through its provider flow, and the transport is rebuilt only when the secret
changed. API-key providers keep the secret captured with the turn.

Main transport attempts have at most five additional transient retries, 60 seconds
apart, before any output event escapes. Summary output stays buffered and has one
additional transient retry after five seconds. Authentication, quota, invalid
requests and malformed/truncated streams are terminal. Every attempted request
reports timing, optional usage and failure facts once through the engine observer.
Unknown token counts remain unknown; an explicitly reported zero remains known.
Summary requests retain provider/model/auth and use at most 8,000 output tokens or
the smaller advertised limit, with the lowest declared reasoning choice.

## Validation and current boundaries

The model tests cover semantic provider requests, stable prompt fields,
SDK cancellation and early release, optional usage, failure mapping and injected
clock retry policy. Their source fixture table includes captured synthetic OAuth
request formatting. The authorization tests exercise a captured package flow
through the Node's durable admission, fake fixed HTTP boundary, private attempt
subscription, credential completion, replay and cancellation. Store fixtures cover
renewal fencing, queued credential identity, metadata protection and quota
ownership. Account catalogs and
controller callback projection use deterministic synthetic fixtures. These checks
do not establish interactive acceptance with real provider accounts.
Native model IDs and adaptive thinking policy use the locally copied, pinned
rig-core capability catalog rather than a second table.
Imported Goose, rig-core and OMP notices remain beside the adaptations.

Independent HTTP socket captures from the reviewed Rust revision live under
`model/testdata/raw-rust`. The request table verifies each source hash and compares
actual SDK request bytes directly with those files, with separate route and
deterministic policy-header checks. OpenAI and Anthropic use their SDKs' public
raw request-body option. Google's SDK exposes map-based body customization, so a
scoped HTTP-client adapter supplies the original source-encoded body while the
SDK retains HTTP cancellation and stream decoding. A rejected encrypted-reasoning
carrier is terminal under the one-request-per-step boundary; the source
transport's corrective second HTTP request is not enabled.

Outgoing tool projection lives in `agent/output`. One execution-scoped cache
creates independent request records, bounds model-facing tool text to 2,000 lines
and 50 KiB, and retains literal call/revision references for `read_tool_output`.
Repeated visible file reads can reuse earlier complete ranges; visibility changes
or compaction membership invalidate that reuse. Numbered file/search/edit and
persisted-result page previews affect only the outgoing copy. Image and MCP
annotations limit assistant audiences before request serialization and compaction.

The provider editor requires a positive context limit within the v1 `uint32`
range and at least the model's output limit. It rejects an empty or oversized
field before narrowing values for the protocol, retaining the draft on failure.
