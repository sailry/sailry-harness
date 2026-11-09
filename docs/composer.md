# Composer

B3 live construction permits an empty model catalog. `Chosen` returns an empty
model and the existing model chip displays `composer_model_empty` while disabled.
Live composers remain read-only until the owning conversation phase supplies
models and submission actions; this never seeds a fixture model or starts work.

The input under the transcript and on the welcome page: the capsules of
what the run waits on, the queue strip, the draft with its file cards and
inline reference chips, the `@` and `/` pickers, the toolbar (attach, mode,
permission, model controls, dictation, send/stop) and the context bar
(host, project, worktree, metrics), plus the file picker and location
dialogs (the reference implementation `conversation/live/composer.rs`, `live/queue*`,
`live/attachments.rs`, `live/references*`, `live/commands`, `live/mode.rs`,
`live/permission.rs`, `live/location.rs`, `model_controls/*`,
`file_picker*`, `dictation*`, `live/approvals*.rs`, `live/questions`).

## Package and files

`internal/desktop/composer`

| File | Holds |
|---|---|
| `model.go` | **The view model**: `Context`, models, teams, queue, hosts, projects, references, files, attachments, dictation, `Message` |
| `composer.go` | `State`, `View`, the surface, draft keys, toolbar, send/stop, context bar, Escape |
| `options.go` | Mode and permission menus, host, project and worktree, location dialog, compact settings popover |
| `models.go` | Model chip and its popover: team row, features, effort, model picker, score tooltip |
| `track.go` | The effort track (custom drawing on `SliderBase`) |
| `queue.go` | Queue strip: rows in `kit.SortableList` (from Lobe MyGo; drag by the handle or Space and the arrows, a drop calls `MoveQueued` before the message now after it), edit, send now, remove; a guiding row shows the loader and Withdraw |
| `attachments.go` | File cards (a picture is `kit.Image`, opening `kit.ImageViewer` over the draft's pictures), drop, paste, limit |
| `references.go` | `@`/`/` trigger, pickers, chips |
| `files.go` | File picker dialog |
| `dictation.go` | Dictation button and level meter |
| `interactions.go` | Interactions contract, `Actions`, approval and question capsules |
| `activity.go` | The activity strip: Git changes, plugin Status badges, task progress, background commands and ports, running subagents |

## Entry point

```go
func View(c *ui.Context, s *State, ctx *Context, actions Actions)
```

`State` is per conversation (the shell keeps one per ID, so drafts,
attachments, chips and choices persist when switching). `Context` is
rebuilt each frame from backend data.

## View model

| Type / field | Meaning |
|---|---|
| `Context.Models`, `Families` | Models offered and the picker's family rail |
| `Model{ID, Name, Provider, Reasoning, Efforts, Family, Score, Features}` | `Reasoning` enables the effort track; `Efforts` replaces the default `composer.Efforts` levels; `Score` feeds the tooltip; `Features` are the feature buttons |
| `Feature{ID, Name, Icon, Options}` | One option toggles; more open a menu with Off |
| `Context.Teams`, `Team{ID, Name, Members}`, `Member.Overridden` | Team row and list; an overridden member is marked |
| `Context.Queue`, `Queued{ID, Text, Attachments, Notice, Compaction, Guided}` | The queue strip, oldest first; `Guided` is guidance pending for the running turn |
| `Context.Hosts`, `Host{ID, Name, Local}` | Host menu (`Name` is an i18n key) |
| `Context.Projects`, `Project{ID, Name, Worktrees}`, `Worktree{Branch, Main}` | Location dialog and worktree menu |
| `Context.Repository{Kind, Branch}` | `Inspecting`, `Git`, `Directory`, `Unavailable`: branch label and New worktree |
| `Context.References`, `Commands`, `Reference{ID, Kind, Label, Key, Description, Icon, Alias, Insert, Children}` | The `@` and `/` catalogs; `Page`/`Folder` have `Children`; `Key` marks i18n labels; `Insert` is the prompt key, mode, model ID or effort |
| `Context.Files`, `Entry{Name, Path, Size, Modified, Mime, Children}` | The file picker's root folder; nil disables attaching |
| `Context.Last` | Up in an empty draft recalls it |
| `Context.Readonly` | Disables input and controls |
| `Context.Interactions` | Approvals and the question (below) |
| `State.Running`, `Searching`, `Metrics`, `Indicator` | Set by the page each frame |
| `State.Draft`, `Model`, `Effort`, `Mode`, `Permission`, `Features`, `Team`, `Host`, `Project`, `Worktree`, `References` | Choices, read through `Message` on send |
| `State.Attachments`, `Attachment{ID, Name, Mime, Size, Image, Path, Data, Status, Progress, Error}` | Draft files; the backend moves `Status` (`Preparing`, `Sending`, `Publishing`, `Ready`, `Failed`, `Uncertain`, `Removing`) and `Progress`; send waits for all `Ready`; at most `MaxAttachments` (8) |
| `State.Dictation{Phase, Levels}` | `Idle`, `PreparingSpeech`, `Recording` (levels 0…1), `Transcribing` |
| `Message{Text, Attachments, References, Mode, Permission, Model, Effort, Features, Team}` | What Send and Queue carry |
| `Interactions.Approvals` | Calls awaiting a decision, oldest first |
| `Interactions.Question` | The pending question, or nil |
| `Approval.ID` | The call's ID (the transcript shows the same call pending) |
| `Approval.Child` | The subagent session asking; `""` for this conversation |
| `Approval.Title`, `Context` | Tool label, and where it acts |
| `Approval.Description`, `Code` | What it does; `Code` shows a mono block up to 128 high |
| `Approval.Notices` | Lines of up to three rows |
| `Approval.Changes` | A change set: `file_changes_approval(_one)`, per file the kind (`file_changes_added/modified/renamed/deleted`), path (`a → b` for renames), `+N −M`, `file_changes_shortened` |
| `Approval.Remember`, `Prefix` | Offer `approval_allow_session`, and `approval_allow_prefix` |
| `Approval.Unrestricted` | Allow's tooltip says `approval_unrestricted` |
| `Ask{ID, Prompt, Options, Multi, Other, Declinable}` | Radio or checkbox choices, `question_other`, a free answer (`interaction_input`), and Decline for forms and links |

## Interactions

- A capsule (small, round, outlined, muted, `CircleUser`) above the input,
  right-aligned: `approval_live_count` and `question_reply`.
- Each opens a popover 320 wide anchored at its bottom right, 8 above:
  the floating fill, shadowed in light, outlined in dark.
- Approvals show one request at a time, with previous/next controls for several.
  **Deny** resolves without a reason. **Allow** has the session and prefix
  choices permitted by the captured request. The removed reason form stays absent.
- Question: the prompt (medium), choices 12 apart, the free answer, then
  Decline (when `Declinable`), `interaction_cancel` and `interaction_submit`
  (disabled until there is an answer). A new question ID starts a new draft.

## Activity strip

One row over the composer (live/activity.rs `composer_activity`): min 28
high, 16 in, 8 apart, wrapping when it is full. Left to right:

| Capsule | From `Context.Activity` | Click |
|---|---|---|
| Git `+N −M` (native: Git is core), details as tooltip | `Changes{Added, Removed, Files}`, nil outside a repository | `OpenChanges` |
| Plugin Status badges: icon, toned `Segment`s or the label, `Details` tooltip | `Badges []StatusBadge` (`Panel` nil for a button) | a button: `Badge(id, "")`; a panel: popover 320 with its body and buttons (`Badge(id, button)`) |
| `reicon:ui/tasks` `composer_todo_count`, while steps remain | `Progress []conversation.Step` | popover 320 of `conversation.Steps` |
| `background_one`/`background_count`, a spinner while `Connected` | `Commands []Command` (running and stopping) | popover 400: rows with the command (opens its `Output`), elapsed, Stop (`StopCommand`) |
| A Globe capsule per service port | `Command.Ports` | `OpenService(port)` |
| `composer_subagent_one`/`_count`, a spinner while `Connected`; hidden when `Readonly` | `Children []conversation.Child` (running) | popover 360, a row each: `OpenChild` |

Then a spacer and the approvals and question. An empty strip keeps its
row, so the composer does not move. Preview: Git totals are the Git
page's changes; the stress conversation has the goals plugin's badge, two
commands (one on port 5173) and its live turn's progress and subagent.
`Badge`, `StopCommand` and `OpenService` toast `preview_notice`.

## Actions

```go
type Actions interface {
	Send(m Message)
	Guide(m Message)
	Queue(m Message)
	Stop()
	SendQueued(id string)
	WithdrawGuidance(id string)
	SaveQueued(id, text string)
	RemoveQueued(id string)
	MoveQueued(id, before string)
	Upload(a *Attachment)
	Discard(a *Attachment)
	Dictate(d *Dictation, cmd DictationCommand)
	Compact()
	CreateWorktree()
	AddProject()
	ManageTeams()
	Decide(id string, d Decision, reason string)
	Answer(id string, a Answer)
	CancelQuestion(id string)
	DeclineQuestion(id string)
	OpenChange(path string)
	Badge(id, button string)
	StopCommand(id string)
	OpenService(port int)
	OpenChild(id string)
}
```

| Method | Fires | Expected outcome |
|---|---|---|
| `Send` | Send or Enter with input, not running, all files ready | Submit and scroll to the end; failure toasts and puts the text back in `Draft` |
| `Guide` | Enter with input while running (the button stays Stop), all files ready | `GuideTurn`: join the running turn at its next model boundary; failure toasts and puts the text back in `Draft` |
| `Queue` | Tab with input | Queue behind the running reply (send at once when idle) |
| `Stop` | Stop, or Escape while running | Cancel the run; uncertain outcomes toast |
| `SendQueued` | A queued row's Send, or Enter in an empty draft for the first message neither a notice, a compaction nor guiding | While running, `GuideQueuedTurn` (the run goes on; the row's guidance is pending); when idle, `SendQueuedTurn` |
| `WithdrawGuidance` | A guiding row's Withdraw | Return the message to the queue before the run takes it |
| `SaveQueued` | Save or Enter in a queued row's editor | Replace its text; a changed original offers Reload |
| `RemoveQueued`, `MoveQueued` | Remove; drop on a row (`before` = that row, `""` = end) | Update the queue |
| `Upload` | A file added (picker, drop, paste) or Retry | Move `Status`/`Progress` on the attachment |
| `Discard` | Remove on a card | Cancel the transfer, delete any upload |
| `Dictate` | Microphone (`StartDictation`), finish, cancel | Move `Dictation`; the transcript goes into `Draft` |
| `Compact` | `/compact` | Compact the context |
| `CreateWorktree`, `AddProject`, `ManageTeams` | Their menu rows | Open the respective flow |
| `Decide`, `Answer`, `CancelQuestion`, `DeclineQuestion` | Interactions (above) | Resolve; conflicts toast |
| `OpenChange`, `Badge`, `StopCommand`, `OpenService`, `OpenChild` | The activity strip (above) | Open Git, run the plugin's handler, stop the command, forward and open the port, open the subagent; failures toast |

