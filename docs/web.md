# Web search and page snapshots

Node owns Web in `internal/node/web`. Client commands carry captured caller,
request and turn identities. The Node registers active fetches and searches
separately, scopes cancellation to their caller, and leases agent requests to the
running turn. A controller disconnect does not transfer ownership. Preview data
never invokes these services.

## Fetch

`Fetcher` validates HTTP(S) URLs, rejects embedded credentials and local host
names, and checks every resolved address against the source public-address
policy and the Node's local interfaces. A host that ends in a number is read as
the IPv4 address a resolver or proxy would make of it (`http://127.1/`,
`http://2130706433/`, `http://0x7f.0.0.1/`), as the reference's URL parser does,
and is refused when that is not a valid address. Every redirect is checked. Direct
connections resolve again immediately before dialing and connect only to the
validated addresses. System proxies on macOS and Windows combine with the
explicit environment settings. Proxy target checks use a bounded best-effort
resolution, excluding the source TUN fake-IP range; failed proxies never retry
directly.

The service owns bounded transports and immutable URL snapshots. It coalesces
concurrent reads of one URL, retains at most 100 snapshots for 15 minutes, and
pages content in 20,000-character slices. Whole fetches have a 30-second bound;
body and extracted content are limited to 10 MiB. HTML decoding follows BOM,
header and meta declarations, then UTF-8 or GB18030 fallback. Pruning removes
hidden and non-content elements, prefers a main region or one article, and uses
bluemonday plus html-to-markdown/v2 for sanitized Markdown. PDF extraction reuses
`internal/office` rather than a second document parser.

A captured, enabled Tinyfish renderer may enrich sparse HTML. Its final URL must
pass the same public-target policy. Failure retains the source direct-fetch
fallback semantics. Page content is always untrusted. Pagination reads the
captured snapshot rather than silently refetching it.

## Search

Search settings and private credentials remain in the existing profile tables.
The SQL-only adapter in `store/workspace_web_settings.go` preserves revisions,
keep/replace/clear secret updates, channel order and capability flags. Saving
settings invalidates the current cache and cooldown state. A stale request may
finish against its frozen plan but cannot populate the new configuration's
cache or cooldowns. Driver tests use one explicit channel, no fallback and no
cache, with a one-result probe.

The search service supports Brave, Tavily, SearXNG, Exa, Bocha, Tinyfish,
Volcengine, Deepseek and configured native model search. Automatic selection
follows saved order. Explicit selection advances only on temporary errors when
fallback is enabled. Cancellation and uncertain cleanup never advance to
another channel. Rate-limit responses retain their advertised retry deadline;
Tinyfish authorization failures use the source one-minute cooldown. Results
have normalized include/exclude host filters and known-date freshness filters.
A 50-entry, five-minute cache uses normalized input and captured selection.

REST drivers use their provider-specific request and result contracts, a 1 MiB
response bound and the source deadlines without added retries. Exa uses REST
when its key is configured; otherwise the pinned MCP Go SDK calls the hosted
`web_search_exa` tool. Automatic multi-round-trip work and reconnect retries are
disabled. Only final results are accepted. The source 20-second operation and
seven-second owned cleanup boundaries are preserved; incomplete cleanup blocks
fallback. The SDK owns protocol shutdown, while tracked HTTP bodies confirm
that transport resources have settled.

Native search supports Responses, Anthropic Messages and Gemini grounding.
Provider metadata and model selection are captured by Node; the existing model
authority resolves credentials and prepares requests. Web owns input, search
tools and system guidance, while declared model policies may supply generation
options. It does not implement another agent loop. Requests use a 120-second
operation bound and a 90-second bound per send. Responses accepts JSON or a
bounded event stream through the official OpenAI SDK decoder. Anthropic retains
original assistant blocks across at most three continuations. Gemini requires
completed grounding metadata and excludes thought parts.

`workspace_web.go` routes settings, fetch/search, fresh driver probes and scoped
cancellation. `workspace_web_native.go` binds current API-key authorization to
the captured model and the Agent engine's canonical usage writer. Grant-based
authorization retains the model service's existing not-configured behavior
until the provider authorization phase supplies it.

Ordinary captured model responses also retain their native source annotations.
The existing SDK adapters carry immutable source snapshots to canonical messages;
Store projects valid HTTP(S) citations and grounding sources with ranges checked
against the message's Unicode text. Gemini search suggestions select the accepted
Globe disclosure in the live conversation, with numbered source links. Generated
support spans never become fetched excerpts.
Entry-relative citation endpoints are rebased across assistant text blocks for
presentation. Numbered links keep Unicode character positions, while canonical
text and copy/export content remain unchanged.

Canonical usage is appended after each billed response and before content
validation, including when cancellation arrives afterward. The append uses the
captured turn without inheriting caller cancellation. Persistence failures do
not cause selected-driver fallback. Results require HTTP(S) citation URLs;
generated answer spans never become excerpts. Empty excerpts may use actual
public page reads, with four concurrent reads and a ten-second bound. Grounding
redirects are filtered after resolving their final source. Secret protection
runs before and after enrichment. Summaries and source excerpts remain
untrusted, bounded text.

## Ownership and validation

Node owns admission, current authorization, cancellation, turn leases and usage
persistence. Web owns network requests, provider mappings, immutable snapshots,
filtering, cache and cleanup. Client owns shared projection and the accepted UI
owns presentation. Search and fetch do not collect third-party histories or
billing.

Deterministic contract fixtures cover DNS rebinding and redirects, proxy failure,
HTML/charset extraction and snapshots, channel order/cooldowns, provider request
contracts, hosted MCP lifecycle, native completion/continuation, usage retention,
secret protection and citation enrichment. They use injected clocks, DNS and
HTTP transports without live credentials. Coordinated builds, race checks and
platform integration results belong to phase validation; compilation alone is
not evidence of native behavior on another operating system.
