# Conversation transcript

B3 installs the controller's message text size in window-scoped Kit preferences.
User bubbles and reply Markdown use 12, 14, 16 or 18 DIPs; the default stays 14.
The transcript retains a caller-owned Markdown theme for its palette/size pair,
so shared Kit themes remain immutable and dynamic theme changes still apply.

The transcript of the selected conversation: a virtualized list of turns,
each a user message and the assistant's reply, with the jump button over
it. It matches the reference implementation `conversation/live/messages.rs`,
`live/tools.rs`, `conversation/disclosure.rs`, `conversation/surface/*` and
Kit's `MessageScroller`.

## Package and files

`internal/desktop/conversation`

| File | Holds |
|---|---|
| `model.go` | **The contract**: view models and `Actions` |
| `group.go` | Grouping rules as pure functions (`Compact`, `AnswerFrom`, `Runs`, `Sequence`, `Summary`) |
| `conversation.go` | `State`, `View`, turns, the reply's header, work region, phase row and footer |
| `tools.go` | Tool rows, groups, sequences, outputs, diffs, tables, progress, question history, subagents |
| `jump.go` | The jump-to-latest button |
| `content.go` | File cards, thumbnails (Kit's `ImageGroup`), visuals, notices, grounding, retries, compaction turns, the changes card, `Cite`, `Kind`, `FileIcon` |
| `viewer.go` | The image viewer: `Viewer` is `kit.Viewer[Image]` (zoom 0.25–8 by 1.25, pan, turn, flip, gallery and its counter) drawn by `kit.ImageViewer` |
| `actions.go` | The reply and user footers, fork and export menus, rewind confirmation, `AnswerText`, `Resendable` |
| `search.go` | ⌘F search (MyGo's `FindBar`), `Find`, `Mark`, match highlights and reveal |
| `session.go` | Loading, older pages, connection banner, diagnostic, selection pill, `Quote` |
| `usage.go` | The context bar's metrics and the composer's context ring (`UsageBar`, `ContextRing`, `Metrics`, `Count`) |
| `group_test.go`, `jump_test.go`, `live_test.go`, `bench_test.go` | Tests and benchmarks |

Shared motion (`kit.Shimmer`, `kit.Sweep`, `kit.Identicon`, `kit.EaseOutCubic`, `kit.Fast`)
is in `internal/desktop/kit/motion.go`; the thinking indicator (`kit.Indicator`:
Orbits, Globe, Ribbon, Ring) in `internal/desktop/kit/thinking.go`. Code blocks in messages and tool diffs
are MyGo's editor (`kit.CodeBlock`, `kit.Diff` in `internal/desktop/kit/code.go`).
Answers are `kit.Markdown` (`internal/desktop/kit/markdown.go`, the `Chat`
variant: 14 DIP text on 24 DIP lines, emphasis in `primary`, code on a
3.5% foreground tint) with the transcript's shared selection; a running
reply's last text block streams (`markdown.Stream`: half-written syntax
shows as it will end, text as it arrives, no paced reveal past the
60 fps cap). File previews
open as `panel.Artifact` tabs (`internal/desktop/panel/artifact.go`), subagent
sessions as `panel.Subagent` tabs that the shell fills with this `View`.

## Entry point

```go
func View(c *ui.Context, s *State, id string, turns []Turn, sess *Session, actions Actions)
```

`sess` is the session around the turns (paging, connection, diagnostic,
usage); nil is a loaded, connected session at its start. `s` is UI state (`conversation.New()`, one per window). `id` names the
conversation, so each keeps its scroll place. `turns` is backend data in
order, oldest first. The view never changes `turns`.

## View model

### Turn and Reply

| Field | Meaning |
|---|---|
| `Turn.ID` | Stable, unique across conversations; keys the list row and the disclosures' open state |
| `Turn.User` | The user's message (plain text, the right-hand bubble); `""` shows no bubble |
| `Reply.Status` | `RunStatus`: the header label and whether the run is active |
| `Reply.Elapsed` | Run time, shown after the status as `9s`, `1m05s`, `1h02m05s`; 0 hides it |
| `Reply.Phase` | What an active run does, for the phase row |
| `Reply.Waiting` | Children waited on, with `WaitingChildren` |
| `Reply.Blocks` | The reply's parts in order |

`RunStatus` and its label key: `Completed` `turn_completed`, `Queued`
`turn_queued`, `Running` `chat_running`, `Stopping` `chat_stopping`,
`Continued` `turn_continued`, `Cancelled` `turn_cancelled`, `Interrupted`
`chat_interrupted`, `Failed` `turn_failed`, `Unsynced`
`chat_status_unsynced`. `Failed` and `Interrupted` are danger. `Active()` is
Queued, Running, Stopping or Unsynced.

`Phase` and its label key: `Thinking` (and `PhaseNone`) `turn_thinking`,
`Generating` `turn_generating`, `AwaitingResponse` `turn_awaiting_response`,
`Compacting` `turn_compacting`, `WaitingChildren` `turn_waiting_child` /
`turn_waiting_children`.

### Block

| Kind | Uses | Renders |
|---|---|---|
| `TextBlock` | `Text` (Markdown) | The answer; blank text is dropped |
| `ReasoningBlock` | `Text` | A `turn_reasoning` disclosure, body at most 160 high |
| `ToolsBlock` | `Calls` | Rows, groups and sequences (below) |
| `QuestionBlock` | `Question` | The question's history row |
| `ChildrenBlock` | `Children` | Subagent rows |
| `ProgressBlock` | `Steps` | A `task_progress` disclosure with `done/total` |
| `ErrorBlock` | `Label`, `Text` | A danger `TriangleAlert` disclosure, body selectable, at most 160 high |

### Call

| Field | Meaning |
|---|---|
| `ID` | Unique across the dataset; keys the row's open state |
| `Name` | Tool name; consecutive calls of one name group |
| `Label` / `LabelKey` | The tool's label from its declaration, or the i18n key of a built-in tool's (`tool_load_skill`, `tool_list_sessions`, `tool_read_session`, `tool_search_session`, `tool_find_tools`, `tool_read_output`) |
| `Icon` | An `icons.ByName` name; unknown or `""` is `settings-2` |
| `Detail`, `Code` | The summary line; `Code` draws it mono, at most 480 wide |
| `Error` | Replaces the detail while `Failed` or `Warning`, unless `Denied`; never mono |
| `State` | State text after the detail: `Denied` `approval_rejected`, `StateInterrupted` `tool_interrupted`, `NotExecuted` `tool_not_executed`; `StateFailed` and `Pending` show none |
| `Failed` | Runtime failure: danger |
| `Warning` | The tool's own failure, or denied, interrupted, not executed: warning |
| `Pending` | Awaiting approval: warning, `help-circle` icon |
| `Running` | Running: label and detail shimmer under one band across the row (`kit.Sweep`, in row coordinates, the icon excluded, each text in its own font and colour; 60 fps, paint only, still with Reduce Motion), colour `muted_foreground` mixed 35% toward `foreground` |
| `Standalone` | Never groups with neighbours |
| `Terminal` | An interaction (`ask_user`, plan review, MCP elicitation): not work, never groups |
| `Output` | What opening the row shows; `nil` rows have no caret |

### Output

| Field | Renders |
|---|---|
| `Text` | Mono, muted, 20 DIP lines, 100 high, scrolls; a hover-only copy button (`tool_copy`) |
| `Command` | Keep the last 100 lines of `Text`, with `tool_output_earlier` above |
| `Paths` | Two or more, or one other than the detail: the group lists unique paths as `file-text` mono rows |
| `Diff` | Outlined box, radius 15: 32 DIP header with the file name and `+N` (success) / `−M` (danger, U+2212), 1–5 rows of 13 × 1.6 DIPs |
| `Table` | 180 DIP columns, 1–5 rows in view, 28 DIP rows |
| `Failure`, `ToolFailure` | A notice tinted danger, or warning with `ToolFailure` |
| `Notices` | Muted notices (`px 12`, `py 8`, 14 DIPs) |
| `Partial` | The `tool_partial` notice |

### Question, Child, Step

- `Question{CallID, Prompt, Options, Multi, State, Answers}`. History row
  icon: Pending `bot` (shown as `help-circle` since it is pending),
  Cancelled `circle-x`, Interrupted and TimedOut `triangle-alert`, else
  `circle-check`. State text: Pending `question_pending`, Accepted
  `question_accepted`, Unconsumed `question_unconsumed`, Declined
  `question_declined`, Interrupted `question_interrupted`, TimedOut
  `chat_response_timeout`; Answered and Cancelled show none. The prompt
  wraps. Answers make it a disclosure.
- `Child{ID, Title, Model, Effort, Status}`. One child is a row: a 16 DIP
  identicon (coloured and turning while running), title (`chat_child` when
  empty), `Model Effort` muted at 0.65. Two or more: a disclosure of the
  identicons and `subagent_count`, body indented 16. Failed and interrupted
  are danger.
- `Step{Text, State}`: Pending `circle-check` muted, InProgress
  `loader-circle` (shimmering while the run is active), Completed
  `circle-check` success + strikethrough, Skipped `minus` + strikethrough,
  Paused `pause` warning, Blocked `triangle-alert` warning, Failed
  `circle-x` danger. Rows 32 high, list at most 320.

## Tools

The rules are `group.go`, tested in `group_test.go`.

1. **Blocks** (`Compact`): blank text goes; each call becomes a block, and
   adjacent tool blocks join while both ends are groupable (not `Terminal`,
   not `Standalone`); text splits them. Adjacent children blocks join.
2. **Work region** (`AnswerFrom`): everything before the first block after
   the last work block folds under the header. Work is any block but text,
   questions and tool blocks of `Terminal` calls only. The header is a
   full-width muted trigger (status, elapsed, a 16 DIP chevron shown on
   hover) above a separator; without work it is the status alone. It is open
   while the run is active and closed after, until the user toggles it.
   Blocks inside are 16 apart.
3. **Sequence** (`Sequence`): a block of more than one call with more than
   one name goes under one closed disclosure, headed by `Summary`: the last
   call's label and detail with the flags of all (pending and running from
   any; failed and warning only once nothing runs; failed hides warning; no
   state text while active). The caret always shows on hover; no count.