## Keys and Escape

- Draft: Enter sends (guides while running; in an empty draft sends the
  first queued message), Shift+Enter breaks the line, Tab queues, Shift+Tab
  cycles the mode, Up recalls `Last` in an empty draft, Backspace after a
  chip deletes it whole.
- Picker open: Up/Down move round, Enter/Tab choose, Backspace on an empty
  query goes back a page, Escape closes it until the trigger changes.
- Escape order: references picker (in the draft) → transcript search (its
  overlay shortcut) → queue edit → stop. The window shortcut is only
  claimed (`State.Escapes`) when the picker is closed, search is closed and
  there is a queue edit or a run, with focus in the main pane.

## Preview

`internal/desktop/preview/composer.go` builds the `Context` (families, made-up
scores and features, two teams, the `@`/`/` catalogs, the file picker's
folder from the files panel tree), the queue operations (guidance
stays pending until withdrawn or the reply ends: the preview's run takes
none), `Transfers`
(preparing 200 ms, sending 1.5 s, publishing 300 ms; a name with "fail"
fails) and `Voice` (no microphone: levels from a sine, transcribing ends
with the `preview_notice` toast). Uploads, `/compact`, New worktree, Add
project and Manage teams toast `preview_notice`.

## Wiring a backend

```go
type live struct{ client *sailry.Client; c *ui.Context; session string }

func (l *live) Decide(id string, d composer.Decision, reason string) {
	if err := l.client.Decide(l.session, id, decision(d), reason); err != nil {
		l.c.Toast(failure(l.c, err))
	}
}

composer.View(c, comp, context(snapshot), &live{client, c, session.ID})
```

