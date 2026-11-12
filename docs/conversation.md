# Conversation transcript

B3 installs the controller's message text size in window-scoped Kit preferences.
The transcript's content text follows it, 12 to 18 DIPs with the default 14
(`textSize` in `tools.go`): user bubbles, reply Markdown, the status header, the
tool, question, subagent, step and phase rows and what they open, notices,
sources, compaction, the changes card's texts, captions, a chart's axis labels
and legend, and the footer's time.
Every measure in this document is the one at 14. A row is `size+10` high, its
icon `size+2` and its caret `size-2`; text a DIP or two smaller than the body
keeps that difference. Controls keep Kit's sizes: buttons, menus, popovers,
dialogs, tooltips, the banner, the welcome page and the composer's context bar
and task capsule (`Steps`).
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
| `content.go` | File cards, thumbnails (Kit's `ImageGroup`), notices, grounding, retries, compaction turns, continuation markers, the changes card, `Cite`, `Kind`, `FileIcon` |
| `visuals.go` | Mermaid in a reply: the `mermaid` code block and `VisualBlock`, the parse cache, CrychicUI's picture (`diagram.Mermaid`) scaled to the reply, the caption |
| `chart.go` | Data charts drawn natively from the model limu's parser reads: xychart (bars, lines, both orientations) and pie, their checks, hover and keyboard pick, tips and legends |
| `viewer.go` | The image viewer: `Viewer` is `kit.Viewer[Image]` (zoom 0.25–8 by 1.25, pan, turn, flip, gallery and its counter) drawn by `kit.ImageViewer` |
| `actions.go` | The reply and user footers, fork and export menus, rewind confirmation, `AnswerText`, `Resendable` |
| `edit.go` | The inline editor of a sent message |
| `embed.go` | File and image cards from a reply's Markdown, the session link icon |
| `search.go` | ⌘F search (MyGo's `FindBar`), `Find`, `Mark`, match highlights and reveal |
| `session.go` | Loading, older pages, connection banner, diagnostic, selection pill, `Quote` |
| `usage.go` | The context bar's figures with their hover cards and compact panel, and the composer's context ring (`Metric`, `Row`, `UsageBar`, `Metrics`, `ContextRing`, `Count`) |
| `group_test.go`, `jump_test.go`, `live_test.go`, `bench_test.go` | Tests and benchmarks |

Shared motion (`kit.Shimmer`, `kit.Sweep`, `kit.Identicon`, `kit.EaseOutCubic`, `kit.Fast`)
is in `internal/desktop/kit/motion.go`; the thinking indicator (`kit.Indicator`:
the loading marks: Loop while thinking or waiting for the response, Chevrons
while replying, Swirl while tools or subagents run, Trace while queued, a
paused Pulse for no known phase) in `internal/desktop/kit/thinking.go`. Code blocks in messages and tool diffs
are MyGo's editor (`kit.CodeBlock`, `kit.Diff` in `internal/desktop/kit/code.go`);
a block's header has its language and Copy as an icon button (`content_copy`,
toast `content_copied`).
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
| `Turn.ID` | Stable, unique across conversations; keys the list row and the disclosures' open state, which `State` keeps so a disclosure stays as it was left when its row scrolls out of the list and back |
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
| `Background` | The command the call sent to the background (`Running`, `Outcome`, `Exit`, `Waiting`), nil for a call that ran in place. The row says its state after the detail, where no other state text shows: `background_running`; or the outcome's `background_outcome_*` label, `background_exit` with the code for a command that exited non-zero; then ` · ` and `background_result_waiting` while no turn has taken the result. Failed, timed out, interrupted and unconfirmed outcomes take the warning color; running, completed, stopped and cancelled read plain. A group's header carries its last call's, and none while the group is at work |

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
  wraps. Answers make it a disclosure. A plan review's `Prompt` is
  `composer_mode_plan` and its answer the plan text; a form's answers are
  `Title: value` lines with option titles and `question_yes`/`question_no`.
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
9. **Phase row**: while active, the phase's mark (`kit.Indicator`, 16, muted) and the phase label, the band in the row's coordinates;
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
| `Turn.Marker` (`Label`, `Locales`) | A turn a package continued ([goal](goal.md)): the package's label in the window's language between two lines, one row high (`rowHeight`), in place of the user message; `turn_continued` when the package declared none. The continuation's prompt is never shown; guidance joined into the turn shows as the user's message under the marker |
| `Reply.Citations` (`URI`, `Title`, `End` in runes) | Numbered links placed in the answer's last text block (`Cite`) |
| `Reply.Changes` (`Files`, `Undone`) | Changes card: count, totals, Undo/Review, 3 files then Show N more; per-file Review blocks and Undo on hover |
| `Reply.Notice` | A notice-only run: no reply frame |
| `ImagesBlock` / `Block.Images` | Thumbnails (`kit.Image`) up to 240 DIPs, wrapping; a Skeleton while `Source` is nil |
| `FilesBlock` / `Block.Files` (`File`) | File cards; a document with `Image` shows it on top |
| `SearchBlock` / `Block.Sources`, `Block.Suggestions` | Globe "Search results" row; it opens the result dialog: the answer as Markdown, the numbered sources as link rows, one a line, and the links of the provider's suggestions. Only http(s) links without credentials open, as a reply's links do ([Links and files](#links-and-files)); an internal open closes the dialog over the panel |
| `RetryBlock` / `Block.Retry` (`Attempt`, `Limit`, `Reason` key, `Detail`, `Exhausted`, `Stopped`) | Network row, shimmer while active, the provider's message when opened; a stopped recovery is a warning row with its reason after the label |
| `Turn.Editing` (`Text`, `Pending`) | The user bubble is its editor (`edit.go`): a field of 1 to 12 lines, Cancel and Save. Enter or Save calls `SaveEdit`; Escape, Cancel or the focus leaving the editor calls `CancelEdit`. While `Pending` the field is read-only, Save and Cancel are disabled and a loader shows |
| `Turn.Controls` (`ReadOnly`, `NoEdit`, `NoFork`, `NoRewind`, `NoWorktree`, `NoExport`, `Busy`) | `ReadOnly` hides Edit, Retry and the fork menu; the `No…` fields disable their control or menu item; `Busy` replaces the fork button with a loader while a fork or rewind of the turn is in flight |
| `Output.Images` | A tool result's pictures as thumbnails under its output; one opens the image viewer |
| `Image.Missing` | A picture that is gone or past the limits shows `chat_image_missing` instead of a frame |
| `State.Embed` | Asked for a link or images standing alone in a paragraph of a reply (`embed.go`): a file card, thumbnails, the missing line, or a link button; web images and links inside a sentence stay Markdown's |
| `State.OpenLink` | Takes every link and file path of the content ([Links and files](#links-and-files)); without it a heading link scrolls, an address `ui.SafeToOpen` allows opens in the system, and tool rows' paths are not links |
| `State.SessionLink` | A link that opens a session carries the Bot icon before its text |
| `Compaction.Elapsed`, `CompactionInterrupted` | The compaction turn's label carries its duration; an interrupted one reads `chat_compact_interrupted` |
| `Reply.PhaseElapsed` | The Compacting phase row carries how long it has run |
| `File.Document`, `Exportable`, `Interactive` | A published document's More menu adds Export HTML and, unless it is an interactive preview, Export image; both disabled while `Exportable` is false |
| `VisualBlock` / `Block.Visual` (`Title`, `Mermaid`) | The chart or diagram in line with the reply's text, with no card: `Title` is one small muted caption above it and its accessible name. An xychart or pie is drawn natively and any other diagram by CrychicUI's `diagram.Mermaid` (on `github.com/Broderick-Westrope/pele-go` v0.2.0), as [B10 visuals](#b10-visuals) describes; a source that cannot be drawn shows as code |
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
	Edit(turn string)                        // open a sent message's editor
	SaveEdit(turn, text string)              // replace the turn with text
	CancelEdit(turn string)                  // close the editor
	Start(text string)                       // a new conversation quoting text
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
| `Edit` | User footer Edit | Set `Turn.Editing`; the button is disabled where the edit cannot apply, so a refusal says nothing |
| `SaveEdit` | The editor's Save or Enter | Replace the turn (`replace_turn`); the editor stays, pending, until the Node accepts, and a failure, conflict or unknown outcome toasts and keeps it |
| `CancelEdit` | The editor's Cancel, Escape or blur | Clear `Turn.Editing`; ignored while pending |
| `Start` | Selection pill New conversation | Open the unsent conversation of the same project with the quote in its draft |
| `OpenFile` | Card Open, More → Preview, document picture | Preview tab in the right panel; an `OtherFile` (PDF, office) opens in the system's app instead |
| `FileAction` | More → Open, Save as, Request edit, Export HTML, Export image | As named; the exports save the document's captured revision (`<title>-v<revision>.html` or `.png`) where the user chooses; the shell's one `openSystem` opens files in the system's app |
| `ImageAction` | Image viewer Copy, Save as, Open | Copy the picture, save it, open it in the system's app (the preview toasts `preview_notice`) |
| `Undo`, `Review` | Changes card | Revert / review the turn's changes |
| `LoadOlder` | The start comes into view, or Retry after a failure | Load the page before; set `LoadingOlder` while it loads |
| `Reconnect` | Banner Retry | Reconnect |
| `Quote` | Selection pill Quote | Append the quote to the draft (the shell does) |

Copy (answer, user message, selection, diagnostic) and the image viewer,
search and selection are the view's own. The preview toasts
`preview_notice` for every action that would run a model or tool, change
a session or write a file; `OpenFile`, `OpenChild`, `LoadOlder` and `Quote`
work on preview data. Demo mode (docs/shell.md) uses the same actions over
the demo's conversations and shows no toast for them.

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
unchanged transcript `Actions` on each visible main or split pane. A frame reads
the Node snapshot once per `Client.Revision` and each visible conversation once,
and shares both copies read-only. A change in a watched conversation redraws its
panes without projecting the Node snapshot again. Joined guidance
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
usage is absent. A failed statistics refresh (the `statistics_failed` change,
`client.ConversationState.StatisticsErr`) is the error toast
`usage_read_failed`, once when the failure begins for the shown conversation
(`reportUsage`), as in the reference.

A compaction that ends without a summary (`engine/compaction.go`) saves, after
its billed attempts, one system record whose `metadata.compaction.failure`
names the source (`manual` or `automatic`), the stage (`budget`, `request`,
`summary`, `history`), the reason, the HTTP status of the last attempt and the
cause; a stopped turn records none. The page carries it as the resource
`{"type":"compaction_failure","failure":{…}}`, statistics report it as the last
compaction with outcome `failed`, and the run still fails with the same error.
The transcript shows it as the error block `chat_compact_failed` with the
cause, and then omits the run's own error block when it repeats that cause, so
the failure reads once; the diagnostic card is unchanged. A manual compaction
turn keeps its failed state and cause from the run. A completed compaction run
whose loaded turn holds no summary shows `chat_compact_unneeded`; a turn whose
entries are not loaded keeps `chat_compacted`.

Differences from the reference, deliberate: the reference ends the turn
normally with an English assistant message ("Ran into this error trying to
compact: … Please try again or create a new session") and never writes the
failure record its statistics, notifications and queue read. Go fails the run
and writes the record, so each client words the notice in its own language. The
reference's check that pauses the queue for a turn holding a failure that did
not continue is not ported: the queue treats that turn as any other failed run.

An answer the model's output limit cut short says so once. The engine records
`metadata.runtime.output_limit_reached` on the reply; the page carries the
resource `{"type":"output_limit"}` after that reply's parts when it requested no
tool, which is the reply that ends its turn (a cut reply that requested tools
goes on to their results and shows nothing). The transcript shows it as
`LimitBlock`, one plain warning row `chat_output_limit` below the cut text, and
keeps it with the answer rather than the work region; exports and the mobile
transcript likewise keep the text before it as the answer and show no row. The
reference stores the flag and shows nothing.

Sent context/session mentions retain their canonical target bindings in private
transcript state through `SetReferences`; inline MyGo text opens the bound target.
A sent message is edited in its bubble. `Edit` captures the turn, the head
turn, the history revision and what the turn sent in the adapter's
`conversationEdits`; `projectControls` shows it as `Turn.Editing`. Only
`SaveEdit` sends `replace_turn`, with the edited text, the sent attachments and
the sent references (a reference written as `@label` in the sent text is
dropped once the new text no longer names it), so the composer's draft is never
read or changed and an abandoned editor cannot turn a later send into a
replacement. The editor closes on Escape, Cancel, blur, when its turn leaves the
history or when its conversation leaves the screen. A rejected replacement
keeps the editor and its text and toasts the failure, the conflict or the
unknown outcome.

`pageControls` follows the reference's enablement. A delegated (child) or
unowned session is read-only: no Edit, Retry or fork menu. Otherwise every
action waits for a connected Node and for no fork, rewind or replacement in
flight; Edit also needs an idle conversation and a turn that sent something;
Fork and New worktree need the turn's run ended (New worktree also a movable
workspace); Rewind needs an idle conversation and a turn before the head. The
export menu is disabled while disconnected.

A rewind reports `chat_rewound`; when the Node saved the removed history, the
toast's `chat_rewind_backup` action opens that backup session. A send that
fails after its text left the composer reports with a `chat_restore_draft`
action, which puts the submission back into an empty composer
(`queue_draft_present` when it is not empty, `chat_restore_unavailable` on a
read-only conversation).

### Links and files

One rule for every link and file of a conversation's content: **a click opens
it in the right panel of that conversation, a click with the platform modifier
(Command on macOS, Control elsewhere) opens it with the system's default**. The
transcript reads the modifier at the click (`conversation.External`) and hands
the link to `State.OpenLink`; live answers with `openChatLink`
(`conversation_links.go`), sample data with `App.sampleLink` (`shell/demo.go`).

| Target | Click | Modifier click | Rust (`conversation/links.rs`, `shell/links.rs`) |
|---|---|---|---|
| `http(s)` address with a host and no credentials (a `www.` host gains `https://`) | A new browser tab of the right panel, the panel opening | The system browser (also on a plain click when the browser pane is unavailable) | Same |
| `http(s)` address with credentials or no host | Nothing | Nothing | Routed to the browser, which refuses it |
| `mailto:`, `tel:` | The system | The system | Same |
| `sailry://session/<id>` | That session; a child in its panel tab; `unavailable` toast for one unknown | Same | Same |
| `sailry-knowledge:` | The knowledge source | Same | Same |
| Workspace file (`path`, `path#L12`, `path:12`, `path:12:3`, `file:` forms), resolved against the session's worktree | The file tab of the right panel at its line; a file with a preview kind in its preview tab | On this device, the file's own app; on another Node, as a click | Same |
| Workspace folder (`./`, a trailing `/`) | The file manager on this device; `files_remote_path` elsewhere | Same | Same |
| Path outside the worktree | The system on this device; `files_remote_path` elsewhere | Same | Same |
| `#heading` | Scrolls the reply | Same | Same |
| Other schemes (`javascript:` and the rest) | Resolved as a path, which fails with `files_open_failed` | Same | Same |

Where the rule applies, all through `State.follow` or the file actions:

| Content | Click | Modifier click |
|---|---|---|
| Reply Markdown links, autolinks and citation numbers (`Options.OnLink`) | `OpenLink` | `OpenLink`, external |
| The link button of an image that is no file (`embed.go`) | `OpenLink` | `OpenLink`, external |
| Search result dialog: the answer's links and the source and suggestion rows | `OpenLink`, then the dialog closes | `OpenLink`, external; the dialog stays |
| File card: Open and the picture on top | `Actions.OpenFile` (the preview tab) | `Actions.FileAction(OpenExternal)`, as the menu's `files_open_system` |
| Changes card file row | `Actions.Review` (the change in the panel) | `OpenLink` with the path, external |
| A path of a tool group's path list, and the file name heading a tool's diff | `OpenLink` with the path | `OpenLink`, external |
| The child panel's transcript, the session overview's sources | The same hook with the child's session | Same |

A tool row's own click stays its disclosure: the path in its summary line is
not a link of its own. MyGo reports only the primary button's click on a
Markdown link, so a middle click does nothing. Links carry MyGo's tooltip (the
address), not a note about where a click goes, as in Rust.

In sample data (the preview and Demo mode) a web address opens a browser tab
that loads nothing and shows the address (`panel.State.Browse`), a file of the
conversation opens its preview tab in either mode (a sample path names nothing
on this device), and mail, session and unknown links open nothing; only a web
address clicked with the modifier reaches the system browser.

`chatEmbed` (`conversation_embeds.go`) answers `State.Embed`:
`sailry-image:<result>/<index>` is the tool result's picture of that reference
(`Image.Reference`) or the missing line; a workspace picture is a thumbnail
read through the attachment image cache; another workspace file is a file
card. Links with a fragment, a query or a line keep their navigation. Tool
result pictures are read as other conversation images: at most the attachment
transfer limit, decoded under the bitmap limits, two at a time; one that is
gone, too large or undecodable is marked `Missing`.

Built-in tools carry `LabelKey` and an icon (`coreTool`); a failed call
without a message of its own shows its failure kind in the user's language
(`toolFailure`). A failed run's title is classified by `failureKey` (model
recovery stop reason, model request reason, plugin callback, compaction), and
the last retry row of a stopped recovery carries the same reason.

Session and host
mentions navigate existing views. Project and worktree mentions use the shared
read-only details dialog for the turn's captured location; dedicated workspace
resource navigation remains with B5. No `Turn` field or `Actions` signature changes
are required for these view-only bindings.

Delegated rows retain their canonical parent turn and tool entry. Shared Client
helpers select the latest relevant child run within that owner, preferring active
work to queued continuations. The waiting phase appears only after the parent's
tool batch is committed and while its foreground wave remains unsettled; streamed
output, consent and pending guidance take precedence. Background children do not
hold this phase. Guidance is pending for the turn that takes it: the run's
`joined` turn until its user record arrives, which for a batch that outlived
its recipient is the successor rather than the `guidance` turn it was sent to.

An approval that Stop closed (`cancelled`) leaves its call a plain row without
state text or warning color, as part of a turn whose footer reads Stopped; an
approval that lost its owner (`interrupted`) keeps the warning. A stopped
session is idle in the sidebar, not failed.

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
open. Read aloud remains a controller-native effect: the local Node's speech
model where one is assigned, the OS speech otherwise ([media.md](media.md#desktop)). Opening a remote session
in another window keeps its requested selection until remote data arrives.
These routes use additive Node identity without rewriting protocol SessionIDs.

### B10 visuals

A fenced `mermaid` block inside a reply's Markdown (the `mermaid` entry of the
replies' `markdown.Options.Code`) and a `VisualBlock` are drawn in place by
`showVisual` (`visuals.go`), with no WebView and no card: no border, fill or
padding, the left edge in line with the text. Each source is read once (at
most 256 are kept). A block that is still streaming, is empty, is over
64 KiB, or that cannot be drawn shows as the ordinary code block, with no
error box and no notice; the chart replaces the code when the fence closes.
Until then the source is not parsed, laid out or logged, so a half-written
diagram never shows an error.

**One title.** A `VisualBlock`'s `Title`, or else a data chart's Mermaid
`title`, is one 12 DIP muted caption line above the visual and its accessible
name; without either the name is the diagram's kind (`xychart`, `pie`,
`flowchart`); a diagram that is not a data chart takes the source's
`accTitle` before its kind. Nothing is titled twice: a native chart never
draws the Mermaid title inside itself, and a picture that draws its own
title shows no caption.

**Data charts** (`chart.go`). `xychart-beta` and `pie` are drawn natively
from the model limu's parser reads (`diagram.Prepare`, then
`xychart.ParsePrepared` or `pie.ParsePrepared`) with MyGo's `ui/chart` scales,
`Plot`, `Pie`, `Sector`, `SliceAt` and `Anchor`:

| Mermaid | Drawn |
|---|---|
| `xychart-beta`: any mix of `bar` and `line` series, named or not; a category or numeric x-axis; an optional y-axis range; `horizontal`; the `xyChart.chartOrientation` config | Bars side by side per category (at most 40 DIP wide) and lines with points over them on one value axis. The source's y range is kept, otherwise bars start from zero on a rounded axis. A category without a value is a gap |
| `pie`, `pie showData` | A 180 DIP pie with the legend beside it: label and share, and the value too with `showData`; two columns past six slices |

The chart is as wide as the reply up to 560 DIP. A plot with its axis labels
is 200 DIP tall; a horizontal chart takes 24 DIP per row between 120 and
260. Axis labels are 11 DIP and muted, the grid is hairlines at the value
ticks only, ticks share one unit (`k`, `M`, `B` from ten thousand), category
labels that would overlap are left out and long ones are cut at 14
characters. An axis title shows only when the source gives one, small and
muted. The legend shows under the plot only for several series of which the
source named at least one. These follow the message text size
(`TestLiveBlocks/content sizes`): axis labels, axis titles and the legend
stay three DIPs under it, a pie's legend two under it on rows six higher than
its text, and the plot's height and the room its axes take grow in proportion
(`size/14`). The pie's 180 DIP box holds no text and keeps its size, and the
tip over a mark is a tooltip, which keeps Kit's 12. Colors are `theme.Tints` (blue, violet, teal,
amber, gray; the lighter end in dark mode), stepping toward the background
from the sixth series.

The mark under the pointer shows its value at once, on Kit's floating
surface inside the plot: the category and value over a single series, the
category and each series' value beside the category for several, the label,
value and share on a pie slice; the other marks dim. The plot or pie is one
Tab stop with role image: Left/Right (Up/Down), Home and End move the pick,
Escape clears it, and each pick is the element's value and is announced.
Nothing animates: there is no arrival animation, as the virtual list would
replay it whenever a turn scrolls back in, and no frame is asked for at rest.

**Hostile data** never panics and is never drawn wrong on purpose. `chartOf`
returns no chart, and the source falls back to the diagram's picture (or to
the code block when that cannot be drawn), for: no series or more than 8; no
category or more than 366; a value that is infinite; no finite value at all;
series of different lengths on a numeric x-axis; a range a float64 cannot
span; a pie with no slice, more than 12, a negative or non-finite value, or
a sum of zero. Mermaid's own rules stay: a non-number is a syntax error
(code block), series on a category axis are padded or cut to the
categories, and repeated category labels are separate categories. Titles,
labels and series names of any length are cut with an ellipsis (60
characters read out and in legends, 20 in a tip, 14 on an axis).

**Other diagrams** are CrychicUI's `diagram.Mermaid`
([crychic.md](crychic.md#diagrams)), which lays the source out with pele-go
in the app's font and text size and paints the result itself: every kind
pele-go draws (33, among them flowchart, sequence, class, state, ER, gantt,
journey, mindmap, timeline, gitGraph, quadrant, sankey, C4 and block).
`crychic.MermaidOf` renders the source first, once per source, font and text
size (at most 256 are kept): it gives the picture's size, kind, `accTitle`
and whether it draws a title, and refuses a source the library would answer
with its error box, which then shows as code. The picture keeps its natural
size, scaled down to the reply's width and to 420 DIP of height; one that
would shrink below 0.7 keeps that scale in a 420 DIP box that scrolls both
ways. It is built under the app's theme with the palette's `Muted` as the
node fill, `Secondary` as its second tone, `Border` as the outline and half
the radius, as before; lines and text are the text color and series take the
accent and status tones, which the library fixes. The library's 200 ms
arrival is held still, a picture has role image with its name and takes no
focus, and nothing moves at rest.

Against limu's picture, which this replaces, the layout no longer follows the
reply's width: a wide left-to-right flowchart is scaled down, then scrolls,
where limu turned it to run down, and a chart or gantt is drawn at pele-go's
own width. A source is laid out twice when first shown (the check, then the
library) and again by the library whenever its element is built anew or the
theme changes, where limu kept one layout per width and repainted on a theme
switch.

Math is bound by `bindMath` (`math.go`): one process-wide
`untiemygo.Renderer` (Untie, Latin Modern Math at
`internal/desktop/conversation/math.otf`) bound with `BindMarkdown` for `$…$`,
`$$…$$`, `\(…\)` and `\[…\]`, colored from the theme at paint time. With no
usable font nothing is bound and formulas keep their source text. Untie adds
its math flags to `Options.Extensions`, where MyGo reads zero as its `Default`
set, so `bindMath` writes `parser.Default` out first: a reply keeps its
tables, task lists, strikethrough, autolinks, footnotes and alerts beside
math. While a reply streams, MyGo holds a top-level table's header back until
its delimiter row is whole; the header of a table inside a list item shows as
a line of text until then (pinned MyGo).

## Turn settlement and queue

`store.FinishTurn` ends a turn and settles its session's queue in one
transaction; `store.RecoverRuns` does the same for turns a restart left
without an owner.

- **Guidance.** A message sent to guide a running turn carries that turn as
  its recipient and is skipped by the ordinary claim while the recipient can
  still join it. When the recipient finishes or is recovered without joining
  it, the message loses its recipient exactly once, in the finishing
  transaction, and is an ordinary queued item at its place: it starts next
  after a turn that left the queue running, and waits in a paused queue until
  Send now or Resume.
- **Batched guidance.** Guidance joins its recipient at a boundary: after
  tool results or a finished reply, before the next model step, never behind
  a failed step. One transaction takes every input waiting for the turn, in
  the order sent, and appends each as its own user record under its own turn
  with its own attachments and references; the model then answers them in a
  single step. Input that arrives after the batch was taken waits for the
  next boundary. A batch that overfills the context goes through the turn's
  ordinary compaction (at most two recoveries) and, when that cannot recover,
  fails the turn: the failed step's record carries `context_overflow` and the
  run's fault is `unavailable`. Nothing is truncated: joined inputs stay whole
  records of their own turns; input waiting during a recovery joins after the
  compaction, before the retried step; input sent during the last attempt
  returns to the queue whole and starts its own turn
  (`engine.TestGuidanceBatchOverflow`). `guide_turn` binds attachments
  like `queue_turn`, so an attachment that is not stored yet refuses the
  message.
- **Compatibility.** Guidance joins only a recipient that froze the same
  settings: session config (model, mode, permission), roles, plugins and
  hooks. Other input stays an ordinary ready item at its place and starts its
  own turn with its own settings. Compatible guidance is placed after the
  guidance already waiting for its recipient and ahead of ordinary items.
- **Outlived guidance.** Guidance released by a recipient that ended first
  keeps its order. When the first of it starts a turn, the inputs sent to the
  same recipient that directly follow it, are ready and froze the same
  settings are addressed to that turn and join it before its first model
  step, so a pile of guidance is still answered together.
- **Queued messages.** Ordinary queued messages are strict FIFO: each starts
  its own turn with the settings it was queued with, one at a time. They are
  never combined.
- **Pausing.** The queue is paused by `stop_turn`, by a turn that ends
  `interrupted` without a stop (Node shutdown, restart recovery) and by a turn
  that ends before its own input reached history, which stays in the queue as
  a retained item. A turn that fails after recording its input does not pause:
  queued messages start and background results are delivered.
- **Pause reason.** A paused queue carries `pause_reason` in the snapshot
  and in every queue update: `user` (`set_queue_paused`), `stopped`
  (`stop_turn`), `interrupted` (a turn lost its owner at shutdown or restart)
  or `unrecorded` (a turn ended before its input reached history; when it also
  lost its owner the reason is `interrupted`). The field is optional: it is
  absent while the queue runs, for queues paused before the field existed and
  for a child session whose turn failed. Any resume clears it.
- **Send now.** `send_queued_turn` during a running turn stops that turn and
  resumes the queue with the chosen item first. Finishing a stopped turn never
  pauses again, so the chosen item starts as soon as the stopped turn ends.

- **Stop.** A turn the user stopped ends `cancelled`, with no error unless
  its outcome is uncertain, and its pending approvals and questions close as
  `cancelled`. `interrupted` is kept for a turn that lost its owner: Node
  shutdown and restart recovery, which also close interactions as
  `interrupted`. A child stopped through its parent turn is a deliberate stop
  too and ends `cancelled`. Any other ended turn closes its leftover interactions as
  `cancelled`. Completion hooks run only for turns that ran to their end: the
  hook binding is bound to the turn's stop and runs nothing once it is
  stopped or the Node is stopping. The Turn End hook receives
  `SAILRY_HOOK_STATUS` `completed` or `failed`, as in the reference; After
  Tool hooks receive `succeeded` or `failed`. Hooks run only for task turns
  outside Plan mode, which also excludes read-only sub-agents because their
  session runs in Plan mode (`engine.TestTurnEndHookStatus`).
- **Attention.** The session becomes unread when a main task turn ends
  `completed`, `failed` or `interrupted`. Stopped turns, compaction runs and
  child runs do not raise it. An automatic turn (a notice turn or a package
  continuation) takes the mark back when it starts, because the session is
  working by itself again; the turn that ends the chain raises it. A session
  a package keeps working is therefore unread once, at the end, not after
  every turn (`store.TestAutomaticChain/automatic turn takes back the unread
  mark`).
- **Outcome notice.** One rule, `store.runNoticed`, decides which finished run
  raises a durable task notification. A turn the user started, including a
  continuation the user asked for, and a turn a dispatch job admitted report
  `completed`, `failed` and `interrupted`. An automatic turn (a notice turn or
  a package continuation the user did not ask for) reports only `failed` and
  `interrupted`, so a broken chain is not silent and a chain of up to 100
  successful turns does not evict the 200 kept notifications. A stopped turn
  and a child run report nothing. The Node cannot know at a turn's end whether
  another automatic turn follows, so every failed or interrupted automatic
  turn is reported, not only the last; the success that ends a chain is its
  package's to report (`store.TestAutomaticRunNotices`). The failure is the
  Node's alone: a package adds no notification of its own for a failed or
  interrupted turn ([goal](goal.md), `node.TestGoalLoop`).
- **Durable end.** A turn keeps its owner until its end is stored. When the
  finishing transaction fails, it is repeated every second, logged once
  (`operation=finish`), until it succeeds or the Node stops; a stopped Node
  leaves the run to restart recovery. Only the write is repeated: the model,
  tools, hooks and cleanup have ended.
- **Restart.** Running and stopping turns end `interrupted` with an uncertain
  outcome and nothing is replayed. Queued delegated runs that never started
  end the same way, except one the user had already stopped, which ends
  `cancelled`. Queued notice turns and plugin continuations are
  cancelled without pausing the queue or raising attention; a queue that
  still holds the user's input is paused.

- **Background results.** A finished background child, or a command that
  asked for a follow-up, starts a notice turn when its session is free: not
  archived, not a child, worktree present, queue running and empty, no
  running turn and no pending approval or question. Work mode does not
  matter; a notice turn in Plan mode runs under Plan's own restrictions.
  Delivery is level-triggered: besides the two edges that finish a result,
  every claim pass that finds nothing ready admits a notice turn for each
  free session that still holds results, so a result waits only for its
  blocker whichever command removed it. An admission that fails (for
  example a disabled provider) leaves the results deliverable and is logged
  once per session and reason (`event=node.background operation=deliver
  status=refused`).
- **Automatic chain limit.** A session starts at most eight automatic turns
  in a row without user input. The count covers started notice turns and
  package continuations since the last user turn or joined guidance;
  automatic turns that never started neither count nor reset it. A package
  that owns a loop declares its own limit, 1 to 100, on each continuation:
  the continuation is admitted while the count is below it, and notice turns
  of the same chain follow the limit its latest continuation declared, so
  one counter bounds the loop. A continuation that declares none keeps the
  chain's limit and carries it on, so the chain has one limit whoever reads
  it (`store.ReadAutomaticChain`). A continuation the user asked for (a Resume
  control) is the first turn of a new chain. At the limit a continuation is
  refused with the reason `limit`, and the remaining results are marked
  suppressed, logged (`status=held`), stay visible as pending background
  tasks and join the user's next message or the package's next continuation
  (`store.TestAutomaticChain`).
- **Continuation admission.** `continue_turn` is refused with a named reason
  in the fault's details (`kind: continuation`): `plan`, `child`, `archived`,
  `worktree`, `queue_paused`, `queued_input`, `running`, `interaction`,
  `limit` or `stale`. A continuation takes the session's waiting background
  results as a user turn does, held ones included, and may name one of the
  package's declared `continuation_markers`.
- **Held results.** A pending result that is `suppressed` starts no notice
  turn and carries the optional `hold` on its background task, in the
  snapshot and activity events: `limit` (automatic chain limit), `stopped`
  (the user stopped the turn that was to receive it), `interrupted` (that
  turn or the Node's restart lost it), `failed` (its notice turn failed
  before taking it) or `other` (the notice turn was withdrawn at claim, for
  example because user input was queued). The hold is cleared when a turn
  reserves the result; when that turn ends without taking it, a result that
  was held stays held for the way the turn ended (`stopped`, `interrupted`,
  `failed` or `other`), so no result is suppressed without a reason. A result that is pending without `suppressed` and not
  delivered is waiting only for the session to be free; the queue's
  `pause_reason` says why when the queue is paused.
  A result that waits is in the transcript, never in the queue strip: a
  background agent's state is on its row under the turn that started it, and
  a background command's is on the row of the call that started it
  (`Call.Background`): running, or how it ended, the exit code of one that
  failed, and "Result not delivered" until a turn takes it, whatever holds
  it (a paused queue, the chain limit, a stop). The Node states the link and
  the code: the tool result's summary names the task it started (`task`, the
  id `command:<request>`, for a `command_started` result, which a command
  moved to the background while it ran answers too), and the task carries
  the optional `exit_code` of a command that ran to its end. `live`
  (`projectBackground`) reads both from `page.Background`; a task the Node no
  longer lists (it keeps the unfinished ones and the 20 latest finished)
  leaves the row plain, as before. The result then shows as the Background
  results bubble of the turn that takes it. The reference draws nothing for
  a finished background command until its notice turn
  (`live.TestQueueStates/a background command reads on its row`,
  `conversation.TestGrouping/Background`).
- **Stalled commands.** A background command that asked for a follow-up and
  stays silent writes `stall:<id>` evidence with the outcome `stalled`
  ([process](process.md)). It is delivered by the same rules as any result:
  one notice turn per stall period, because evidence reserved for a turn is
  not rewritten and only delivered evidence is renewed by the next period;
  each such turn counts toward the automatic chain limit; and a session the
  user stopped receives none, because its queue is paused and evidence
  released by a stopped notice turn stays held across later periods until
  the user's next message takes it (`store.TestStalledDelivery`).
- **Turn callbacks.** After a main task turn ends, whatever its status, and
  once that end is stored and published, the Node calls the `after_turn`
  handler of every package the turn captured, as in the reference. Child
  turns, compaction and a stopping Node call none. The call is an ordinary
  `call_plugin` request dispatched for the turn's caller with the session
  and worktree as context, so the callback runtime's allocation budget,
  deadline and cancellation apply. Its input states the turn's facts: `turn`
  and its `sequence` in the session's order, `status` (`cancelled` is the
  user's Stop, `interrupted` a turn the Node lost, `failed` with `error`),
  `mode`, `automatic` (a notice turn or a continuation), `notice`, the
  package's own `continuation` marker, the package's tool `names`,
  `tool_calls` and per-name `tools`, `expired` approvals and questions,
  `usage` of the turn and `session_usage` (both with delegated children),
  `background.running` and `background.pending`, and the automatic `chain`
  (`node.TestPluginTurnFacts`). `turn.initialize` receives `automatic`,
  `notice` and `continuation` as well. Callbacks of consecutive turns can
  overlap, because a queued turn starts as soon as its predecessor ends;
  `sequence` lets a package ignore the older one. A package that was
  disabled, removed or changed since the turn has no callback; any other
  failure is logged (`area=plugins operation=after_turn`) and never changes
  the ended turn. A handler that issues `continue_turn` or submits a turn
  inside a transaction admits a turn whose input is captured on the run at
  admission, because a plugin transaction stores only a digest of its
  request (`store.TestPluginTransactionSubmit`). [Goal](goal.md) is the
  bundled loop built on these.
- **Cancelled foreground children.** A child that ends `cancelled` or
  `interrupted` after its owning turn recorded the cancellation note has its
  background task marked consumed, as the reference collects it; the model
  was already told. When the child's end is stored first, the owning turn's
  settlement consumes it after the child has settled
  (`store.ConsumeStoppedChildren`, see [delegation](delegation.md)). The turn
  keeps the children it cancelled, so one that finishes and leaves the runtime
  before the turn settles them is consumed as well. A child cancelled any
  other way stays a pending task.
- **Plan review source.** The review offered after a completed Plan turn
  points at the turn's last assistant message that is visible to the user
  and has text, as in the reference, so a hidden runtime note recorded after
  the answer does not suppress it. A source that carries an error, an
  interrupted or failed finish, or a tool request still offers no review.
- **Phases.** The turn's owner collects its execution intervals
  (`engine/timeline.go`): each model request as waiting until its first token
  and output after it, each tool call, and as excluded the time the turn
  waits for somebody else (an approval, a question, an MCP input request, a
  delegating tool call). When the turn's hooks have ended and before its end
  is stored, the owner reduces them once to `active_us`, `waiting_us`,
  `output_us`, `tools_us` and `other_us` and appends them as a hidden record
  (`metadata.execution.phases`). Every instant belongs to one phase, the
  later of waiting, output and tool where they overlap; an excluded interval
  removes its own call's time and nothing else, and time left with nothing
  but excluded intervals is not active. The session's statistics sum the
  turns' phases (`observations.phases`) and report them unknown while any
  finished turn has none, which a turn that ended before this record existed
  or lost its owner does. A call owns its intervals by its id, where the
  reference uses a task-local number (`engine.TestTurnPhases`).

### Queue item states

An item is a row of `agent_pending`; `ready`, the recipient and the queue's
`paused` flag decide what it does. Every transition is one store transaction
and bumps the item or queue revision that its command checks. The Desktop
column is what the composer's queue strip shows: rows never say why they wait
and held background results are not rows ([removed-ui.md](removed-ui.md),
[composer.md](composer.md)). "Held" in the table is the Node's name for an
item that is not ready to send (`ready=0`); it is neither the removed "Held"
wait label nor a held background result.

| State | Stored as | Leaves by | Desktop |
|---|---|---|---|
| Held | queued run, `ready=0` | `start_queued_turn`, `send_queued_turn`, `guide_queued_turn`, `remove_queued_turn` | A plain row |
| Queued | queued run, `ready=1`, no recipient | claim when the session has no running turn and the queue runs; `remove_queued_turn`; `guide_queued_turn` | A plain row; with the queue paused it stays until Send now or a new message. A pause the Node made after a turn lost its owner (`interrupted`) is one warning toast, `queue_halt_interrupted`; the user's own Stop or pause is not reported |
| Guiding | `ready=1`, recipient is the running turn | join at the boundary; `withdraw_guidance` (held); `edit_queued_turn` or `set_queue_paused` (queued); recipient ends or restart (queued); `remove_queued_turn` | The row shows the loader and Withdraw; guidance that runs as its own turn is one toast (`queue_guidance_separate`, `queue_guidance_outlived`) |
| Joined | run `completed` with `joined`, row deleted | terminal; shown under the joining turn | No row; the message is under the joining turn |
| Running | run `running`; row kept until its input is recorded | turn end | Its turn in the transcript |
| Retained | run `failed`, `cancelled` or `interrupted` before its input was recorded, row kept, queue paused | `start_queued_turn` or `send_queued_turn` (queued again), `remove_queued_turn` | A plain row again, and one error toast, `queue_halt_unsent` (`unrecorded`), or the warning above when the turn also lost its owner |
| Removed | run `cancelled`, row deleted | terminal | No row |

| Trigger | Effect | Test |
|---|---|---|
| `queue_turn` | held item at the end | `engine.TestQueueOrder` |
| `start_queued_turn` | held to queued, queue resumed | `engine.TestQueueOrder` |
| `guide_turn`, same settings | guiding, after the recipient's other guidance | `store.TestGuidanceQueue/pending guidance is taken together` |
| `guide_turn`, other settings | queued in place | `store.TestGuidanceQueue/other settings stay ordinary` |
| `guide_turn` with an attachment not stored | refused, nothing admitted | `store.TestGuidanceQueue/pending guidance is taken together` |
| Boundary | all guiding items joined in order in one step; later ones next boundary | `engine.TestGuidanceBatch`, `store.TestGuidanceQueue` |
| `edit_queued_turn` | new text, guiding to queued; stale revision refused | `store.TestGuidanceQueue/queue commands` |
| `guide_queued_turn` | held or queued to guiding; compaction refused | `store.TestGuidanceQueue/queue commands` |
| `withdraw_guidance` | guiding to held | `store.TestGuidanceQueue/queue commands` |
| `move_queued_turn` | new position; stale queue revision refused | `store.TestGuidanceQueue/queue commands` |
| `set_queue_paused` | pause: guiding to queued, nothing starts or joins; resume: first ready item starts | `store.TestGuidanceQueue/queue commands` |
| `remove_queued_turn` | removed | `store.TestGuidanceQueue/queue commands` |
| `send_queued_turn` | item first, running turn stopped, queue resumed | `store.TestQueueSettlement/send now replaces the running turn` |
| `stop_turn` | queue paused; items stay; guiding to queued when the turn ends | `store.TestQueueSettlement/guidance after stop waits for *` |
| Turn completes or fails after its input | guiding to queued; next ready item starts | `store.TestQueueSettlement/guidance after completion starts`, `guidance after failure starts`, `failure keeps queued input running` |
| Turn ends before its input | retained, queue paused | `store.TestQueueSettlement/failure before input holds the queue` |
| Outlived guidance starts | following guidance of the same recipient joins it | `store.TestGuidanceQueue/outlived guidance starts one turn`, `engine.TestGuidanceBatch` |
| Node restart | guiding to queued once, queue paused when it holds user input; queued notice turns cancelled | `store.TestQueueSettlement/guidance after restart waits for resume`, `restart cancels a queued notice turn without pausing` |
| `compact_context` while running | compaction item waits its turn; not editable, not guidance | `store.TestGuidanceQueue/queue commands` |
| Background result | notice turn admitted only into an empty running queue; cancelled at claim when user input is queued | `store.TestBackgroundDelivery`, `store.TestBackgroundSchedulingAndContinuationFence` |
| Package `after_turn` issues `continue_turn` | continuation turn queued and started like any ready item; `automatic` on its own callback; refused with a reason when the session is not free or the chain is at its limit | `node.TestPluginAfterTurn`, `node.TestGoalLoop`, `store.TestAutomaticChain` |
| `remove_session` with waiting input | refused `busy` until the items are removed | `store.TestGuidanceQueue/queue commands` |
| `set_session_archived` | queue unchanged | `store.TestGuidanceQueue/queue commands` |

### Differences from the reference

| Difference | Reason |
|---|---|
| Outlived guidance batches only the inputs sent to the same recipient; the reference also folds every following ready compatible queued message into that turn | An ordinary queued message keeps the turn it was queued for; the reference combines it only when it happens to follow released guidance |
| Guidance compatibility does not compare the provider snapshot or the compaction capture | The joined input runs under the recipient's frozen provider either way; a provider edit is not a per-message choice |
| A joined guidance run ends `completed` with `joined` set; the reference uses `continued` | Existing Go behaviour that the desktop projection reads |
| A paused queue cannot hold guiding items, so a boundary needs no pause check; the reference checks at the boundary | Pausing already returns guidance to the queue in both |
| Automatic turns are bounded per session by a flat count of eight, which a continuing package may raise to at most 100 for its own chain; the plan described a chain depth of four | A flat count needs no lineage between results and also bounds independent results arriving apart; eight leaves room for a fan-out, and each notice turn carries up to 16 results. A package loop states its own limit so the user sees one counter |
| A package continuation shows as a marker row in the window's language; the reference's desktop did the same and the Go projection had rendered the package's prompt as a user message | The prompt is input for the model, not something the user wrote |
| Child runs raise no attention; the reference marks the child session unread | A child session is read through its parent, which gets its own turn or notice |
| Guidance pending at a user Stop waits in the paused queue; the reference starts it as a successor turn at once | Stop means stop: nothing runs until the user sends or resumes, and the message stays visible and editable |
| The ordinary claim skips guidance while its recipient runs, and every exit of the recipient releases it; the reference claims without looking at the recipient and repairs the tag on each exit path | One release point in `FinishTurn` and one in `RecoverRuns` cover Stop, failure, completion and restart |
| A cancelled foreground child is consumed when it ends, keyed on the owner's recorded cancellation note; the reference collects it from the task that awaited the child | The store settles it in the child's own finishing write, so it does not depend on the owner turn still running |
| A plan review whose chosen source has a tool request, an error or an unfinished answer is not offered; the reference checks only visibility and text | Existing Go validation; a review must point at a finished answer |
| A stopped turn whose input was not recorded keeps that input as a retained queue item; the reference writes it into history | Nothing is lost and the user decides whether to send it again |

## UI state vs backend data

Canonical plugin cards append to the existing tool disclosure in their original
block order. `live/plugin_cards*.go` reads the captured declaration and code by
canonical receipt, supplies the restricted card Host, and fences interactions by
receipt and mount generation. `State.SetToolCard` owns only presentation;
`Call`, `Output`, canonical history and the existing Actions contract are
unchanged. Node remains the authority for card permissions and current
same-digest package access. Offscreen or closed cards stop their observers and
native authorization page.

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

`demo*.go` is the dataset of Demo mode (docs/shell.md), written as a
product's own work in en and zh-CN (`demo/<locale>.yml`). Its conversations
between them show every value of the view model's enumerations and set every
field; `TestDemo` reads both from the types, so a kind added to the view
model fails it until a demo conversation shows it.

| Conversation | Shows |
|---|---|
| `harbor-api/rate-limit` (opens) | A running reply: reasoning, grouped read and search calls, a running and two pending calls, subagents, a task's progress, a pending question; approvals (a change set of every kind, a command with a prefix and a notice, unrestricted access, a subagent's), a question with Other, the queue (plain, guided, attachment, compaction, notice), background commands, a plugin's badge with its panel |
| `harbor-api/deploy` | Approvals in every state: an SSH host key to trust and one replacing a known key, details being read, a failed read with Retry, a decision on its way, one sent again and a receipt; a question whose answer is sent again; a rewind in flight |
| `harbor-api/webhooks` | A plan awaiting review: Continue and Execute |
| `harbor-api/flaky-test` | A message with a picture and a file; a failed call, a tool's own failure, a diff, a long command output, a partial table with a notice; the changes card with a file undone |
| `harbor-api/invoices` | A step in every state; denied, interrupted and unexecuted calls; background results in every outcome; commands sent to the background, one running, one ended and one failed with its exit code, their results not delivered; retries, one stopped by the provider and one exhausted, and the turn's error; interrupted, failed, continued and cancelled runs |
| `lumen-web/review` | Subagents in every status, a standalone call, a run waiting for two of them |
| `lumen-web/onboarding` | Questions in every state, single and multiple choice; a plan review, a connector's form and its link; a declinable question with several choices waiting |
| `lumen-web/audit` | A subagent's own session: read-only messages, no composer, the queue's read-only tray |
| `lumen-web/tracker` | A connector's form: text with length bounds, multiline text, an integer with a range, yes or no, one choice and several, required and optional; declinable |
| `lumen-web/metrics` | Search sources, citations and the search's suggestions; files of every kind and preview state, a published document and a published page; generated pictures; a tool's pictures, one of them gone; a reply's links as a file card and as pictures; two charts (weekly active teams as bars, week-4 retention as a line); math, a diagram and links; a draft with attachments in every status. The report is its last turn, so the session overview lists its cited pages and outputs |
| `growth-notes/sign-in` | A project that is a plain folder; a connector's authorization link, declinable |
| `chat/hashing` (pinned) | Markdown: headings, lists, a table, code, a quote, math, a sequence diagram, a task list, links; its last message open in its editor |
| `chat/handbook` | A built-in tool's label, a handoff summary, compaction turns in every state with their duration, a reply compacting with its elapsed time, a queued run |
| `chat/staging` | A connection recovering with a diagnostic, an unsynced run awaiting a response, a question that cannot be answered meanwhile, export disabled, an older page that did not load |
| `chat/release-notes` | A run stopping while the model thinks |
| `chat/translate` | An offline host |
| `lumen-web/pursuit` | A goal in pursuit: the user's turn that set it, a finished continuation and a running one, each under the package's marker row in place of a user message; the badge's icon with Running and the time, its details (objective, turns, time, tokens) and panel (Pause, End) |
| `lumen-web/pursuit-limit` | A goal paused at the turn limit: its wrap-up continuation, the badge's icon with Paused and the reason, and the panel (Resume, End) |

The first page loading (`Session.Loading`), an older page loading
(`LoadingOlder`, which the preview plays on a timer), an answer on its way
(`Ask.Busy`), an edit being saved (`Editing.Pending`) and a plugin's tool
card are not in the demo: the first four are moments rather than content,
and a card is drawn by a mounted plugin. The demo's projects carry an
appearance (icon and color).

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
- `internal/desktop/shell/loading_test.go` (`TestShellRules/LoadingMarks`):
  in the demo's running conversation, the pending rows' and the question's
  icon, label and mono detail are level within half a DIP at scale 1 and 2,
  and the phase row has its mark before "thinking" and "replying"; with
  `$SAILRY_SNAPSHOTS` it writes the `loading-*` frames.

`TestLiveBlocks/transcript` drives the message actions with the pointer in
Chinese and dark: the inline editor (open, Escape, Cancel, blur, Save, pending),
enablement in a read-only session and on a turn whose actions do not apply,
Markdown cards, a tool result's picture, the stopped retry, the interrupted
compaction, the document exports, the search result dialog and the selection
pill; its frames go to `.runtime/ui-shots/transcript-*.png`. The live
`TestComposerSendRouting/replace_references` row does the same against the
adapter: the `replace_turn` command and its failures, every link kind's
destination, the rewind toast's backup action, and
`failure_retains_draft` the Restore draft action.

## Deviations

| Deviation | Reason |
|---|---|
| The editor of a sent message edits its text only; its attachments and references show unchanged and are sent again | The reference's editor is the same text field; the bubble has no chip editor |
| A file link of a reply resolves against the session's current worktree | The reference resolves against the worktree the turn ran in; the page projects no per-turn worktree path |
| The web search result is a dialog over the window, and the provider's suggestions show as their links | The reference opens a second window with the suggestions in a scriptless web view; the transcript owns no window, and the dialog is the existing host for content over it |
| The search result's links follow the content's rule (panel on a click, system with the modifier) | The reference's result window sends every link to the system browser; the user's rule is one behaviour for every link of a conversation |
| A file card's Open, a changes row, a tool group's paths and a diff's file name take the platform modifier or open the file | The reference reads the modifier on Markdown links and the image link button only, and its tool paths are text; the user's rule covers files too |
| A web address with credentials opens nowhere | The reference hands it to its browser resource, which refuses it; here it also never reaches the system browser |
| A tool result's pictures follow its other output | The reference places each picture at its position among the result's content blocks; the summary carries the pictures apart from the detail |
| A failed send keeps its draft; Restore draft is offered only when the composer was changed before the failure arrived | The reference clears the composer on send and always offers the restore; saves keep drafts on failure here |
| Reasoning is shown as a disclosure; live mode drops it | Kept for the preview to show the block; drop it in `project` to match |
| The jump fade runs at the display's rate | MyGo element transitions have no frame-rate parameter; only drawings take `AnimationFrameRate` |
| The in-progress step's `loader-circle` does not spin | A spinning icon would be an `Element.Loop` at the display's rate; the shimmer on its text marks it instead |
| PDF and other files without a native preview open no tab: Open hands them to the system's app (a missing file toasts `files_open_failed`) | MyGo v0.8.3 has no PDF or office view; a WebView preview comes later (docs/panel.md) |
| The image viewer zooms with its buttons and + − 0, not the wheel or pinch | MyGo elements report no wheel or pinch deltas; drag pans |
| Search is MyGo's find bar over the transcript ("1 of N", previous, next, Done) | The accepted layout; live reads Node history and loads pages through an exact match, while preview searches its loaded fixtures |
| Search marks prose only: code spans, fenced code and a link's destination keep their text, so a match inside them is counted by the find bar but not highlighted | `<mark>` inside code would show as text and inside a destination would break the link |
| Search marks the answer's text blocks and the user bubble, all matches alike in the answer | Markdown marks with `<mark>` (one color); the bubble's current match is stronger |
| The selection pill sits at the turn's top end | MyGo reports the selection's text, not its bounds |
| Citations are placed in the answer's last text block | Rust offsets them across the text parts of the run |
| The phase's mark is one of MyGo's loading marks (`plugins/loading`: Swirl, Trace, a paused Pulse; and Kit's Loop and Chevrons, drawn as the set's marks MyGo lacks), not Rust's dot figures (Orbits, Globe, Ribbon, Ring) | The user's decision: ready-made marks from one set with the sidebar's loader. The custom figures are removed; the marks move at the display's rate ([components.md](components.md#theming-caveats)) |
| Row texts are moved a fraction of a DIP by `kit.Level` so their capitals sit at the icon's middle; a question's row is as high as the others for one line (24), its icon and caret where theirs are | MyGo centres a text's line box and snaps its baseline, which left labels up to 1.2 DIPs below their icons ([components.md](components.md#text-beside-icons)); the wrapping row had its text at the top with the icon 2 below |
| A row's single-line label and its detail are `ui.RichText` of one span, levelled by `kit.LevelSpan`, running or not | `kit.Sweep` draws its band with `Painter.RichText`, whose line starts from the theme's 14 whatever the span's size; a plain `ui.Text` at another message text size sits up to 2 DIPs off the band, and a label laid out one way while running and another once settled would move |
| A code block's Copy is an icon button confirmed by a toast, not MyGo's `CopyButton` with its "Copy" and "Copied" texts | The user's decision: the icon alone; outcomes show as toasts |
| Mermaid is laid out on the UI goroutine and its unreadable sources show the code block without a notice | pele-go lays a bounded source out (64 KiB here, 50,000 characters and 5,000 edges in pele-go) when it is first shown and when the theme changes; Rust prepares images off the UI thread. The library's error box never shows in a transcript: the adapter checks the source first |
| A diagram is not laid out for the reply's width | `diagram.Mermaid` passes pele-go no width; the picture is scaled down to 0.7, then scrolls. limu, which it replaced at the user's request, laid out per width |
| Mermaid xychart and pie are drawn by our own painter (`chart.go`) on MyGo's `ui/chart`, not by the diagram's picture or a CrychicUI chart | Verified gap: the picture has no hover and draws its title; CrychicUI's `chart` components fail the conversation's rules ([docs/crychic.md](crychic.md#charts)): their value tips are MyGo's delayed opaque tooltip or an own surface outside the plot, their colors are the status tones, and they cannot draw Mermaid's shared-axis bar and line mix, a y range, gaps or horizontal series |
| Radar, quadrant, sankey and treemap stay the diagram's picture | They are not category/value charts the painter draws; they get the card-free, capped presentation of every diagram |
| Visuals have no card, and a diagram is left-aligned and scaled down rather than centered at its own size | The accepted minimal presentation of charts in a conversation |
| ⌘F is a window shortcut, not an entry in the native menu | The shell's menu has no conversation commands yet |
| Subagent tabs show Bot, not the identicon | `panel.look` returns an icon; the identicon is a drawing |
| Older pages load when the first turn is built (near the start) | Rust asks on scrolling near the top; the list builds rows just outside the view |
| Copy confirms with a toast, not Kit `Clipboard`'s check icon | Sailry's rule: outcomes show as toasts |
| Embedded card authorization opens an isolated MyGo page window on macOS | Pinned MyGo supplies a native page window; the source places it in a resource pane |
| Shimmer and identicon are approximations | Custom drawing (MyGo has no moving text highlight or procedural avatar); same timing and colours, not pixel-identical |
| Diff rows have no syntax highlighting or line numbers | Not in the view model yet |
| MCP forms and authorization links are answered in the composer's question popover | The Node binds each request to its exact active parent call and persists its answer without adding a second transcript row |
| Live deltas keep flowing however many entries a turn commits | The reference stops streaming for the rest of a turn after 4096 committed entries; the Node remembers the 4096 most recent ones, which is all a late delta can name |
| A summary request the provider refuses for its size fails with the provider's own error | The reference replaces it with a fixed "Failed to compact: the base prompt … exceeds the model's effective context window" text that advises disabling extensions, which Sailry does not have; the summary input is already fitted to the captured limit before it is sent |
| A plan review's history row shows its answers as plain lines, closed | The reference renders the plan as Markdown, open by default |
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
A reusable connection whose server ended is closed and replaced on its next use.

Turn execution has no Node-wide worker pool or task cap. The Rust runtime ran
main turns on 8 workers and refused work beyond 64 supervised tasks; both were
removed by decision and are not to return. The governing limit for delegated
work is the Team setting `ChildConcurrency` (default 8): at most that many
children of one parent conversation run at once, counted across all of the
parent's turns, and the rest stay queued and start in admission order (FIFO).
A child cannot delegate, so there is no nested level for the limit to miss.
Different conversations do not share a budget, so the total across the Node is
unbounded. See [delegation.md](delegation.md).

A stopped, failed or panicked turn settles every unanswered tool call with an
interrupted response before its foreground cancellation notes. A call left
unanswered by a process crash stays in history without a result row; the model
request omits calls without results and results without calls, so such a
session continues. A response whose stream failed keeps its text and its error;
its tool calls are neither saved nor run, and the turn fails without another
request. A panic in a tool becomes that call's failed result, and a panic in a
turn fails the turn with an uncertain outcome; both log their stack and leave
the Node running.

Declared plugin storage and progress tools run under the declaring package of
the frozen turn; storage writes record an approved `storage` approval rather
than a prompt (docs/plugins.md). A tool call records the presentation, grouping
and display its package declares: bundled packages from the embedded catalog,
installed packages from the turn's captured version, including declared MCP
tools. A `progress` presentation therefore projects its snapshot for installed
packages too.

Node validates every reference of an input before the turn is admitted
(`validateAgentReferences`), for submit, queue, guide, replace, continue, start
coding, task start, queued edits and child guidance. Queued edits and child
guidance use the turn's captured config, roster and packages. A rejected input
creates no turn; the composer keeps its draft, references and attachments and
shows the `chat_action_failed` toast.

| Rule | Fault |
|---|---|
| Blank label, label over 512 bytes or a repeated target | `InvalidRequest`: references require distinct targets and bounded labels |
| Connection conversation with a target other than database, SSH, session or attachment | `PermissionDenied`: connection conversations only accept connection references |
| Plugin or skill whose package the turn does not freeze (an assistant freezes only its own package) | `InvalidRequest`: referenced plugin is disabled or unavailable |
| Plugin or skill of a desktop-scope package | `InvalidRequest`: desktop-only plugins cannot be referenced by an agent |
| Skill absent from the frozen package version | `InvalidRequest`: referenced skill is unavailable in this plugin version |
| SSH connection the conversation cannot reach | `PermissionDenied`: referenced SSH connection is not available to this conversation |
| Database the conversation cannot reach | `PermissionDenied`: referenced database is not available to this conversation |
| Empty or over 256-byte database, schema or table name, or a table without a database | `InvalidRequest`: invalid database reference |
| File or directory path that is not a normalized relative path | `InvalidRequest`: expected a normalized worktree-relative path |
| File or directory under a managed worktree | `PermissionDenied`: managed worktrees require their own file scope |
| Agent in an assistant conversation without delegation | `PermissionDenied`: plugin assistant does not allow delegation |
| Second agent, or a role outside the session roster | `InvalidRequest`: select one role from the frozen session roster |
| Session that does not exist | `NotFound`: session does not exist on this Node |
| Session of another project | `WrongTarget`: conversation belongs to another project |
| Attachment that the same input does not carry | `InvalidRequest`: attachment references require an attachment of the same input |

MCP form and URL elicitation uses the canonical question owner. An
authorization link is HTTPS, or HTTP to `localhost` or a loopback address, with
a host and no user information; the Node refuses any other request and the
Desktop opens no other link. Concurrent
questions keep distinct parent identities and decision wakes. Only active
elicitation time pauses tool timing. Closing a turn joins its tool resources
before settlement; failed cleanup retains `OutcomeUnknown`.
