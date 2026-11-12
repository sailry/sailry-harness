# Agent programs as models

The user's decision of November 12, 2026 authorizes one more model route: an
installed, signed-in command-line agent that speaks the
[Agent Client Protocol](https://agentclientprotocol.com) (ACP, JSON-RPC 2.0 over
standard streams, one message per line) is used as a **plain language model**.
Sailry keeps its loop, tools, history and compaction. The agent's own tools and
agent behaviour are switched off. This is not a second agent loop.

The first package is `plugins/agy` (Google Antigravity CLI, `agy acp`). A
second consumer, the Cursor CLI (`agent acp`), is decided but not built; the
declaration is shaped so that it needs no core change.

## Ownership

| Layer | Owner | Knows |
| --- | --- | --- |
| Declared programs | `internal/node/program` | Processes. No protocol, no vendor |
| ACP model route | `internal/node/agent/model` (`acp.go`, `acp_text.go`, `acp_discovery.go`) | ACP. No vendor name or branch |
| Vendor differences | The package manifest (`programs`, `model_adapters[].acp`) | One vendor's flags and quirks |

Packages are declarations. They get no script-level process or standard-stream
API, and the route has no script hook: every difference found so far is data.

### Layer 1: declared programs

`program.Start` runs one declared executable; `program.Pool` bounds and reaps
them. The layer offers:

- **Consent.** The `process.run` action, shown in the package details as "Run
  its declared programs on this device". A manifest with `programs` and without
  the action is refused.
- **Resolution.** `command` is an executable name the Node resolves on its own
  search path. A name with a separator is refused, there is no shell, and a
  missing executable is `program.ErrNotInstalled`.
- **Environment.** The program inherits only the search path, the account's
  directories and session bus, locale, temporary directory and proxy variables.
  A declaration may add variables the Node does not own.
- **Directory.** The working directory is
  `<profile>/plugins/data/<package>/programs/<program id>`. Declared files are
  written there through an `os.Root` before each start and nowhere else.
- **Lifetime.** Started on demand, at most four per package, killed after ten
  idle minutes, when the package changes, is disabled or removed, and when the
  Node stops. Each program leads its own process group (a job object on
  Windows), so its children die with it.
- **Diagnostics.** Standard error goes to the Node's debug log line by line; the
  last line is kept for a failure message.
- **Streams.** `Process.Stdin` and `Process.Stdout` are plain pipes.

A program attached to a PTY is **not built**. Sharing `internal/node/terminal`
is not small: that package owns a MyGo terminal session with a mirror, sizing
and a controller contract, not a bare PTY. The seam is one more start function
in `internal/node/program` that returns the same `Process` over a PTY that
package opens; `Pool` and its users do not change. There are no sockets.

A second line protocol (for example a vendor's own stream format) reuses layer 1
unchanged. It would add: its own manifest member naming a program, its framing
and handshake, its mapping to model deltas and failures, and its `Pool` value
(the per-process state `acpAgent` is for ACP).

### Layer 2: the ACP model route

The client is `github.com/coder/acp-go-sdk` v0.13.5 (Apache-2.0), used at its
connection level (`NewConnection`, `SendRequest`, `SendNotification`). Its typed
client is not used because agents return members it does not model (the model
list of `session/new`, `session/set_model`).

- **Handshake.** `initialize` with protocol version 1 and client capabilities
  `fs.readTextFile: false`, `fs.writeTextFile: false`, `terminal: false`.
- **Plain-model discipline.** `session/new` passes `mcpServers: []`. Every
  `session/request_permission` is answered with the agent's reject option, or
  `cancelled` when it offers none. Any other request from the agent is
  `Method not found`.
- **System text.** The discipline, the tool convention with the tool list, then
  the caller's system text. It goes into the declared `_meta` path of
  `session/new` when the agent advertises that path as `true` in its
  capabilities; otherwise it opens the first prompt between `<system>` tags.
- **Models.** Read from the session's `category: "model"` select option and from
  `models.availableModels`. Selected with `session/set_config_option`, with
  `session/set_model`, or by the program's `{model}` start argument (one process
  per model). Declared effort suffixes fold variants into one model with effort
  levels; otherwise a `thought_level` session option supplies them. Discovery
  opens a session and sends **no prompt**. An agent that lists nothing fails
  with "`<command>` did not list its models"; no model is invented.
- **Tool calls as text.** An ACP agent has no function-call channel a client
  can fill. The system text lists the tools; the reply names a call as
  `<tool_call>{"name": …, "arguments": …}</tool_call>`; results return as
  `<tool_response id= name=>` blocks. The streaming parser holds back text that
  may still become an opening tag, ends a call at the first closing tag before
  which the body is valid JSON (a closing tag inside a JSON string does not end
  it), gives every call a fresh identifier, keeps identical parallel calls
  apart, keeps the text before the first call and drops what follows a call. A
  body that is not JSON, or a reply that ends inside a call, is an
  `InvalidResponse` model failure. Nothing is repaired.