The production adapter is `internal/desktop/live/conversation*.go`. First send
captures the selected Node, project, worktree and provider revision, creates a
real session, retains its local composer/pane identity, then submits the input.
Existing sessions submit, guide or queue with their captured configuration
revision. Configuration saves serialize with admission and retain provider model
options. Successful command outputs bridge the short interval before the Node
feed arrives. Failures, conflicts and uncertain outcomes retain input; an explicit
identical retry retains the uncertain request ID.

Node defaults and controller remembered choices initialize new drafts. Existing
sessions reconcile effective remote configuration. Enabled Node providers supply
models, features, declared efforts and the family rail; score reads select the
nearest measured effort. The `@` catalog exposes real context and same-project
sessions; `/` exposes available models, reasoning, compaction, modes and prompts.
Later catalogs contribute neither fixture rows nor empty headings.

Queue rows use optional `Load`/`Save` callbacks to read complete input and keep
editor drafts until revision-checked saves succeed. Existing `Actions` remain the
preview fallback. Captured queue revisions protect edits and reordering.

`Context.LocationLocked` freezes admitted session locations during B4. New drafts
choose from registered projects and worktrees; the existing worktree DTO carries
the canonical path until Git inspection supplies branch labels in B5. File
transfers, worktree creation, team execution, dictation and plugin activity keep
their later-phase ownership and report unavailable when invoked before wiring.