4. **Groups** (`Runs`): runs of one name. A single call without a path list
   is a row. Otherwise a group disclosure headed by `Summary`; a single call
   without output is a plain summary. The body is the path list or the rows.
5. **Rows**: a 24 DIP button, `gap 8`, icon 16, label nowrap, detail
   truncated, state text, caret 12 shown on hover. Without output, a plain
   row with no caret. Hover turns quiet rows (not failed, pending or
   warning) to `foreground`.
6. **Rail**: open bodies hang from a 1 DIP `border` line at 7.5 DIPs, padded
   15.5 so they start under the label; sequence and group bodies pad 8 top
   and bottom, single rows 4 and 8.
7. **Opening is instant**, as in the reference (Kit `Collapsible` without a
   motion ID). Everything starts closed.
8. **Continuing**: while the run is active the last tool row of the last
   block is drawn running, so the shimmer stays on the latest activity.
9. **Phase row**: while active, the orb and the phase label, the band in the row's coordinates;
   hidden while a call is pending or running, a child runs, or a question
   waits.
10. **Footer** (ended runs): copy the answer (`turn_copy`, toast
    `content_copied`), and retry (`chat_retry`) for Failed, Interrupted and
    Cancelled runs.

Approvals and the pending question are answered in the composer: see
[composer.md](composer.md#interactions). Their tool rows show here as
pending.

## Jump to latest

Kit `MessageScroller`, in `jump.go`:

- **Shows** while the list is laid out and not at its end. There is no
  threshold: any scroll up stops the list following its end (MyGo
  `ListState.FollowEnd`, as Kit's follow mode).
- **Hides** at the end (within 0.5 DIP, MyGo's `AtEnd`).
- **Look**: 32 × 32, fully round, 1 DIP `border`, `background` fill,
  `foreground` `ArrowDown` 16; tooltip and accessibility label
  `turn_latest`; centred, absolute over the list.
- **Motion**: a 200 ms ease-out cubic `v` (MyGo `AnimateWith`): opacity `v`,
  bottom `8 + 8v`. While fading out it is disabled. Reduce Motion makes it
  instant.
- **Click**: `ScrollToEnd`, at once (no `FollowAnimation`), which follows the
  end again. Sending a message does the same.
- **Streaming**: while following, a growing reply keeps the end in view; a
  user who scrolled up stays where they are.
- The list's role is `Log`.

### Live features (phase 4a, provisional fields)

| Type / field | Shows |
|---|---|
| `Turn.Sent`, `Reply.Finished` | Hover times in the user and reply footers |
| `Turn.Images`, `Turn.Files` | Attachments under the bubble: thumbnails (viewer on click) and file cards |
| `Turn.Notices` (`Notice`: Title, Agent, Outcome) | Background results bubble, outcome tone danger/warning/muted |
| `Turn.Compaction` (`State`, `Summary`, `Cause`) | A compaction turn: centered status (shimmer while running), summary or failure disclosure |
| `Reply.Citations` (`URI`, `Title`, `End` in runes) | Numbered links placed in the answer's last text block (`Cite`) |
| `Reply.Changes` (`Files`, `Undone`) | Changes card: count, totals, Undo/Review, 3 files then Show N more; per-file Review blocks and Undo on hover |
| `Reply.Notice` | A notice-only run: no reply frame |
| `ImagesBlock` / `Block.Images` | Thumbnails (`kit.Image`) up to 240 DIPs, wrapping; a Skeleton while `Source` is nil |
| `FilesBlock` / `Block.Files` (`File`) | File cards; a document with `Image` shows it on top |
| `SearchBlock` / `Block.Sources` | Globe "Search results" disclosure listing the sources |
| `RetryBlock` / `Block.Retry` (`Attempt`, `Limit`, `Reason` key, `Detail`, `Exhausted`) | Network row, shimmer while active, the provider's message when opened |
| `VisualBlock` / `Block.Visual` (`Title`, `Mermaid`) | The chart or diagram on the group box: limu (`github.com/firede/limu` and `limumygo` v0.5.0) lays the Mermaid source out to the card's width in the app's fonts, cached per source and width, and paints it in the palette (`ThemeOnly`); a source limu cannot read shows as code |
| `File` (`Path`, `Mime`, `Kind`, `Text`, `Image`, `State`, `Editable`) | Card label/kind, the preview tab's content or state |
| `Session.Older`, `LoadingOlder`, `OlderFailed` | Row before the first turn: loader (reaching it calls `LoadOlder`), failure with Retry |
| `Session.Loading` | Loader and "Loading conversation" instead of the list |
| `Session.Connection` | Banner: Reconnecting (shimmer) or Host disconnected, with Retry |
| `Session.Diagnostic` (`Title`, `Fields`, `Detail`) | Failure card after the last turn, Copy |
| `Session.Usage` | Composer context bar: tokens, speed, cost, cache, turns (hover cards) and the context ring |

UI-only state: the image viewer, the search bar (query, matches, index),
the rewind waiting for confirmation, the turn read aloud, disclosures and
"Show N more".

## Actions

```go
type Actions interface {
	OpenChild(id string)
	Retry(turn string)
	Fork(turn string)
	Rewind(turn string) // after the confirmation dialog
	Worktree(turn string)
	Export(turn string, format ExportFormat) // "" exports the conversation
	Read(turn, text string)                  // read aloud / stop
	Edit(turn string)                        // edit a sent message
	OpenFile(f File)                         // preview in the right panel
	FileAction(f File, action FileAction)    // open elsewhere, save, request edit
	Undo(turn, path string)                  // "" undoes all
	Review(turn, path string)
	LoadOlder()
	Reconnect()
	Quote(text string) // the selection, quoted, into the draft
}
```

| Method | Fires | Expected outcome |
|---|---|---|
| `OpenChild` | Click on a subagent row | Open the child session (the preview opens a `panel.Subagent` tab) |
| `Retry` | Retry on a Failed, Interrupted or Cancelled run | Submit the turn again; conflicts and uncertain outcomes toast |
| `Fork`, `Worktree` | Fork menu (Network icon) | New session / worktree from the turn |
| `Rewind` | Fork menu → Rewind, then the dialog's Rewind | Keep the turn, drop later messages; files unchanged |
| `Export` | More menu: Markdown, HTML, Image | Save the turn |
| `Read` | Read aloud / Stop reading | Start or stop the reader |
| `Edit` | User footer Edit | Edit the message in place |
| `OpenFile` | Card Open, More → Preview, document picture | Preview tab in the right panel; an `OtherFile` (PDF, office) opens in the system's app instead |
| `FileAction` | More → Open, Save as, Request edit | As named; the shell's one `openSystem` opens files in the system's app |
| `ImageAction` | Image viewer Copy, Save as, Open | Copy the picture, save it, open it in the system's app (the preview toasts `preview_notice`) |
| `Undo`, `Review` | Changes card | Revert / review the turn's changes |
| `LoadOlder` | The start comes into view, or Retry after a failure | Load the page before; set `LoadingOlder` while it loads |
| `Reconnect` | Banner Retry | Reconnect |
| `Quote` | Selection pill Quote | Append the quote to the draft (the shell does) |

Copy (answer, user message, selection, diagnostic) and the image viewer,
search and selection are the view's own. The preview toasts
`preview_notice` for every action that would run a model or tool, change
a session or write a file; `OpenFile`, `OpenChild`, `LoadOlder` and `Quote`
work on preview data.

### Keys

| Key | Scope | Action |
|---|---|---|
| ⌘F | window, while the conversation shows and the panel has no focus | Open or focus the search bar |
| Enter / ⌘G, ⇧Enter / ⇧⌘G | search field | Next / previous match, revealed |
| Esc | search bar | Close it |
| ← → + (= or ⇧=) − 0, Esc | image viewer (preview.rs `key`) | Previous, next, zoom in, out, fit; close. The wheel and a pinch zoom, dragging pans |

## Wiring a backend

`internal/client/conversation*.go` owns retained per-session canonical projections,
subscription generations, cursor recovery, history paging, search and tool detail
caches. `WatchConversation` shares one subscription across windows and panes;
its release is idempotent. Cursor gaps reopen observation without replaying a
business command. Loaded extents survive reconnect. History loads twenty logical
turns, completing missing large turns in 500-entry continuations first. Older
reads wait for their source sequence and retain newer live metadata. Only the
authoritative Node session list confirms removal.

`internal/desktop/live` maps this state through `Conversation`, then supplies the
unchanged transcript `Actions` on each visible main or split pane. Joined guidance
uses its parent row while canonical ordering remains intact. Reasoning stays in
canonical history and has no live row. Tool row identity is canonical entry/index;
ambiguous provider call IDs cannot attach results to another call. Runtime
failures, returned tool errors, pending/denied/interrupted calls, progress,
question history, notices, usage and compaction map into the accepted DTOs.

`Call.Load` requests revision-scoped details when a disclosure opens. Client
validates 64 KiB pages and limits transfers and retained allocations to 64 MiB;
the cache also holds at most 64 entries. The same cache serves all windows.
Typed text, tables and diffs fill existing output controls. Closing and reopening
a failed detail disclosure retries its read.

The accepted FindBar delegates live scans to `SearchConversation`; it loads
twenty results per page and reveals an exact canonical target through Client
history loading. Preview keeps the local `Find` implementation. Search cancellation,
query generations and history revisions prevent stale callbacks from moving the
current selection. `SetReadAloud`, `SetReaderActive` and `SetExport` connect the
application reader/export services. Speech completion controls the Read/Stop
button; a missing service reports unavailable.

Automatic handoffs use `CompactionBlock` and the same summary disclosure as manual
compaction, so the handoff is excluded from answer copying and speech. Current
draft text, reasoning and compacting resources determine the phase independently
of older work blocks. The latest failed run supplies the diagnostic card, and
summary context estimates use their captured budget limit when reported context
usage is absent.

Sent context/session mentions retain their canonical target bindings in private
transcript state through `SetReferences`; inline MyGo text opens the bound target.
Editing restores the same references before `ReplaceTurn`. Session and host
mentions navigate existing views. Project and worktree mentions use the shared
read-only details dialog for the turn's captured location; dedicated workspace
resource navigation remains with B5. No `Turn` field or `Actions` signature changes
are required for these view-only bindings.

Delegated rows retain their canonical parent turn and tool entry. Shared Client
helpers select the latest relevant child run within that owner, preferring active
work to queued continuations. The waiting phase appears only after the parent's
tool batch is committed and while its foreground wave remains unsettled; streamed
output, consent and pending guidance take precedence. Background children do not
hold this phase.

Child tabs subscribe to the real registered child session through the same Client
cache. Their transcript and composer are read-only. The parent's composer presents
ordinary child approvals and resolves them with the child's session identity.
Background notices use committed delivery evidence; their canonical untrusted
input is hidden from the ordinary user text while the notice bubble retains each
task title and outcome.

### B7 remote sessions

The same Client projection serves in-process and iroh transport. A visible
conversation retains its execution Node's bound Runtime for history, search,
tool details, attachments, commands and all asynchronous actions. Transport
connection changes feed the existing Reconnecting/Host disconnected banner.
Retry refreshes that Client and its retained conversation observation; it does
not resubmit a turn or select the controller's local Node.

A revoked or removed execution peer leaves captured actions unavailable. Explicit
Node ownership never falls back to the local Client, including when another
Node has a resource with the same ID. Retained drafts stay read-only until their
execution owner is available; their content and captured project Host are kept.

The application export callback resolves and freezes the session's Client before
opening the native save panel. Title, committed history, images and attachment
bytes all use that exact owner even if navigation changes while the panel is
open. Read aloud remains a controller-native effect. Opening a remote session
in another window keeps its requested selection until remote data arrives.
These routes use additive Node identity without rewriting protocol SessionIDs.

## UI state vs backend data

| UI state (`State`) | Backend data |
|---|---|
| Scroll place per conversation, follow mode | Turns, blocks, calls, outputs |
| Disclosures opened, image viewer, search, rewind confirmation | Run status, phase, elapsed, files, pictures, changes, citations |
| Work regions toggled, open child tabs | Questions, registered children, frozen models, background notices, steps |
| Markdown options | |
| Native save-panel destination and pending window selection | Captured execution Client, connection state and verified remote resources |

## Preview

`internal/desktop/preview/turns.go` has a scene for each case: sequence with a path
list (explore), diff and long command output (edit), runtime and tool
failures with a turn error (failed), denied, interrupted and not executed
(refused), built-in tools with a table, notices and partial (sessions),
question states (questions), one and three subagents (delegated), every
step state (planned), a plain answer (answered), and a live reply with
running and pending calls, running children and a step in progress (live).
`content.go` adds attachments (two pictures and a file), artifacts (a file
of every kind and preview state, generated pictures, a drawn chart),
grounded (sources and citations), retried (a retry, an exhausted retry
with the provider's message, the turn error), changed (five files, one
undone), noticed (three background outcomes) and compacted (summary,
failure, unneeded) scenes; times and usage for every conversation; and
three unassigned conversations for the session states: "Long history,
paged" (8 turns shown, 56 more 8 at a time after 0.6 s), "Connection
recovering" (banner and diagnostic) and "History loading".
`stream.go` plays a canned reply after a send on the frame clock: thinking,
a running read, its output, then the answer growing a character at a time.

## Tests

- `group_test.go`: `Compact`, `Runs`/`Sequence`, `Summary` flags,
  `AnswerFrom`, the last 100 lines, `Elapsed`.
- `jump_test.go`: appears on the smallest scroll and hides at the end;
  click scrolls to the end; stays pinned while streaming, stays put once
  scrolled up, and follows again after the click.
- `live_test.go`: `Cite`, `Mark` outside code, `Find`, the viewer's zoom
  limits and steps, `Quote`, `Count`; every scene's blocks in en/zh-CN and
  light/dark (snapshots to `$SAILRY_SNAPSHOTS`); a card's Open; the search
  bar's count, Enter and Escape; the start asking for older turns.
- `bench_test.go`: 1000 turns scroll; `BenchmarkScroll`, `Steady`, `Jump`.
- `internal/desktop/shell/shell_test.go`: send, stop, the reply playing to
  Completed, approvals and the question.

## Deviations

| Deviation | Reason |
|---|---|
| Reasoning is shown as a disclosure; live mode drops it | Kept for the preview to show the block; drop it in `project` to match |
| The jump fade runs at the display's rate | MyGo element transitions have no frame-rate parameter; only drawings take `AnimationFrameRate` |
| The in-progress step's `loader-circle` does not spin | A spinning icon would be an `Element.Loop` at the display's rate; the shimmer on its text marks it instead |
| PDF and other files without a native preview open no tab: Open hands them to the system's app (a missing file toasts `files_open_failed`) | MyGo v0.8.3 has no PDF or office view; a WebView preview comes later (docs/panel.md) |
| The image viewer zooms with its buttons and + − 0, not the wheel or pinch | MyGo elements report no wheel or pinch deltas; drag pans |
| Search is MyGo's find bar over the transcript ("1 of N", previous, next, Done) | The accepted layout; live reads Node history and loads pages through an exact match, while preview searches its loaded fixtures |
| Search marks the answer's text blocks and the user bubble, all matches alike in the answer | Markdown marks with `<mark>` (one color); the bubble's current match is stronger |
| The selection pill sits at the turn's top end | MyGo reports the selection's text, not its bounds |
| Citations are placed in the answer's last text block | Rust offsets them across the text parts of the run |
| Thinking figures (Orbits, Globe, Ribbon, Ring) approximate Rust's dot drawings | Drawn on the painter at 60 fps from the same states, not ported point for point |
| ⌘F is a window shortcut, not an entry in the native menu | The shell's menu has no conversation commands yet |
| Subagent tabs show Bot, not the identicon | `panel.look` returns an icon; the identicon is a drawing |
| Older pages load when the first turn is built (near the start) | Rust asks on scrolling near the top; the list builds rows just outside the view |
| Copy confirms with a toast, not Kit `Clipboard`'s check icon | Sailry's rule: outcomes show as toasts |
| Shimmer and identicon are approximations | Custom drawing (MyGo has no moving text highlight or procedural avatar); same timing and colours, not pixel-identical |
| Diff rows have no syntax highlighting or line numbers | Not in the view model yet |
| MCP forms and authorization links use the existing question controls | The Node binds each request to its exact active parent call and persists its answer without adding a second transcript row |
| A jump across the transcript (scroll bar drag) costs ≈4.1 ms a frame, against ≈1.8 ms before tool rows | MyGo's `ListState.AtEnd` builds another frame whenever the rows in view change, not only the end; a jump re-measures rows until they settle. Steady (≈0.19 ms), hover (≈0.19 ms) and scroll (≈1.0 ms) frames are unaffected |

Workspace tool details replace successful single-file edit fragments with their
saved checkpoint diff. This projection uses the original turn/call identity and
never rereads the working tree. Change sets, notebooks and failed calls retain
source output. Full tool details preserve original output bytes; compact diff
rows count actual changes, excluding unified headers and hunk metadata.
File approvals show the Node-validated change preview with rename targets,
counts and truncation, plus localized captured approval notices. These are
projected independently of the tool output so consent precedes physical effects.

Installed script tools wrap the captured native operation owner. Their bounded,
read-only prepare and result steps preserve native approval previews, sandbox
selection, request identity, checkpoints and receipts. Delegation binds to the
existing child execution owner. MCP tools use the official Go SDK pool and retain
their original content, structured result and error status in canonical history.
An unavailable server contributes a bounded safe startup instruction and leaves
the remaining tools available; cancellation and uncertain cleanup remain terminal.

MCP form and URL elicitation uses the canonical question owner. Concurrent
questions keep distinct parent identities and decision wakes. Only active
elicitation time pauses tool timing. Closing a turn joins its tool resources
before settlement; failed cleanup retains `OutcomeUnknown`.
