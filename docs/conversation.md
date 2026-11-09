# Conversation transcript

The transcript of the selected conversation: a virtualized list of turns,
each a user message and the assistant's reply, with the jump button over
it. It matches sailry-public `conversation/live/messages.rs`,
`live/tools.rs`, `conversation/disclosure.rs`, `conversation/surface/*` and
Kit's `MessageScroller`.

## Package and files

`internal/conversation`

| File | Holds |
|---|---|
| `model.go` | **The contract**: view models and `Actions` |
| `group.go` | Grouping rules as pure functions (`Compact`, `AnswerFrom`, `Runs`, `Sequence`, `Summary`) |
| `conversation.go` | `State`, `View`, turns, the reply's header, work region, phase row and footer |
| `tools.go` | Tool rows, groups, sequences, outputs, diffs, tables, progress, question history, subagents |
| `jump.go` | The jump-to-latest button |
| `group_test.go`, `jump_test.go`, `bench_test.go` | Tests and benchmarks |

Shared motion (`kit.Shimmer`, `kit.Identicon`, `kit.EaseOutCubic`, `kit.Fast`)
is in `internal/kit/motion.go`.

## Entry point

```go
func View(c *ui.Context, s *State, id string, turns []Turn, actions Actions)
```

`s` is UI state (`conversation.New()`, one per window). `id` names the
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
| `Running` | Running: label shimmers, colour `muted_foreground` mixed 35% toward `foreground` |
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
9. **Phase row**: while active, the orb and the phase label shimmering;
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

## Actions

```go
type Actions interface {
	OpenChild(id string) // a subagent row was clicked: open its session
	Retry(turn string)   // the footer's retry: run the turn again
}
```

| Method | Fires | Expected outcome |
|---|---|---|
| `OpenChild` | Click on a subagent row | Navigate to the child session; failure toasts the error |
| `Retry` | Footer retry on a Failed, Interrupted or Cancelled run | Submit the turn again; a stale revision toasts the conflict; an unknown outcome toasts that the result is uncertain |

The preview (`internal/shell`) toasts `preview_notice` for both.

## Wiring a backend

```go
type live struct{ client *sailry.Client; c *ui.Context }

func (l *live) OpenChild(id string) { l.client.Navigate(id) }
func (l *live) Retry(turn string) {
	if err := l.client.Resend(turn); err != nil {
		l.c.Toast(failure(l.c, err)) // the error's localized message
	}
}

// In the page:
turns := project(snapshot) // []conversation.Turn, built from the history
conversation.View(c, state.Messages, session.ID, turns, &live{client, c})
```

`project` maps the backend's history to `Turn`s: localize labels (or set
`LabelKey`), resolve icons and details, keep IDs stable between snapshots,
and mark flags as above. Do not group: the view does.

## UI state vs backend data

| UI state (`State`) | Backend data |
|---|---|
| Scroll place per conversation, follow mode | Turns, blocks, calls, outputs |
| Disclosures opened | Run status, phase, elapsed |
| Work regions toggled | Questions, children, steps |
| Markdown themes | |

## Preview

`internal/preview/turns.go` has a scene for each case: sequence with a path
list (explore), diff and long command output (edit), runtime and tool
failures with a turn error (failed), denied, interrupted and not executed
(refused), built-in tools with a table, notices and partial (sessions),
question states (questions), one and three subagents (delegated), every
step state (planned), a plain answer (answered), and a live reply with
running and pending calls, running children and a step in progress (live).
`stream.go` plays a canned reply after a send on the frame clock: thinking,
a running read, its output, then the answer growing a character at a time.

## Tests

- `group_test.go`: `Compact`, `Runs`/`Sequence`, `Summary` flags,
  `AnswerFrom`, the last 100 lines, `Elapsed`.
- `jump_test.go`: appears on the smallest scroll and hides at the end;
  click scrolls to the end; stays pinned while streaming, stays put once
  scrolled up, and follows again after the click.
- `bench_test.go`: 1000 turns scroll; `BenchmarkScroll`, `Steady`, `Jump`.
- `internal/shell/shell_test.go`: send, stop, the reply playing to
  Completed, approvals and the question.

## Deviations

| Deviation | Reason |
|---|---|
| Reasoning is shown as a disclosure; live mode drops it | Kept for the preview to show the block; drop it in `project` to match |
| The jump fade runs at the display's rate | MyGo element transitions have no frame-rate parameter; only drawings take `AnimationFrameRate` |
| The in-progress step's `loader-circle` does not spin | A spinning icon would be an `Element.Loop` at the display's rate; the shimmer on its text marks it instead |
| Footer has no read-aloud, turn menu, export menu or hover timestamp | They need reader, fork, export and history services this UI-only prototype does not have |
| Copy confirms with a toast, not Kit `Clipboard`'s check icon | Sailry's rule: outcomes show as toasts |
| Shimmer and identicon are approximations | Custom drawing (MyGo has no moving text highlight or procedural avatar); same timing and colours, not pixel-identical |
| Diff rows have no syntax highlighting or line numbers | Not in the view model yet |
| Plan review, form and URL questions are not modeled | `Ask` covers choices and free text; add fields when a backend needs them |
| Benchmarks are slower than the first transcript (Steady ≈3.6 ms, Scroll ≈4.1 ms per frame with the tester's CPU rendering) | Hover-dependent rows rebuild on pointer moves; well under a 16.7 ms frame |