## UI state vs backend data

| UI state (`State`) | Backend data (`Context`) |
|---|---|
| Draft, chips, attachments list, mode, permission, model, effort, features, team, host, project, worktree | Models, families, teams, hosts, projects, repository, catalogs, files |
| Pickers, popovers, dialogs, queue editor | Queue, `Running`, attachment status and progress, dictation phase |
| Reason drafts, question draft | Approvals, question |

## Tests

`internal/desktop/shell/shell_test.go`: send, Enter and stop; the model picker
(effort track, family rail, efforts submenu, non-reasoning model); the
worktree menu; Tab queueing, the `@` picker into an agent page and chip,
and the Escape order (picker before stop); approvals and the question.
`internal/desktop/conversation/bench_test.go` Scroll builds from scenes without
pictures (`preview.PlainTurns`).

## Deviations

| Deviation | Reason |
|---|---|
| Kit `DropdownButton` is a button and a chevron button with a menu | MyGo has no split button |
| Chips are `TextRanges` (primary color, weight 500), deleted whole by Backspace; no click on a chip | A MyGo text area draws no inline elements |
| The model score ring is a tooltip with axis bars | No ring component; the data is the same |
| Member badges are initials; collision handling is left to the logic half | UI-first scope |
| File picker: list view only, no grid, context menu or go-to address | Kept to the attach flow |
| Drop target tint is `Muted` mixed 12% with `Primary` | No Kit drop-zone slot in the theme |
| Plugin-contributed toolbar controls are not shown | No plugin runtime in the prototype |
| The Git changes badge is native, with the Git icon (Rust: the git plugin's Status button, Settings2 fallback icon) | Git is core here |
| The activity strip wraps when it is full | Rust keeps one row and lets capsules overflow |
| Subagent rows show no elapsed time | `conversation.Child` has none |
| Modes are Code, Plan, Flagship (Rust `WorkMode`); explore/build/review/plan are only the welcome cards | Follows Rust |
| Approval details' loading and retry, SSH trust prompts and child identicons are not shown | Not in the view model of this UI-only prototype |
