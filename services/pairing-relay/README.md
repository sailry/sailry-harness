# Short-code ticket exchange

This service exchanges a six-digit code for an opaque iroh pairing ticket.
Rust Link establishes the authenticated connection and saves peer trust.
There is no account login or second approval page. Codes expire after **60 seconds**.
Clients mint a new invitation and publication request on rollover; they stop on
successful pairing, cancellation or closing the pairing view.

## Contract

All requests use JSON POST bodies over HTTPS. Request IDs are 64 lowercase hex
characters: a 12-character Unix millisecond timestamp followed by 52 random hex
characters (208 random bits). Retain the complete ID and body across retries.
The Rust client and TS `requestId()` generate this format. IDs expire after 60
seconds; clocks may lead the server by at most five seconds. Before the first
product release, the contract stays at v1 and changes in place. Client and
service use the same current definition; no legacy formats or migrations are supported.

| Path | Body | Success |
| --- | --- | --- |
| `/v1/codes` | `request_id`, `ticket`, optional `expires_at_ms` | 201: `code`, `cancel_token`, `expires_at_ms` |
| `/v1/codes/redeem` | `request_id`, `code` (string) | 200: `ticket`, `expires_at_ms` |
| `/v1/codes/cancel` | `code`, `cancel_token` | 204 |

Redeem has one winner. Retrying the winning request ID returns the same ticket
until expiry; another claimant receives 404. A lost publication response can
be retried without allocating another code. Cancellation removes the ticket,
but revoking a ticket already delivered to a client must also revoke the local
Rust invitation. It does not remove an already established pairing.

Rust publishers provide their invitation deadline. The server caps validity at
both that deadline and 60 seconds from admission; HTTP latency cannot extend it.
The shared Rust `Relay::share` future drives rollover while its owner is alive.
Cancellation or dropping the future invalidates its local invitation. Remote
cleanup is best effort; expiry remains authoritative if the network is unavailable.

The service sees the ticket, unlike the former password-authenticated mailbox.
Never publish a private identity key or business credential as a ticket. Disable
request-body logging. Responses are `no-store`; codes and tickets never go in URLs.
Six digits are not a long-term secret: short expiry, one-use arbitration, bounded
capacity and conservative source/global rate limits reduce guessing risk but do
not turn the public service into a high-entropy authentication credential.

## Run and verify

```sh
pnpm install --frozen-lockfile --ignore-scripts
pnpm typecheck
pnpm test
pnpm test:rust
pnpm test:desktop
pnpm build
pnpm build:edgeone
pnpm test:edgeone
pnpm dev
```

`build` is a Wrangler **dry run**, not a deployment. Tests include actual workerd
process restart, Durable Object SQLite and concurrent HTTP redemption. Tests use
isolated temporary storage, not production namespaces.

`test:rust` starts workerd and two real Rust Nodes, publishes an invitation,
redeems its PIN, authenticates over iroh and executes a Node snapshot request.
It also verifies retry and cancellation behavior. It does not test a public relay.

`test:desktop` uses Kit's interaction test context with real Tokio I/O enabled,
workerd and isolated Nodes. It clicks share/cancel/connect and verifies a real
authenticated Node request.

For desktop integration, run `cargo run -p sailry-desktop -- --data-dir
/absolute/private/profile` from the repository root, then open Settings → Remote
connections and enter this service URL. Literal loopback HTTP URLs are allowed for
local testing. Add `--relay https://...` for a custom iroh relay. Desktop normally
starts its Node in `~/.sailry`; use an explicit private test directory for local
acceptance. Only `--preview` starts without a Node or user data.

Tickets carry the complete iroh endpoint address, including endpoint ID, direct
addresses and the selected relay URLs, plus the expiring invitation credential.
The exchange service preserves this opaque value; it must not substitute its own
relay. Host supports `--internet` for the default relay network or repeated
`--relay https://...` options for custom relays. The relay connection is established
before Host announces its address. These options are separate from the short-code
service URL. Custom relay cloud connectivity still requires deployment acceptance.