- **Session affinity.** Each program serves one ACP session. The route records
  a digest per message the session holds. A request whose history starts with
  those digests sends only the new messages; any other history opens a new
  session in the same process and replays as a `[USER]`/`[ASSISTANT]`/`[TOOL]`
  transcript. After a cancelled or failed prompt the session's content is
  unknown and the next step replays.
- **Byte budget.** A prompt larger than the declared `replay_bytes` fails with
  `ContextOverflow` before anything is sent, so the loop compacts. Nothing is
  truncated.
- **Usage and stop reasons.** `inputTokens` plus cached read and write tokens is
  the input; `thoughtTokens` is reasoning inside the output. `max_tokens` is
  `length`, `refusal` is `content_filter`, `cancelled` is a `Cancelled` failure.
- **Cancellation.** `session/cancel`, then five seconds for the prompt to end,
  then the process is killed. The 600-second silence rule applies.
- **Failures** are model failures, never assistant text: executable missing
  (`NotConfigured`, "`<command>` is not installed or not on the search path"),
  code `-32000` (`Authentication`, the declared sign-in instruction), process
  exit (`Network`, with the last diagnostic line), rate limits (`RateLimited`),
  a declared failure marker in the reply (`Unavailable`).

## Declaration

See [sdk/plugins.md](../sdk/plugins.md#agent-programs) for the members. A new
ACP-backed package declares:

1. Actions `models.provide` and `process.run`.
2. One `programs` entry: `command`, `args` (with `{model}` only when the model
   is a start argument), optional `env` (with `{key}` for an API key) and
   `files` that switch the agent's own tools off.
3. One `model_adapters` entry with `framing: "ndjson"`, `origins: []`, no
   `script`, its capabilities and `acp`: `program`, `replay_bytes`, `sign_in`,
   and where they apply `system_meta`, `efforts`, `model_argument`,
   `auth_method`, `failure_markers`.
4. One `model_providers` preset with a `plugin` route, `adapter`,
   `models: discover` and `auth` `none` (the agent holds its own sign-in) or
   `api_key` (the key reaches the program through `{key}`).

The route's `endpoint` is required by the channel contract and is not contacted;
a package declares its vendor's home page.

Such a channel is created without a sign-in step, so a package that drives the
user's own account declares its risk notice as the preset's optional `warning`
(`{text, locales}`). The Desktop asks it once when the user picks the preset,
before the channel form
([providers.md](providers.md#package-sign-in-warning-and-model-facts)). `agy`
declares one from 0.1.1.

## Antigravity

`plugins/agy` 0.1.0 runs
`agy acp --agent sailry-model --include-default-customizations=false --sandbox`
with `AGY_CLI_DISABLE_AUTO_UPDATE=true`.

- **Sign-in without copying credentials.** `agy` keeps its own profile and
  keyring. The package does not pass `--gemini-dir` or `--app-data-dir`, reads
  no credential file and sets no `SSH_CONNECTION`. What strips the agent lives
  in the program directory, which is also the session's `cwd`:
  `.agents/agents/sailry-model/agent.md` (no tools, skills, rules, agents,
  plugins, inherited MCP servers or default components) and `.agents/hooks.json`
  (a `PreToolUse` hook that denies every tool).
- **`SSH_CONNECTION` is not set.** It is needed only where `agy` must fall back
  to file token storage for a second profile. This package uses no second
  profile, so the user's existing sign-in is what `agy` reads.
- System text goes to `_meta.antigravity.systemPrompt.append`; `-low`, `-medium`
  and `-high` variants are effort levels; the replay budget is 112 KiB, under
  `agy`'s own prompt limit; "Agent execution terminated due to error." in a
  reply is a failure.

### Differences from the references

Not taken from `agy-acp2api`: copying the profile into a per-worker directory,
redirecting API traffic through a local proxy (`--cloud_code_endpoint`), faking
`SSH_CONNECTION`, the HTTP gateway and admin API, a public default key, and
reporting errors as assistant text. Not taken from `agy-cli`: the MCP bridge
through `--ide-mcp-port` and a `nc` shim on the search path, which relies on
undocumented behaviour. Native tool calls through such a bridge are a possible
follow-up, not part of this route.

## Limits

- Not verified against a real account: no prompt, session or sign-in was run
  during development. Only `agy --version`, `agy --help` and `agy acp --help`
  were. The fake agent in `internal/node/agent/model/acp_test.go` covers the
  route; it does not prove what `agy` does.
- The hook command uses `printf`, so the hook is inert on Windows. The stripped
  agent, the refused permissions and the absent client capabilities still apply.
- Whether `agy acp` loads `.agents/hooks.json` from a folder it has not been
  told to trust is unverified.
- One prompt runs per program; a fifth concurrent request of one package fails
  with "every `<command>` process is busy".
- Images travel only when the agent advertises `promptCapabilities.image`.
  Files are not supported.
- The model's context size and output limit are not reported by ACP and are not
  invented.
- Tool calls are text. A model that ignores the convention answers in prose;
  one that half-follows it produces an `InvalidResponse` failure.
