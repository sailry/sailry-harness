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

A session forwarded from a paired Node brings its provider with it
([node.md](node.md#session-forwarding)). That provider, and each provider its
roles use, is stored as an unlisted record under a new provider and credential
ID, so it cannot alias a provider configured here. It appears in no provider
list and cannot be edited; its credential metadata is listed like any other.
Only API keys and Copilot grants leave their Node; a Copilot grant renews on an
unlisted record, every other account kind requires a listed provider. The record
is deleted when its session is removed.

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
([media.md](media.md)). Requests follow the same rule: the saved `reasoning` tag
of such a model decides whether reasoning parameters are sent, and discovery's
reasoning capability decides only for models the user has not tagged. Refresh preserves configured token limits, custom effort choices and
compaction overrides. Catalog lookups merge shared IDs conservatively across
providers. Scores cover published model benchmarks and are separate from Sailry
session usage; no provider account history is collected.

Session and usage costs are reference prices from the stored models.dev catalog,
not billed amounts. A response is priced by the exact model identifier under the
vendor of the channel that ran it: the built-in preset the channel was created
from (the one captured with the run, else the channel as saved now), then that
preset's brand, then the vendor its wire protocol belongs to (OpenAI for
Responses and Chat Completions, Anthropic, Google). Seven names differ from
models.dev and are mapped in `store/agent_usage.go` (`gemini` to `google`, `qwen`
to `alibaba`, `moonshot` to `moonshotai`, `fireworks` to `fireworks-ai`,
`together` to `togetherai`, `zhipu` to `zhipuai`, `doubao` to `volcengine`). When
none of these lists the model, the identifier is priced from the other vendors
only if every one that states a price states the same one; resellers price one
identifier differently, so a disputed or unknown model stays unpriced. Partner
and package presets are not vendors. The Rust source prices by wire protocol
alone, which leaves every Chat Completions vendor other than OpenAI, and the
DeepSeek, OpenCode and Azure protocols, without a cost.

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

Media a tool returns is model input as media, never as base64 text.
`store.AppendCanonical` moves every binary field a new tool result declares
beside a MIME key into a blob and leaves a slot in the stored value. Reloading
the turn's history lifts the `inline_data` of a JSON result (at its root, or in
the `data` of a `computer`, `browser`, `external_browser` or `file_image`
output) out of the value into the in-memory result's attachments; an MCP result
keeps its image, audio and resource content. `BuildCall` then emits one tool
message: the result text followed by its media parts, which each wire renders
its own way:

| Wire | Tool media |
| --- | --- |
| Anthropic Messages | `image` blocks, and a PDF as a `document` block, inside the `tool_result` content |
| OpenAI Responses | `input_image` parts in the `function_call_output` content list |
| Chat Completions | The tool message ends with "This tool result included an image that is uploaded in the next message."; the images of one assistant turn follow its tool messages in a single user message |
| Gemini | `inlineData` parts inside the `functionResponse` for `gemini-3*` models, beside it for earlier ones |
| Plugin adapter | `image` content blocks in the result's MCP envelope, when the adapter declares vision |

Only a model with the vision capability receives images: the listed model's
tag, or the provider's default vision channel for a model the provider does not
list. Any other model reads "(tool image omitted: model does not support
images)" in the result text. Producers keep the reference's size rules (file
images 5 MiB and 1568 px, browser captures 1 MiB and 1568 px); media a
producer did not bound is refused above 5 MiB with "(tool image omitted:
larger than 5 MiB)". Media with an `audience` annotation that excludes the
assistant is not sent.

Differences from the reference, all deliberate:

- The reference lifts media in every outgoing projection; Go lifts once when a
  record is loaded, so history and token estimates never hold the base64.
- The reference sends a tool result's stored attachments. Go's only attachment
  is a generated image, which is shown and never sent back
  ([media.md](media.md)), so attachments a tool declares are not model input.
- The 5 MiB bound and its note, and the note that replaces audio or binary
  content a wire cannot carry, exist only here; the reference drops such
  content silently or lets the provider reject the request.
- Chat images are deferred until the tool messages of the assistant turn end,
  so several results never interleave user messages between tool messages.
- An MCP resource blob that is an image is sent as an image on every wire.
- The reference omits the older images of an MCP server with `keep_images`
  from each request, across the history; Go applies `keep_images` to each
  result when the tool runs, so earlier results keep their images.
- The reference sends an adapter every content block; Go adds lifted images
  only for an adapter that declares vision.

Account credentials are authorized before every inference and before compaction.
A credential that is still valid is read locally; an expired one is renewed
through its provider flow, and the transport is rebuilt only when the secret
changed. API-key providers keep the secret captured with the turn.

A native model request that sends nothing for 600 seconds, before its response
or between two reads of its stream, is cancelled and fails as a network error
"Model stream sent no data for 600 seconds", as the reference's read timeout
and the plugin transport do. A step that has shown nothing then takes the
retry path below; stopping the turn still cancels the request at once.

Chat Completions content passes through the inline thinking filter
(`model/thinking.go`, from Goose): text between `<think>` or `<thinking>` tags,
in any case, with attributes, nested or split across deltas, is reasoning, and
a self-closed tag is removed. A tag still arriving is held back up to 8 KiB; a
stray closing tag and an unfinished tag at the end are text, and reasoning that
never closed stays reasoning. Once the provider reports reasoning in
`reasoning_content` or `reasoning`, inline reasoning is dropped, since that
field is its source. Inline reasoning is stored and replayed as any other
reasoning of the wire. The reference buffers inline reasoning and emits it once
when the stream ends; Go streams it as reasoning in the order it arrives, so a
long thought shows while it is written.