Headless Host can explicitly enable pairing with `--pairing-service https://...`.
It prints a private six-digit PIN with its expiry, automatically replaces expired
codes, and stops sharing after pairing succeeds. The Node continues serving its
paired clients. SIGINT/SIGTERM cancels the invitation before shutting down the Node.
Treat this opt-in process output as sensitive; do not expose it in public logs.
Without this option Host does not publish invitations. Run
`node test/rust-e2e.mjs --host` here to verify a real Host process with workerd,
iroh pairing, a Node request, graceful exit and persisted peer recovery.

On macOS, the Host archive also includes `host-launchd.sh` (requires `jq`). First
pair the foreground Host using the intended private profile, then stop it. Generate
a user service with the same profile and a fixed installed binary path:

```sh
bash host-launchd.sh /absolute/sailry-host /absolute/private/profile --internet
```

The generator prints a new plist path; it does not install or start a service.
It preserves argument boundaries and creates private logs inside the profile.
Use `--relay https://...` instead of `--internet` for a custom relay. Omit
`--pairing-service` from the installed service to keep pairing closed after restart.
Place the generated plist at `~/Library/LaunchAgents/ai.sailry.host.plist`, then run:

```sh
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/ai.sailry.host.plist"
```

This service runs while the macOS user is logged in, independently of Desktop.
Launchd restarts a failed process and sends SIGTERM when unloaded. To update, run
`launchctl bootout "gui/$(id -u)/ai.sailry.host"`, wait for `Node stopped` in the
private output log, replace the installed binary with the verified new package,
and bootstrap the same plist. Keep the previous binary and the stopped profile
backup for recovery; never reset or convert an incompatible development profile.
Desktop and Host cannot simultaneously own that profile. Logout stops the user
service; system-wide installation and other platforms require separate acceptance.

## Cloudflare and EdgeOne

Cloudflare: `wrangler.toml` provisions the `PairingCodes` SQLite Durable Object
binding when explicitly deployed. Development namespaces use the current v1
contract. Do not replace an existing deployment or convert its data implicitly.

Both targets run `src/exchange.ts`, using native storage only: Durable Object/
SQLite on Cloudflare and Blob on EdgeOne. No Redis, external database or
cross-provider proxy is involved. The Blob adapter pins `@edgeone/pages-blob`
0.0.16, uses strong reads and `onlyIfNew` writes, and bypasses caches. See the
[official Blob API](https://pages.edgeone.ai/document/blob-storage).

EdgeOne's project root is this directory. `cloud-functions/[[default]].ts` is the
native Node handler; `edgeone.json` configures build output and a native minute
schedule for `/internal/cleanup`. The SDK obtains platform credentials inside
the deployed function; never put a project token into client code. The handler
uses platform `clientIp`, not a client-supplied forwarding header. See the
[Node Functions contract](https://pages.edgeone.ai/document/node-functions).

Records belong to the request ID's immutable minute generation. PIN modulo 3
routes to that generation; a generation has 333,333 candidate six-digit codes.
The three neighboring generations are distinguishable, preserving validity
across minute boundaries and preventing immediate reuse. Publication election
makes orphan candidates unusable; a separate conditional claim selects one
redeemer. Cancellation and expiry are rechecked after asynchronous arbitration.
No distributed read/delete lock or SQL transaction emulation is used.

Rate limits use fixed UTC minute windows: at most 120 attempts per action
globally, six publications and five redemptions/cancellations per source, with
256 allocation slots per generation. Cleanup reads at most 128 records per
page and only deletes generations at least three minutes old. Cloudflare uses
an alarm plus request-driven cleanup; EdgeOne uses a bounded native scheduled
pass plus request-driven cleanup. Logical expiry is exactly the invitation/ID
deadline, independent of physical cleanup. Failed schedules can leave expired
objects until a later pass; they never make an expired ticket redeemable.

`test:edgeone` runs the production Node handler and Blob adapter against a local
SQLite-backed Blob contract fixture, then real Rust Nodes/iroh, including the
60-second rollover. It does not contact EdgeOne storage or prove cloud deployment.
`build:edgeone` verifies bundling only. Actual deployments and scheduled-job
execution on both cloud providers remain separate release acceptance.

No resources, secrets, DNS or cloud deployments are created by the test commands.
