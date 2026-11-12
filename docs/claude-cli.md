# Claude through the user's Claude Code

By the user's November 12, 2026 decision a Claude subscription is no longer
reached with Sailry's own OAuth grant and direct Messages API calls. Sailry
drives the user's own installed, signed-in, unmodified `claude` binary as a
plain model. Sailry keeps its loop, tools, approvals, history and compaction;
it holds no Claude token and reads none.

The reference is magpie (MIT, revision `0af73f4`, read only). Flags were
checked against Claude Code 2.1.322 on the development machine with
`claude --help` and parser errors only. No prompt was sent, no session was
started and `~/.claude` was not touched, so every statement about runtime
behavior below is taken from magpie or from the binary's strings and is
unverified until the user's acceptance run.

Status: the `-p` route, its codec and the tool bridge are built in
`internal/node/claudecli` against fakes. The interactive mode, the provider
declaration and the wiring into `node.newModel` are designed here and not
built ([what blocks them](#interactive-mode)).

## Modes

| Mode | What runs | When |
| --- | --- | --- |
| Interactive (designed, default once built) | The real TUI in a pseudo-terminal. Input is typed as a bracketed paste; output is read from Claude Code's own session transcript; tools cross through MCP | Every turn, when the machine has a PTY and the mode is accepted |
| `-p` (built) | `claude -p` with stream-json on stdin and stdout | Until interactive mode is accepted; where no PTY exists; once per turn when interactive mode is refused by content |

magpie reports that Anthropic meters `-p` (entrypoint `sdk-cli`) at about
twice the plan allowance of interactive use (`cli`), citing
anthropics/claude-code#97074. Sailry sets no entrypoint variable in either
mode: each mode is what it is.

## The tap, and why it is not ported

magpie's interactive mode streams replies through a "tap"
(`internal/claudetui/tap.go`, `tap.js`). It is code injection:

- The helper sets `BUN_OPTIONS=--preload <tap.js>`, so Bun loads magpie's
  script inside the `claude` process before Claude Code itself.
- The script replaces `Bun.FetchSession` with a subclass. For each main
  `/v1/messages` request it tees the response body and writes every SSE event
  and the `anthropic-ratelimit-unified-*` headers to a file.
- It also rewrites outgoing request bodies: it replaces `safeguards` and sets
  `output_config.effort` from a file, and sets `CLAUDE_CODE_EXTRA_BODY` inside
  the process.
- It removes itself from `BUN_OPTIONS`, `process.execArgv` and the
  environment so that nothing shows it was loaded.

That alters the binary's behavior and hides doing so. It is out, with
everything that depends on it: token streaming in interactive mode, rate-limit
figures on successful interactive turns, and changing effort inside a running
interactive process.

## Interactive mode

Without the tap the mode still has a mechanism that injects nothing:

1. Start `claude` in a PTY (200x50) with a new `--session-id <uuid>`.
2. Wait for the prompt on the screen and for the bridge to see `tools/list`.
3. Paste each user message (bracketed paste) and press Enter.
4. Tail `~/.claude/projects/<folder>/<session-id>.jsonl`, the transcript
   Claude Code writes for that session: assistant rows, a `<synthetic>` API
   error row, and `turn_duration` as the end of the turn.
5. Tools cross through the same MCP bridge as `-p`; the `tools/call` is the
   step boundary.

It is viable on those terms, with these costs, all taken from magpie:

- No streaming. The transcript has a message only once it ends, so text and
  reasoning arrive a whole message at a time.
- Text before a tool call can be lost. In magpie's measurement 4 of 8 runs
  wrote the calling message after its MCP call returned; such a step has the
  call and no text or reasoning, and its usage arrives with the next reply.
- Screen reading. Readiness, the folder-trust question ("No, exit" is the
  default), the first-run screens and the sign-in screen are recognized from
  drawn text, which changes between Claude Code versions. magpie's
  `claudetui` is about 3,000 lines with as many of tests, most of it this.
- Residue in the user's Claude Code data: one transcript per conversation
  under `~/.claude/projects`, a trusted-folder entry for Sailry's working
  folder in `~/.claude.json` (written by Claude Code when the question is
  answered), and the conversations appear in the user's `/resume` list.
  Sailry would only read its own session's transcript there.
- Images are pasted as temporary file paths. Cancellation is the Esc key or
  ending the process; there is no control request. Effort is fixed at start.
- Startup sends Claude Code's own small quota request before the first turn.

It is not built in this pass, for three reasons: it needs a PTY child from
the process layer that `wt/agy` has not merged, the rule against a second
process layer forbids building one here; its correctness is almost entirely
screen and timing behavior that cannot be written honestly without running
the real binary, which this pass may not do; and it is not small. The seam
is ready for it: the route talks to a `Child` (bytes in, bytes out) and a
`Bridge`, and an interactive child would present the same stream-json lines
the `-p` child does, as magpie's helper does.

Options for the user:

1. Accept `-p` first, then build interactive mode against the merged process
   layer with a real account at hand (recommended).
2. Build interactive mode without ever answering the trust question: the
   user runs `claude` once in Sailry's working folder. Less screen driving,
   one manual step.
3. Stay on `-p` and accept its metering.

## Command line

`-p` mode, as `claudecli.Arguments` builds it:

```text
claude -p --input-format stream-json --output-format stream-json
  --include-partial-messages --verbose
  --model <model> --tools "" --strict-mcp-config --mcp-config <json>
  --setting-sources "" --settings <json> --disable-slash-commands
  --no-session-persistence --permission-mode dontAsk --allowedTools mcp__sailry
  --append-system-prompt-file <file> [--effort <level> --thinking-display summarized]
```

| Flag | Purpose | Source |
| --- | --- | --- |
| `-p`, `--input-format stream-json`, `--output-format stream-json`, `--include-partial-messages`, `--verbose` | Streamed JSON both ways | Help; choices checked by the parser |
| `--model` | The captured model | Help |
| `--tools ""` | No built-in tool | Help ("Use \"\" to disable all tools") |
| `--strict-mcp-config`, `--mcp-config` | Only Sailry's bridge; none of the user's MCP servers | Help |
| `--setting-sources ""` | No user, project or local settings, so none of their hooks, permissions or environment | Help lists the sources; the empty value is magpie's, unverified |
| `--settings {"disableDeepLinkRegistration":"disable","showThinkingSummaries":true}` | No URL-handler registration; thinking summaries | Help for the flag; keys are magpie's and occur in the binary, unverified |
| `--disable-slash-commands` | No skills or commands | Help; not used by magpie |
| `--no-session-persistence` | No transcript on disk | Help (print mode only) |
| `--permission-mode dontAsk --allowedTools mcp__sailry` | Only the bridge's tools run and nothing prompts | Help and parser. magpie passes `--dangerously-skip-permissions`; Sailry does not bypass. The server-wide rule `mcp__sailry` is magpie's interactive form, unverified here |
| `--append-system-prompt-file` | Sailry's system prompt after Claude Code's | Hidden flag: absent from help, present in the binary. magpie uses it. `--append-system-prompt` is the documented form and would put the prompt on the command line |
| `--effort` | The captured effort | Help (`low`, `medium`, `high`, `xhigh`, `max`) |
| `--thinking-display summarized` | Reasoning text in the stream | Hidden flag; the parser names its choices |

Interactive mode would drop `-p`, the formats, `--verbose`,
`--include-partial-messages`, `--thinking-display` and
`--no-session-persistence`, and add `--session-id <uuid>` and `--no-chrome`
(both in help). magpie's `--permission-mode default` is refused by 2.1.322:
the choices are `acceptEdits`, `auto`, `bypassPermissions`, `manual`,
`dontAsk`, `plan`.

Not used: `--bare` (never reads the OAuth sign-in), `--system-prompt[-file]`
(replaces Claude Code's prompt, the variant magpie flags as a ban risk),
`--dangerously-skip-permissions`, `--safe-mode` (also disables MCP servers;
whether that includes `--mcp-config` is unknown).

## Environment

The child gets the scrubbed environment of the process layer plus the rows
marked adopt. A user could set each of them for a headless run.

| Variable | magpie | Class | Sailry |
| --- | --- | --- | --- |
| `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`, `ANTHROPIC_BETAS` | removed | Correctness: the subscription sign-in answers, not a stray key or endpoint | Adopt (removed) |
| `CLAUDECODE`, `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_SSE_PORT` | removed | Isolation from a Claude Code session Sailry was started from. Removing is not forging: nothing sets an entrypoint | Adopt (removed) |
| `CLAUDE_CODE_OAUTH_TOKEN` | removed, or set to a held setup token | Removal is isolation; setting it holds a credential | Adopt removal only |
| `CLAUDE_CODE_EXTRA_BODY` | removed; set by the tap | Removal is isolation; setting it alters requests | Adopt removal only |
| `CLAUDE_CODE_PROMPT_CACHE_TTL`, `FORCE_PROMPT_CACHING_5M` | removed, then the TTL set to the user's pick | Correctness for magpie's own cache marks | Not adopted: Sailry places no marks; the user's values pass through |
| `ENABLE_CLAUDEAI_MCP_SERVERS=0` | set | Isolation: no claude.ai connectors | Adopt |
| `DISABLE_AUTO_COMPACT=1` | set | Correctness: Sailry owns compaction | Adopt |
| `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` | set | Isolation: no memory read or written | Adopt |
| `CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS=1` | set | Isolation: no repository status in the prompt | Adopt |
| `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1` | not set | Isolation: no CLAUDE.md. The name occurs in the binary; unverified | Adopt |
| `DISABLE_AUTOUPDATER=1` | set | Isolation: a run does not update the user's install | Adopt |
| `DISABLE_ERROR_REPORTING=1` | set | Isolation | Adopt |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` | one-off runs only | Isolation, but it stops feature flags, and with them sending only new messages per turn | Not set; the user's own value passes through |
| `DISABLE_TELEMETRY`, `DO_NOT_TRACK` | passed as the user set them | The user's choice | Pass through |
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | set from its proxy, SOCKS bridged | Correctness | Adopt from the Node's proxy; a SOCKS proxy is refused before start (Bun takes HTTP proxies only) |
| `IS_SANDBOX=1` | set when root | A false claim that makes the permission bypass acceptable | Out |
| `BUN_OPTIONS=--preload`, `MAGPIE_CLAUDE_TAP`, `MAGPIE_CLAUDE_GUARD` | set | Code injection | Out |
| `CLAUDE_CONFIG_DIR` | set for saved accounts | Needs copied credentials | Out |
| `DISABLE_PROMPT_CACHING=1` | set for client cache marks | Not needed | Out |

## Isolation and residue

The binary uses its own sign-in in its default configuration location;
Sailry sets no `CLAUDE_CONFIG_DIR` and never reads the Keychain item or
`.credentials.json`.

| The user's | Kept out by |
| --- | --- |
| Settings, hooks, permissions, settings `env` | `--setting-sources ""`. Managed (policy) settings still apply |
| MCP servers, claude.ai connectors | `--strict-mcp-config`, `ENABLE_CLAUDEAI_MCP_SERVERS=0` |
| Skills, commands | `--disable-slash-commands` |
| CLAUDE.md, memory | A private working folder, `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` |
| Session files | `--no-session-persistence` in `-p` |
| Installed plugins | Not confirmed. `--setting-sources ""` should leave none enabled; check in acceptance |

The working folder is one fixed private folder under the profile
(`tmp/claude-cli/work`), never a project. Sailry's system prompt names the
workspace; Claude Code's prompt names this folder as its own.

What a `-p` run may still leave in the user's Claude Code data, written by
Claude Code itself: counters and a project entry for the working folder in
`~/.claude.json`, and refreshed sign-in tokens. Unverified; list the changed
files in acceptance.

## Tools

Sailry's tools are native MCP tools of the `claude` process.

- The Node runs the MCP server itself (the pinned go-sdk `mcp.Server`), one
  `Bridge` per child, listening on a Unix socket inside a private `0700`
  folder of the run.
- `--mcp-config` names one stdio server, `sailry`, whose command is the
  running Sailry binary in a hidden mode: `<sailry> claude-bridge <folder>`.
  `cmd/sailry-desktop` and `cmd/sailry-host` enter it before anything else.
  The helper is a relay: it dials the socket, sends the token it reads from
  the folder's `token` file (32 random bytes, `0600`), and copies its stdin
  and stdout to the socket. It holds no tool logic.
- The socket is reachable only by the same OS user; the token is compared in
  constant time and never appears on a command line.
- Claude Code names the tools `mcp__sailry__<name>`. It keeps only
  `[A-Za-z0-9_-]` and 64 characters, so a name that would change is offered
  as a cut form with a hash of the whole name (`claudecli.ToolName`, ported
  from magpie's `bridgeToolName`); the route maps it back.
- A tool set that changes between steps (after `tool_search`) is applied to
  the running server, which notifies `tools/list_changed`; the parked result
  waits up to five seconds for the new `tools/list`.
- A schema whose root is not an object is refused before start.

## Steps and turns

Sailry asks for one inference step at a time; Claude Code runs a whole turn.
The MCP call is where they meet.

1. A step starts when the route writes a user message to the child (or
   resolves parked calls).
2. Text and reasoning deltas stream from `stream_event` lines.
3. A `tool_use` block for a bridge tool becomes a tool call with Claude's own
   `toolu_` identity. The step ends with reason tool calls at the
   `message_stop` of that message.
4. Claude Code then sends `tools/call`; the bridge parks it under the
   identity in `_meta["claudecode/toolUseId"]` (by name when absent).
5. Sailry settles approvals and runs the tools. The next step carries their
   results; the route hands each to its parked call, or keeps it for a call
   that has not arrived yet. Several calls of one message are answered one
   after another, as Claude Code makes them.
6. A message that ends without a bridge call ends the turn at the `result`
   line, with reason stop or length.

User input that joins with tool results (guidance) is appended to the last
result as a marked `[user message]`, so the process and its cache go on.

One child serves one conversation and is kept while idle (ten minutes by
default, at most four children; magpie measured about 205 MiB per idle
process on Linux arm64). A request without tools, such as a compaction
summary or a title, uses its own child and ends it with the step.

### Divergence

After each step the route remembers how many prompt messages the child has
taken and a digest of them. The next request goes on in the same child only
when that prefix is unchanged, the message after it is the reply's assistant
message with the same calls, and the rest is exactly the parked results or
new user messages. Anything else starts a new child told the whole
conversation: an edit, a rewind, a compaction, another model, effort or
system prompt, a stopped turn, an ended or evicted child, a Node restart.

The replay is one user message: the history as marked text (`Human:`,
`Assistant:`, `[tool call … id=… args=…]`, `[tool result id=…]`) with its
images as image blocks, then the open turn. Its limits: earlier calls are
text, not calls; reasoning and its signatures are gone; the whole
conversation is written to the prompt cache again. `--resume` with a kept
session would avoid the last, at the cost of session files in the user's
Claude Code data; it is not used.

## System prompt, reasoning, images, usage

- Sailry's system prompt is appended after Claude Code's own through a file.
  It never replaces it. Per-turn context messages stay in the conversation
  as they are for every provider.
- Reasoning streams as summarized thinking. Signatures stay inside the
  child; Sailry stores the text and replays none of it.
- Effort is `--effort` at start. The `apply_flag_settings` control request
  magpie uses to change it in place is not sent; a changed effort starts a
  new child.
- User images are base64 `image` blocks of the stdin message; tool images
  are MCP image content.
- Usage is the step's last API message: input, cache writes and cache reads
  from `message_start`, output from `message_delta`. It includes Claude
  Code's own system prompt, so the context ring reads higher than on the API
  route. Costs stay reference prices.
- `rate_limit_event` lines (status, window, reset) are decoded. A rejected
  window fails the step as rate limited with the reset as its retry time.
  Showing the windows as a quota snapshot is not built.
- Prompt caching is Claude Code's own; Sailry places no marks.

## Cancellation, concurrency, errors

Stopping a turn ends the child and its process group; the next turn replays.
The `interrupt` control request would keep the child, but Sailry seals a
stopped reply as interrupted output, which the child's history would not
match.

| Condition | How it is recognized | Failure |
| --- | --- | --- |
| Not installed | The launcher reports no executable | `not_configured`: "Claude Code is not installed" |
| Not signed in | A result or exit text with "Not logged in" or "/login", or status 401 | `authentication`: "Claude Code is not signed in" |
| Refused as third-party use | Result text "Third-party apps now draw from … extra usage" | `quota`, with Anthropic's text |
| Rate limit | Status 429 or a rejected `rate_limit_event` | `rate_limited` |
| Context too long | "Prompt is too long" | `context_overflow` |
| Child ended mid-step | End of output before the step ended | `unavailable`, with the end of its error output |
| Stopped | The request's context | `cancelled` |

The content refusal can come through the real binary: magpie reports that
Anthropic judges the caller's system prompt as a whole. In interactive mode
magpie asks once again in `-p`; in `-p` there is nothing further to try, and
Sailry never retries it on its own.

## What the user needs, and the warning

Claude Code installed on the execution Node (Desktop's machine, or the Host)
and signed in there with the subscription, by running `claude` once.

Copy for the enable prompt, to be added with the provider declaration
(English; the catalogs get it then):

- Title: `Use Claude through Claude Code`
- Body: `Sailry runs the Claude Code installed on this computer with its
  sign-in. Use counts against that subscription, and Anthropic may limit or
  refuse use from other apps`
- Action: `Enable`

## Declaration and migration (proposed)

- The provider is a new official package, `claude-cli`, bundled where
  `claude-auth` is (`internal/node/plugins/catalog.go`). It declares one
  executable entry (`claude`, found on the user's PATH, never downloaded)
  and one model route on it, in the manifest shape the process layer of
  `wt/agy` defines for its ACP route. That shape does not exist yet and
  authored manifests are validated strictly, so no manifest is written in
  this pass.
- `node.newModel` gains one branch for that route kind which returns
  `claudecli.New`. Model capture needs no secret and no endpoint.
- `claude-auth` 0.1.3 and the `claude_oauth` request path stay as they are
  in this pass. Proposal: once the new route is accepted with a real
  account, hide Claude OAuth from Add provider, keep existing `claude_oauth`
  providers working behind the same warning for one release, then remove
  `model/claude.go`, its billing header and version string, and the package.
  Stored grants are left in place and never read by the new route.

## Not ported

| magpie | Why not |
| --- | --- |
| The tap | Code injection and request rewriting |
| Downloading a `claude` build | The binary is the user's own install |
| Saved accounts, setup tokens, Keychain and credential reads | No imported profile or credential |
| `--dangerously-skip-permissions`, `IS_SANDBOX` | Not needed with no built-in tools; the second is a false claim |
| `--system-prompt-file` ("short prompt") | Replaces Claude Code's prompt |
| Client cache marks, cache-TTL negotiation, history chunk marks | Sailry places no marks |
| `rewind_conversation`, session branches, `--resume` | Replay is simpler; no session files |
| WebSearch pass-through, `--json-schema`, `/usage` reading, WSL | Outside this route |

## Joining the process layer

`claudecli.Launcher` is the seam: it receives the argument list, the
environment to set and to remove, the working folder and the files to write
before start, and returns a `Child` with `Write`, `Read`, `Diagnostics` and
`Close`. After `wt/agy` merges, the adapter maps that onto the process
layer's start call (consent, bounded count, idle shutdown, process group,
scrubbed environment, confined folder). The route's own child bound and idle
timer then give way to the layer's.

## Size

| Part | Lines, without tests |
| --- | --- |
| Bridge and helper | about 350 |
| stream-json codec | about 250 |
| Route, replay, runs | about 650 |
| Process-layer adapter, declaration, `newModel` branch | about 200, not built |
| Interactive mode | about 1,500 to 2,500, not built |

## Acceptance (the user, with a real account)

1. `claude --version`, then `claude` once to confirm the sign-in.
2. Note `~/.claude.json` and `ls ~/.claude/projects` before the first run.
3. One turn without tools: text and reasoning stream; usage is recorded.
4. One turn with a file read and a command: approvals appear in Sailry, the
   results reach the model, the turn ends.
5. A second turn in the same conversation: only the new message is sent
   (cache reads in usage).
6. Edit an earlier message, compact, and restart the Node: each next turn
   answers from the replay.
7. Stop a turn mid-reply: no `claude` process remains.
8. Sign out of Claude Code: the turn fails as not signed in.
9. Compare step 2's files: record what Claude Code changed.
10. Watch for the third-party refusal and note the plan usage per turn.