A reply that ends at the output limit is an answer, not a failure. Responses
`response.incomplete` is a terminal that keeps its text, reasoning, complete
calls and usage: `max_output_tokens` finishes as length, `content_filter` as
content filter and any other reason as other. A tool call the reply ended
inside never runs. Chat Completions (on `length`) and Responses drop it and
keep the complete calls, as the reference does. Messages keeps it with an
error, so the engine answers it with a failed result the model reads: "tool
call was not run: the reply reached the output limit before its arguments
ended; send less in one call", or "tool call ended before its arguments were
complete" when the stop reason was another; the reference's one message is
"tool call ended before its content block was complete". A call that sent no
input at all is a call without arguments (`{}`) unless the output limit ended
the reply. A stream that ends without its completion event stays terminal.

A provider that lists models offers only those: `model.Capture` refuses a model
missing from a non-empty list as `not_configured` ("model is not in the
provider's model list"), for turns, media, search and plugin generation alike,
so a model removed from its provider stops at its next turn. A provider with
an empty list runs any name. Such a model declares no capabilities, and its
context window (`model.ContextLimit`) is derived, in order, from:

1. the size its server reports. `model.New` asks an OpenAI-compatible endpoint
   (Responses, Chat Completions, DeepSeek, OpenCode) once for
   `GET <base>/models` and reads `data[].meta.n_ctx` of the model's entry, or
   of the only entry. The request carries the provider's headers, follows no
   redirect and ends after 5 seconds or when the turn stops. An answer,
   including "no size", is kept for 15 minutes and a failure for 60 seconds, in
   at most 128 entries;
2. the bundled catalog (`model/data/model-catalog.json`): the model under its
   API's own vendor, by full name or by the name after the last `/`, with or
   without a dated suffix; then the same name under any vendor, where the
   smallest window wins;
3. 128,000 tokens.

Automatic compaction, `/compact`, overflow recovery, the recorded observation
and the composer's context ring all use that window. Differences from the
reference, all deliberate:

- The reference keys the reported size by session and credential fingerprint;
  Go keys it by API, endpoint and model, since the size is a fact of the
  server.
- The reference resolves names through Goose's canonical-name builder and its
  `limits.json`. Go has one bundled catalog and a plain lookup; a name known
  only under another vendor takes the smallest window listed for it, so a wrong
  guess compacts early instead of overflowing.
- For Azure AI Foundry the reference sizes the model a deployment serves; Go
  does the same when the deployment lookup that `model.New` already performs
  names another model, without a second request.
- The ring of a session that is not running reads the window through
  `store.agentStatisticsBudget`, which performs no request and therefore shows
  the catalog value until a turn has asked the server.

Main transport attempts have at most five additional transient retries before
any output event escapes. Summary output stays buffered and has one additional
transient retry. `model.Run` is the only retry loop: the provider SDKs run with
their own retries off. A retry waits 5 seconds, then twice as long each time up
to 60 seconds (5, 10, 20, 40, 60), each wait spread by a fifth
(`cenkalti/backoff` exponential schedule), so five retries wait about 135
seconds and never more than 162. A positive `Retry-After` (header, or
`retry_after_ms` from a plugin adapter) replaces the scheduled wait, capped at
60 seconds; a provider that asks for longer is asked again after 60. Stopping
the turn ends a wait at once. The retry record carries the wait that was
actually chosen (`delay_ms`). The reference parses `Retry-After` and ignores
it, waiting a fixed 60 seconds five times (5 seconds once for a summary); Go
differs deliberately, so a brief failure is retried within seconds and a rate
limit is retried when the provider says. Authentication, quota, invalid
requests and malformed/truncated streams are terminal. Every attempted request
reports timing, optional usage and failure facts once through the engine observer.
Unknown token counts remain unknown; an explicitly reported zero remains known.
Summary requests retain provider/model/auth and use at most 8,000 output tokens or
the smaller advertised limit, with the lowest declared reasoning choice.

## Desktop channel editor

The Desktop groups the preset directory by brand and treats each offered
preset as one connection method whose routes are switched inside the
configuration form; the model, the Add provider flow and what an edited
channel keeps are in [settings.md](settings.md#brands-methods-and-routes).
`save_provider` carries the chosen route as the preset reference's route key
with the route's wire protocol as `api`, and a hosted route's product as the
`hosted` connection option for the Chat Completions, Anthropic and DeepSeek
protocols. `TestMediaSettings` drives the flow over the bundled catalog: the
two groups and their members, a mark for every preset and brand, distinct
method rows, the family filter's options, the protocol switch and the command
a save sends.

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
`TestToolResultMedia` holds each wire's tool-media layout and the refusals;
`TestToolMedia` runs a tool that returns a picture through storage, reload and
the next request.

The provider editor's capability boxes and the Providers page's Media models
group are described in [media.md](media.md#desktop). A model the user tagged
is saved with `custom_capabilities` and its boxes as tags; the editor never
turns a discovered `image_tool` into `image` while the box stays ticked.
Reset to model data sends the model without `custom_capabilities`, and the
save completes its tags from the catalog as for a model never tagged.

The provider editor requires a positive context limit within the v1 `uint32`
range and at least the model's output limit. It rejects an empty or oversized
field before narrowing values for the protocol, retaining the draft on failure.
