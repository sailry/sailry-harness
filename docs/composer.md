# Composer

B3 live construction permits an empty model catalog. `Chosen` returns an empty
model, the model chip reads `composer_select_model` and opens the Node's model
settings through `Context.SelectModel`, and Send is disabled.
Live adapters supply models and submission actions from the execution Node;
an unavailable catalog never seeds a fixture model or starts work.

The input under the transcript and on the welcome page: the capsules of
what the run waits on, the queue strip, the draft with its file cards and
inline reference chips, the `@` and `/` pickers, the toolbar (attach, mode,
permission, model controls, dictation, send/stop) and the context bar
(project, worktree, metrics), plus the file picker
(the reference implementation `conversation/live/composer.rs`, `live/queue*`,
`live/assets*`, `live/mode.rs`, `live/context.rs`, `live/sessions.rs`,
`live/attachments.rs`, `live/references*`, `live/commands`, `live/mode.rs`,
`live/permission.rs`, `live/location.rs`, `model_controls/*`,
`file_picker*`, `dictation*`, `live/approvals*.rs`, `live/questions`).

## Package and files

`internal/desktop/composer`

| File | Holds |
|---|---|
| `model.go` | **The view model**: `Context`, models, teams, queue, hosts, projects, references, files, attachments, dictation, `Message` |
| `composer.go` | `State`, `View`, the surface, draft keys, toolbar, send/stop, context bar, Escape |
| `options.go` | Mode and permission menus (with the packages' modes), project and worktree, compact settings popover. A menu's id is its composer state's (`State.menuID`), so the menus of two panes open apart |
| `models.go` | Model chip and its popover: team row, features, effort, model picker with its effort column, score tooltip |
| `track.go` | The effort track (custom drawing on `SliderBase`) |
| `queue.go` | Queue strip: rows in `kit.SortableList` (from Lobe MyGo; drag by the handle or Space and the arrows, a drop calls `MoveQueued` before the message now after it), edit, send now, remove; a guiding row shows the loader and Withdraw; the outgoing row; a child session's read-only tray |
| `overview.go` | The session overview: `Overview`, `OverviewState` and `OverviewButton`, the conversation page's header button and its popover |
| `attachments.go` | File cards (a picture is `kit.Image`, opening `kit.ImageViewer` over the draft's pictures), drop, paste, limit |
| `references.go` | `@`/`/` trigger, pickers, chips |
| `files.go` | File picker dialog |
| `dictation.go` | Dictation button and level meter |
| `interactions.go` | Interactions contract, `Actions`, approval and question capsules |
| `activity.go` | The activity strip: Git changes, plugin Status badges, task progress, background commands and ports, running subagents |
| `indicator.go` | `Indicator`, the toolbar's progress rings, drawn by `kit.Ring` |

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
| `Model{ID, Name, Provider, Reasoning, Efforts, Family, Score, Features, Capabilities}` | `Reasoning` enables the effort track; `Efforts` replaces the default `composer.Efforts` levels; `Score` feeds the tooltip; `Features` are the feature buttons; `Capabilities` (`Capability{ID, Label, Icon}`) are the row's marks and the rail's capability filter ([media.md](media.md#desktop)) |
| `Feature{ID, Name, Icon, Options}` | One option toggles; more open a menu with Off |
| `Context.Teams`, `Team{ID, Name, Members}`, `Member.Overridden` | Team row and list; an overridden member is marked |
| `Context.Queue`, `Queued{ID, Text, Attachments, Notice, Compaction, Guided, Joining}` | The queue strip, oldest first; `Guided` is guidance pending, `Joining` guidance aimed at the turn that takes guidance now |
| `Queued.Wait` | Why the row is not starting, in muted text after its label: held until started, paused (by the user, after Stop, after a turn lost its owner), or not sent when its own turn ended before its input was recorded; `live.queueWait` reads it from the item, its run and `Queue.pause_reason` |
| `Context.Held`, `HeldResult{ID, Title, Wait}` | Finished background results the Node keeps back from automatic delivery (`ConversationBackgroundTask.hold`): rows without actions after the messages, saying why (automatic limit, after Stop, after interruption, after failure, or held). The user's next turn takes them |
| `Context.Outgoing` | A queued or guiding message still on its way to the Host: the tray's last row, its actions disabled |
| `Context.Child` | A subagent's session: no composer, a read-only queue tray |
| `Context.Hosts`, `Host{ID, Name, Local}` | Execution-owner metadata; there is no composer Host picker ([removed-ui.md](removed-ui.md)). The live window leaves it empty |
| `Context.PluginModes`, `PluginMode{Key, Label, Description, Icon, Disabled, Selected}`, `Context.SelectMode` | The packages' conversation modes after the built-in ones; `SelectMode` takes a package mode's key or `code`, `plan`, `flagship` |
| `Context.Compact(focus)` | `/compact` with the text after it; nil falls back to `Actions.Compact` |
| `Context.Stale(reference)` | Whether a chosen command or skill is offered no more |
| `Context.Projects`, `Project{ID, Name, Host, Worktrees}`, `Worktree{Branch, Main}` | Project and worktree menus; `Host` is the execution Node identity |
| `Context.Repository{Kind, Branch}` | `Inspecting`, `Git`, `Directory`, `Unavailable`: branch label and New worktree |
| `Context.References`, `Commands`, `Reference{ID, Kind, Label, Key, Description, Icon, Alias, Insert, Children}` | The `@` and `/` catalogs; `Page`/`Folder` have `Children`; `Key` marks i18n labels; `Insert` is the prompt key, mode, model ID or effort |
| `Context.Files`, `Entry{Name, Path, Size, Modified, Mime, Children}` | The file picker's root folder; nil disables attaching |
| `Context.Last` | Up in an empty draft recalls it |
| `Context.Readonly` | Disables input and controls |
| `Context.Indicators`, `Indicator{ID, Percent, Loading, Tone, Hint, Start, Panel, Open, Toggle, Disabled}` | The toolbar's progress rings in order (below); empty for a draft without a session |
| `Context.SendBlocked` | Holds Send, Enter and Tab although the draft has input: the Node is away, the project or worktree is unavailable, a request is under way, or, live, no model is configured (`SelectModel` then opens the model settings from the chip) |
| `Context.MemberModels` | The models a team member may take; nil for `Models`. On another Node only that Node's own: a member's model is resolved there, and the local Node's providers reach it only as the lead of a forwarded session |
| `Context.SelectModel` | Opens the Node's model settings from the Select model chip while `Models` is empty; nil disables that chip |
| `Context.Interactions` | Approvals and the question (below) |
| `State.Running`, `Searching`, `Metrics`, `Indicator` | Set by the page each frame; `Indicator` builds the preview's ring, a live session's are `Context.Indicators` |
| `State.Draft`, `Model`, `Effort`, `Mode`, `Permission`, `Features`, `Team`, `Host`, `Project`, `Worktree`, `References` | Choices, read through `Message` on send |
| `State.Attachments`, `Attachment{ID, Name, Mime, Size, Image, Path, Data, Status, Progress, Error}` | Draft files; the backend moves `Status` (`Preparing`, `Sending`, `Publishing`, `Ready`, `Failed`, `Uncertain`, `Removing`) and `Progress`; send waits for all `Ready`; at most `MaxAttachments` (8) |
| `State.Dictation{Phase, Levels}` | `Idle`, `PreparingSpeech`, `Recording` (levels 0…1), `Transcribing` |
| `Message{Text, Attachments, References, Mode, Permission, Model, Effort, Features, Team}` | What Send and Queue carry |
| `Interactions.Approvals` | Calls awaiting a decision, oldest first |
| `Interactions.Question` | The pending question, or nil |
| `Approval.ID` | The call's ID (the transcript shows the same call pending) |
| `Approval.Child` | The subagent session asking; `""` for this conversation |
| `Approval.Title`, `Context` | Tool label, and where it acts |
| `Approval.Description`, `Code` | What it does: the call's `content`, else its first approval notice, else its `target`, else its `summary`; `Code` shows a mono block up to 128 high |
| `Approval.Notices` | Lines of up to three rows: the approval notices the description does not already show |
| `Approval.Changes` | A change set: `file_changes_approval(_one)`, per file the kind (`file_changes_added/modified/renamed/deleted`), path (`a → b` for renames), `+N −M`, `file_changes_shortened` |
| `Approval.Remember`, `Prefix` | Offer `approval_allow_session`, and `approval_allow_prefix` |
| `Approval.Unrestricted` | Allow's tooltip says `approval_unrestricted` |
| `Approval.Trust` | An SSH host key to trust: the card shows `approval_ssh_trust`, or `approval_ssh_replace` for a changed key |
| `Approval.Loading`, `Failed`, `Reload` | Details being read (a spinner) or a failed read (`chat_retry`); Allow waits for the details, Deny does not |
| `Approval.Disabled`, `Busy` | Not decidable now (host away, turn not running, a decision on its way); `Busy` marks the request being decided |
| `Approval.Again`, `Receipt` | Resend a decision whose outcome is unknown; `Receipt` marks one the run no longer lists |
| `Ask{ID, Prompt, Kind, Options, Multi, Other, Declinable}` | `AskChoice`: radio or checkbox choices, `question_other`, a free answer (`interaction_input`); Decline for forms and links |
| `Ask.Kind == AskPlan` | A plan review: `plan_execute_confirm`, `interaction_continue` and `interaction_start` |
| `Ask.Fields` (`AskForm`) | `Field{Name, Title, Description, Required, Kind}` with the bounds of its kind: text lengths, number `Integer`/`Minimum`/`Maximum`, choice `Options`/`Multi`/`MinItems`/`MaxItems` |
| `Ask.URL` (`AskLink`) | The page `question_open_url` opens |
| `Ask.Disabled`, `Busy`, `Retry` | Host away; an answer on its way; an answer whose outcome is unknown (Submit reads `chat_retry`) |
| `Answer{Choices, Text, Start, Opened, Values}` | Options by index and the text; `Start` starts coding (an empty Answer continues planning); `Opened` for a link; `Values` by field name for a form |

## Interactions

- A capsule (small, round, outlined, muted, `CircleUser`) above the input,
  right-aligned: `approval_live_count` and `question_reply`.
- Each opens a popover 320 wide anchored at its bottom right, 8 above:
  the floating fill, shadowed in light, outlined in dark.
- Approvals show one request at a time, with previous/next controls for several.
  **Deny** resolves without a reason: it has no menu and no reason form
  ([removed-ui.md](removed-ui.md)). **Allow** has the session and prefix
  choices permitted by the captured request, the only remembered scopes. An SSH host
  key shows its trust prompt. Details being read show a spinner and a failed
  read `chat_retry`; Allow waits for them.
- A request is disabled while the host is away, its turn is not running or a
  decision is on its way; the request being decided shows a spinner. One
  decision travels at a time (`approval_busy`). A decision whose outcome is
  unknown keeps its request: the toast and the request offer `chat_retry`,
  which sends that same request, and the capsule reads `approval_receipt`
  once the run lists no other request.
- Question: the prompt (medium), the inputs of its kind, then Decline (when
  `Declinable`), `interaction_cancel` and `interaction_submit` (disabled until
  the answer is valid). A new question ID starts a new draft. The body
  scrolls within half the window.
  - Choice and text: choices 12 apart, `question_other`, the free answer.
  - Plan review: `plan_execute_confirm`, then `interaction_continue` (keeps
    planning) and `interaction_start` (starts coding). The plan is the reply
    above it.
  - Form: per field its title (`Title *` when required, a
    `question_optional` checkbox that includes an optional one), its
    description, and a text area, a single-line input (a format or a
    number), `question_yes`/`question_no`, radios or checkboxes. Submit
    stays disabled until each included field fits its bounds.
  - Link: the URL, and `question_open_url`, which opens the system browser
    and answers.
- An answer on its way locks the inputs and shows a spinner in Submit; one
  whose outcome is unknown keeps the draft locked and Submit reads
  `chat_retry`, sending the same request. Cancel and Decline replace it.

## Toolbar and context bar

The reference is `conversation/live/composer.rs`, `composer/compact.rs`,
`context.rs`, `model_controls/mod.rs`, `theme.rs` (`subtle_button`,
`send_button`) and `plugins/contributions/controls/indicator.rs`.

The toolbar is one wrapping row, at least 32 high, its children 4 apart:

| Group | Wide (680 and up) | Compact (under 680) |
|---|---|---|
| Start | Attach, mode, permission, start-aligned rings, the packages' start-aligned composer controls | Attach, the settings button, start-aligned rings |
| End, pushed to the right and wrapping under the start when it does not fit | The packages' end-aligned composer controls, rings, the model chip, dictation, send or stop | Rings, dictation, send or stop |

A package's composer control is asked for with its place (`SetPluginControls`,
`ControlStart`, `ControlEnd`, `ControlMenu`): split by its alignment while the
toolbar is wide, and a row of the settings popover (`MenuRow`), after the
permission, while it is compact. An indicator is never one of them: it is a
ring. The worktree actions the location menu already has (`create_worktree`,
`fork_worktree`) are left out. A package's context-slot control is the context
bar's own button (`ContextButton`).

The context bar has one project control and at most one branch control:

- The project is the native `projectButton`, on every conversation; a chat
  without a project reads `composer_no_project`. The bundled worktrees
  package publishes the same choice as its `location` context control, which
  `live/plugin_controls.go` does not draw.
- The branch shows only for a project that is a Git repository
  (`Repository.Kind == Git`): a chat, a plain folder and a repository still
  being inspected have none. While the bundled Git package shows its
  `branch` context control, which lists branches and other checkouts, that is
  the one (`Context.PackageBranch`); otherwise it is the native
  `worktreeMenu`, with New worktree.

- Attach, the settings button, dictation and send are round, 32 across, with a
  16 icon. Mode, permission and the model chip are `subtle`: Kit's default
  button, 32 high, 10 at each end, the icon 8 before the label, round, accent
  on hover, `secondary_active` pressed. Permission is the warning color for
  project access and for full access (`permissionColor`; the danger color
  stays for errors and destructive actions), and has its label as tooltip;
  mode has none.
- The mode and permission menus are Kit's choice rows
  ([components](components.md#visual-details)): each choice's icon in a tile
  of its tint (`choice.tint`: Code and asking neutral, Plan blue, Flagship
  violet, project access teal, full access amber), its description under the
  label, a check on the chosen one. Full access has its description in the
  warning color. A package's mode, which declares no color, has one of the
  colored `theme.Tints` by its key's FNV hash (`modeTint`), so it keeps its
  color and does not read as Code.
- The model chip is at most 260 wide: the provider's initials tag, the Fast
  bolt, the name, the effort in the muted color, the other chosen features,
  8 apart. With a team it is the team's avatar and name, the lead's model and
  effort its tooltip. While `Context.Models` is empty it reads
  `composer_select_model` and calls `Context.SelectModel`
  (`model_label` and `Event::Settings`); `composer_model_empty` is only the
  picker's empty search result.
- The model picker is a page of that popover (the controls' model opens it,
  Back returns): three columns the panel's height, 8 and a 1 DIP hairline
  (`Border`) and 8 between them.
  - The rail, 48 wide: Back, the families, then the capability filter.
  - The providers' groups and their models, 236 wide, scrolling. A row is 32
    high, 8 inside, `RadiusLg`: the name, the capability marks, the score.
    The chosen model's is filled with the primary color at 12%.
  - The efforts, 132 wide, a listbox named `composer_effort` under that
    heading (12, muted): the levels of one model as rows of the same shape,
    by their IDs, each with the score measured at it (`Model.Scores`, else
    `Model.Score` at its `Tested` level) and a check on the one the model's
    row would choose. A model without efforts shows `composer_effort_unavailable`
    in the rows' place, so the panel keeps its width.
  - The column lists the chosen model's efforts until the pointer moves on
    another model's row or the focus comes to it; that row is then filled
    with `Hover` and stays so while the pointer or the focus is in the
    column. One move onto a row is enough, the first after the list opens
    too: the picker reads the pointer's place each frame, so a move is known
    from where it rested. A pointer at rest takes nothing: not from the
    keys, and not for a row the list opens under it. From the column's own
    row a pointer headed for the column, more rightwards than up or down
    (read every 6 DIPs), crosses the rows between without giving them the
    column. Changing the family, the filter or a group gives the column back
    to the chosen model.
  - A model's row chooses it at the effort chosen now where the model has a
    level of that name, else at its default (`DefaultEffort`, medium, the
    middle). An effort's row chooses the column's model at that effort.
    Either goes back to the controls with the popover open, as the efforts'
    menu did; no menu opens over the panel.
  - The controls are 296 wide and the picker 450. The popover is the page,
    its padding and, for the picker, its edge (`popoverWidth`): 320 and 476
    from the chip, 312 and 468 as the compact settings popover. The picker's
    page fills the popover, and the popover has the picker's size in the
    frame the page shows (`kit.Snapped`), not eased: a popover still on its
    way would have the columns and the last rows past its edge, where a
    press closes it. Back eases to the controls' size as before.
  - The picker's page is 320 high, the efforts' heading and nine rows, and
    the window's height less 320 where that is less, never under 160: 320 in
    a window 820 or 720 high, 240 at the smallest (560), where the models
    and the efforts scroll in that height.
  - The popover stays 8 from the window's sides. In a window too narrow for
    the three columns the rail and the efforts keep their widths and the
    models between them narrow, their names truncating, never under the
    controls' width. The production window is at least 760 wide, where the
    three columns always fit.
- Send is Kit's primary button with the arrow; Stop, the square, replaces it
  while a reply runs; while dictating without a running reply neither shows.
  Send is disabled (light: `secondary_active` fill; dark: the primary fill at
  15%; the icon the muted color at half) and does nothing, as Enter does
  nothing, unless the draft is deliverable: it has text, a file or a
  reference, its files are `Ready`, the conversation is not `Readonly`,
  `Context.SendBlocked` is unset and, to start a turn, `Context.Models` is not
  empty (`send_blocked`). Tab does not queue while `SendBlocked` is set or
  when, idle, there is no model.
- The settings popover opens over the settings button's start, 8 above, 8
  inside, 4 between rows: the model controls (or a full-width Select model
  button without a model), then the mode and the permission as rows (a small
  ghost button 32 high: 16 icon, label, 16 chevron, 8 apart; the permission
  in the control's color), each opening its page under a Back row of the same
  shape, the page's choices the menus' rows (`kit.ChoiceRow`);
  under 440 the project and the worktree follow as rows opening their menus.
- The context bar is 12 in from the surface, at least 32 high, 8 by 4 inside,
  muted, its children 4 apart: the location, then the usage at its end
  (12 apart, one button when compact). The project and the worktree are Kit's
  small round ghost buttons: 24 high, 8 at each end, a 14 icon, the label and
  the caret 4 apart, at most 192 wide, side by side on one line at normal
  widths. Wide, the bar wraps; compact, it is
  one line; under 440 the location is in the settings popover.

### Context ring

A package's indicator (`ui::Indicator`, the statistics package's `context`:
composer slot, end-aligned, order 90, no handler) is `Context.Indicators`.

- Place: in the toolbar's end group in every layout, directly before the
  model chip, or before dictation when the chip is folded away. It never
  moves into the settings popover.
- Geometry: `kit.Ring` (`kit.RingSize`), as the reference's `ProgressCircle`
  at 20, draws 15 across with a stroke of 15% of that (2.25), inside a wrapper that adds 4 on each side and does not
  shrink. With the toolbar's gap the ring ends 8 before the model chip.
- Drawing: a full track at 20% of the color, and over it the arc of `Percent`
  (0 to 100, clamped) clockwise from the top with flat ends. An unmeasured
  value is 0: the track alone. `Loading` sweeps instead: the end eases round
  in one second, the start follows through its second half; under Reduce
  Motion it is a still quarter.
- Tone: the package says it. Statistics sends muted below 80%, `warning` from
  80% and `danger` from 100%, which are the muted text, warning and danger
  colors (`kit.Neutral`, `kit.Caution`, `kit.Negative`).
- Tooltip and accessible name: `Hint`, the package's text as it is.
  Statistics sends `composer_context_usage` (`%{used} / %{limit}`, compact
  counts as `50.0k / 200.0k`), `composer_context_estimate`
  (`Estimated context %{used} / %{limit}`) for an estimate, `chat_compacting`
  while a compaction runs and `0` while the usage is unknown, which is not
  shown: an indicator without a measure that is not loading has no hint
  (`indicatorHint`), so no tooltip. These keys are
  the package's own, in its ten locales; there is no percentage, cost or
  compaction hint in it.
- Not a control: without a `Panel` the ring takes no clicks and has no
  hover fill. An indicator with a handler has a `Panel` and is a small round
  ghost button (24 high, 4 at each side) whose popover opens over its end;
  `Toggle` reports the request and `Open` is the package's answer.
- Hidden: a draft without a session has no indicators (the reference filters
  them out until `session.is_some()`), and the toolbar keeps no place for
  them. A session whose usage is unknown shows the empty ring, without a
  tooltip.

The preview's ring is `State.Indicator` (`conversation.ContextRing`, the
reference's `usage/preview.rs`), with `composer_compression_hint`. The shell
sets it for the preview alone: a live session's ring is the package's, and
both would be two rings.

## Activity strip

One row over the composer (live/activity.rs `composer_activity`): min 28
high, 16 in, 8 apart, wrapping when it is full. Left to right:

| Capsule | From `Context.Activity` | Click |
|---|---|---|
| Git `+N −M` (native: Git is core), details as tooltip | `Changes{Added, Removed, Files}`, nil outside a repository | `OpenChanges` |
| Plugin Status badges: icon, toned `Segment`s or the label, `Details` tooltip | `Badges []StatusBadge` (`Panel` nil for a button) | a button: `Badge(id, "")`; a panel: popover 320 with its body and buttons (`Badge(id, button)`) |
| `reicon:ui/tasks` `composer_todo_count`, while steps remain | `Progress []conversation.Step` | popover 320 of `conversation.Steps` |
| `background_one`/`background_count`, the loader (`kit.Loader`, 14, the capsule icons' size and muted color) while `Connected` | `Commands []Command` (running and stopping) | popover 400: rows with the command (opens its `Output`), elapsed, Stop (`StopCommand`) |
| A Globe capsule per service port | `Command.Ports` | `OpenService(port)` |
| `composer_subagent_one`/`_count`, a spinner while `Connected`; hidden when `Readonly` | `Children []conversation.Child` (running) | popover 360, a row each: `OpenChild`, and a 24 DIP `kit.IconButton` Stop (`subagent_stop`) bound by `ActivityCallbacks.StopChild`, disabled while `ChildBusy` or the child is stopping, absent without the callback |
| `composer_subagent_one`/`_count`, the loader while `Connected`; hidden when `Readonly` | `Children []conversation.Child` (running) | popover 360, a row each: `OpenChild` |

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
| `Send` | Send or Enter with a deliverable draft (above), not running | Submit and scroll to the end; failure toasts and puts the text back in `Draft` |
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
| `Decide`, `Answer`, `CancelQuestion`, `DeclineQuestion` | Interactions (above) | `live/interaction_flow.go` sends one `resolve_approval` and one `resolve_question` at a time per conversation. `Answer` sends the shape the question takes: `start_coding` with the session revision and `plan_coding_message`, or text for a plan; `form` values; `opened` after opening the link; `choices`; `text`. Toasts: closed `approval_closed`/`question_closed` (conflict), `plan_config_changed`, `question_invalid_answer`, `chat_action_unknown` with Retry (the request is kept), else `chat_action_failed`; refusals `approval_busy`, `chat_reconnecting`, `live_read_busy` |
| `OpenChange`, `Badge`, `StopCommand`, `OpenService`, `OpenChild` | The activity strip (above) | Open Git, run the plugin's handler, stop the command, forward and open the port, open the subagent; failures toast |

## Keys and Escape

- Draft: Enter sends (guides while running; in an empty draft sends the
  first queued message; a draft that is `/compact` and a focus compacts with
  it), Shift+Enter breaks the line, Tab queues, Shift+Tab cycles the mode
  through the built-in ones and the packages' enabled ones, Up recalls
  `Last` in an empty draft, Backspace after a chip deletes it whole.
- A key's and a paste's callbacks run between frames, where a MyGo context
  shows no toast and has no locale: what they need of a frame waits for the
  next one (`State.inFrame`).
- Picker open: Up/Down move round, Enter/Tab choose, Backspace on an empty
  query goes back a page, Escape closes it until the trigger changes. A row
  under a resting pointer does not take the selection from the keys: the
  pointer selects once it moves.
- Model picker: it opens with the focus on the chosen model's row. Up, Down,
  Home and End move round a column (a MyGo focus group each); Right or Tab
  go from the models to the checked effort of the column's model, where it
  has efforts; Left goes back to that model's row and Shift+Tab to the row
  that had the focus; Enter chooses the focused row; Escape closes the
  popover. ⇧⌘M opens the controls, whose model button opens the picker.
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
real session, retains its local composer/pane identity, saves its selected team
and explicit member overrides, then submits the input.
Existing sessions submit, guide or queue with their captured configuration
revision. Configuration and team saves serialize with admission and retain provider model
options. Successful command outputs bridge the short interval before the Node
feed arrives. Failures, conflicts and uncertain outcomes retain input; an explicit
identical retry retains the uncertain request ID. A conversation retains one
uncertain turn request, as the reference does. It serves a retry only while the
composer and the observed runs and queue are what they were when it was sent; a
changed draft, a moved history or queue, another turn command or a definite
answer drops it, so the same text sent later is a new request. A turn or queue
item carrying the retained request ID completes the send that lost its result
and clears its draft.

Node defaults and controller remembered choices initialize new drafts. Existing
sessions reconcile effective remote configuration. Enabled Node providers supply
models, features, declared efforts and the family rail; score reads select the
nearest measured effort. The `@` catalog exposes real context and same-project
sessions; `/` exposes available models, reasoning, compaction, modes and prompts.
Later catalogs contribute neither fixture rows nor empty headings.

Guidance sent from the composer is followed until its fate is known
(`live/composer_context.go`, `guidanceOutcome`): one neutral toast says that it
is queued as its own turn, either because its settings differ from the running
turn's (`queue_guidance_separate`, also the answer to `guide_queued_turn`) or
because the reply ended before it could join (`queue_guidance_outlived`).
Guidance the user edited, paused or withdrew is not reported. The Rust desktop
shows neither the row reasons, the held results nor the second toast; they
exist because the Go Node reports `pause_reason` and `hold`.

Queue rows use optional `Load`/`Save` callbacks to read complete input and keep
editor drafts until revision-checked saves succeed. Existing `Actions` remain the
preview fallback. Captured queue revisions protect edits and reordering.

`Context.LocationLocked` freezes admitted session locations during B4. New drafts
choose from registered projects and worktrees; the existing worktree DTO carries
the canonical path until Git inspection supplies branch labels in B5. File
transfers and worktree creation use their live workspace owners.

### Dictation

Dictation is the controller's, whichever Node the conversation runs on
(COMP-041 to COMP-043). `live/dictation.go` owns it on the local runtime and
moves `State.Dictation`; the view is unchanged.

- Start leases the selected speech model (`dictation.model`) from
  `internal/speech/models` and shows `PreparingSpeech`. A worker verifies the
  model's files, asks for the microphone (`capture.Request`), constructs the
  recognizer (`internal/speech`, sherpa-onnx through purego) with
  `dictation.language` and opens `dictation.microphone` or the system default
  (`internal/speech/capture`, miniaudio through purego). Nothing is
  downloaded: a missing model opens Settings › Dictation.
- `Recording` mixes the device's frames to mono at 16 kHz every 20 ms and
  appends a level (RMS on a 60 dB scale) to `Dictation.Levels` every 60 ms;
  the button draws the last four. After each further second of audio, at most
  every 800 ms, the cumulative audio is decoded and the hypothesis replaces
  the previous one at the caret. Five minutes finish the recording.
- Finish shows `Transcribing`, stops the device, decodes everything and puts
  the transcript in the draft through `State.DictationCaret` and
  `State.ReplaceDictation`. An empty transcript is the `dictation_empty`
  message; a draft edited under the partial transcript stops the dictation
  with `dictation_draft_changed`.
- Cancel (a click while preparing or transcribing) leaves the button idle at
  once and drops later results. The worker keeps the model's lease until its
  decode returns; a dictation started meanwhile is `dictation_busy`, as is a
  second composer's while one dictates.
- Failures are toasts and keep the draft: `dictation_permission`,
  `dictation_unavailable` (libraries absent), `dictation_model_invalid`,
  `dictation_microphone`, `dictation_no_audio`, `dictation_silent`,
  `dictation_failed`. A read-only conversation does not start.
- Audio stays in memory and is never stored or sent.

Team choices use the Node's catalog and the session's frozen role profiles.
`State.MemberModels` separates member edits from the lead model controls;
inherited choices do not become explicit overrides. A member's picker lists
`Context.MemberModels` when the context has them (`controlModels`). Team and member saves retain
their original revision and uncertain request identity. First input, guidance
and queued input wait for successful saves; failures preserve the draft.
Existing sessions retain their captured role references in `@ Agents`; unsent
drafts use the current catalog. Skills come from enabled installed packages read
at their exact digest and settings revision, and chips retain `package:skill`.

Child sessions have no composer (`Context.Child`): only the queue tray shows,
read-only, with Read (the message in a read-only editor with Copy, Close and
Remove), Withdraw and Remove, which act while the session's Host is connected.
Parent composers
project active registered children into the existing activity capsule and show
every pending approval of a child with the child's name, an SSH host key to
trust included: the child's own tab is read-only, so the parent is the only
place to decide it. Approval decisions target the child session; opening a row
observes its real transcript.

### B7 projects and services

The project menu contains local and paired execution Nodes' registered projects.
Choosing a project before first send captures its Node, resets the worktree to
that project's first entry and reads that Node's available models and teams.
Display names are labels, not routing identities. Existing admitted sessions
keep their captured Node and location.

The composer has no Host selector and no location dialog
([removed-ui.md](removed-ui.md)): a draft runs on its project's Node, and a
chat without a project on the Host the shell opened it for (`App.Host`). A
draft on another Host is offered that Host's providers and this desktop's,
which the session is forwarded with (below); the model list names each
provider's source (`chat_provider_source`: the Host's name, or
`chat_config_desktop`), and a forwarded session's names its private provider
`chat_provider_session` beside the Host's `chat_provider_node`.

### Modes of packages

`live/composer_context.go` (`projectComposerModes`) lists the conversation
modes Host packages declare (`PluginExtension.ConversationModes`) after Code,
Plan and Flagship, in the mode menu, the compact settings popover's mode page
and the Shift+Tab cycle; a disabled package's mode shows disabled only while
the session has it. A package mode is its package's option in the session
configuration over a built-in base mode (`client.SelectMode`: Code when the
mode may write, Plan otherwise); choosing any mode rewrites the composer's
base configuration, which is saved as every other change of it. The row's
description is the package's.

### Queue outcomes

A queue command that fails with a conflict toasts `session_changed`; any other
failure is a toast with Retry, which sends the same command again
(`queueCommand`). Send now on a queued row while a turn runs asks the turn to
take it as guidance; when the Host leaves it queued because its settings
differ from the turn's, the `queue_guidance_separate` toast says so. A message
being queued or sent as guidance is the tray's last row until the Host
answers (`Context.Outgoing`).

### Assistant panel sessions

A conversation a package mounts for one of its assistants
(`live/plugin_native_conversation.go`) has New and the session history beside
its heading (the reference's `session_controls`). The history is a searchable
list of the assistant's sessions for the same resource, project and worktree,
neither archived nor delegated, the shown one checked; choosing one moves the
mount to it. New moves it to a draft that keeps the composer's model, effort
and mode and asks before acting. Both are disabled while a turn runs or the
Host is away. A session without a message shows `chat_sidebar_empty`.

### Session overview

`live/session_overview.go` fills `composer.Overview` for the header's button
(`shell.Hooks.Overview`): the running subagents, the current turn's task
progress and cited pages from the transcript, and the session's outputs and
sources, read with `list_conversation_assets` twenty groups at a time when
the popover opens and what it holds is behind the history. Rows show the
newest occurrence of each target. A row opens the subagent's session, the
file preview, the workspace's Files tab for a source file, or the link.
`chat_assets_more` reads the next page; a failed read toasts
`chat_assets_failed` and leaves Retry in the popover. A draft's button is
disabled. The preview builds its overview from the scene
(`preview.Overview`); its file rows toast `preview_notice`.

`live/commands.go` projects the captured session's running command services.
The strip shows one Globe capsule per distinct port, disables it while the
connection is unavailable or a forward is being opened, and labels a listening
forward as `remote → local`. `OpenService` freezes the session, command and
service identity, asks that Client to forward the port, then opens the confirmed
loopback URL through the window's native browser callback: the URL the command
announced with only its host and port replaced by the local listener
(`forwardURL`, the reference's `local_url`), so an `https` scheme, a path and a
token query survive. A forward opened from the ports list has no announced URL
and opens the listener's root over HTTP. A service removed from the observed
running command closes its associated Forwarder.

Forwarding state belongs to the window and is keyed by Node and remote port.
Another Node's matching port cannot supply the label or browser address.
Connection loss changes the existing banner and service controls; it does not
move the draft to a different Node or stop admitted work.

### Plugin contributions

`live/plugin_controls.go` projects metadata-declared composer, model, context,
statistics, footer and header contributions into their existing slots. Registry
keys retain the Node, worktree, session and each unsent draft's presentation
identity. Generations fence old callbacks and simultaneous drafts. Shared
assistant contributions follow the package's `ui_shared` and `ui_overlay`
declarations.

`live/plugin_status.go` consumes the Node-owned Status topic through Client.
Badges preserve plugin ordering, toned segments and tooltips; Files and Git
package entries are excluded. Popovers mount the package's immutable UI entry.
Actions keep the original request through failure, conflict or uncertain
completion. A returned command intent can open a hidden status popover.

Message commands use ordinary submit actions. Node resolves a declared leading
slash command before model admission, captures its callback scope, and records
the parent request's durable outcome. Repeating that request cannot reroute it
through a model or repeat the callback after inventory changes. Ordinary text
continues through the existing agent loop.

A package's location control can give an unsent project draft a session base:
a display name, branch and commit. `live/session_base.go` keeps it with that
draft until its project or worktree changes. The first send or first attachment
creates a managed worktree from the base, moves the draft into it and continues
with the ordinary session creation. The draft is busy meanwhile. The send
that started the worktree continues as it was captured: text, references or
attachments entered while the worktree is created stay in the composer. A failure
leaves the draft and base unchanged; an uncertain result retries its captured
request rather than creating a second worktree.

### Session creation and forwarding

A draft's first send or first attachment creates its session
(`live/conversation_create.go`). A draft on the local Node, or on another Host
with a model of that Host's own provider, sends `create_session` to that Node.
A draft on another Host also offers the local Node's enabled providers
(`composerProviders`, `offeredModels`); with one of their models the Desktop
sends `create_session_at` through the local Node's controller, with the Host
as `target`, the local provider's ID, its `credential` in `config` and its
revision as `provider_revision`. The session that returns carries a private
copy of the provider (`Session.Profile`): its composer shows and saves with
that copy, and offers it beside the Host's own providers.

A refused creation keeps the draft. `not_configured` (the provider is gone, or
its Claude or ChatGPT sign-in stays with the Node it was made on) toasts
`chat_config_unavailable`; a revision conflict toasts `session_changed`; an
unconfirmed outcome toasts `chat_action_unknown` and the next send repeats
the same request.

### Connection references

`live/references_connections.go` fills the `@` Databases and SSH pages from the
conversation's execution Node. Profiles follow the bound resource or their
sharing with the conversation's project. A database page reads its databases
and tables with `browse_database`; results are cached by session, profile
revision and database, so a late answer cannot reach another composer. A failed
read shows the empty page and one failure toast. Sent chips open a read-only
details dialog.

## UI state vs backend data

| UI state (`State`) | Backend data (`Context`) |
|---|---|
| Draft, chips, attachments list, mode, permission, model, effort, features, team, host, project, worktree | Models, families, teams, hosts, projects, repository, catalogs, files |
| Pickers, popovers, dialogs, queue editor | Queue, `Running`, attachment status and progress, dictation phase |
| Question draft, explicit member model drafts | Parent and child approvals, question, frozen team roster |
| Selected project before admission and window-owned port actions | Project-to-Node identity, command services and Forwarder state from the captured Client |

## Live attachments

A paste attaches the copied files and folders by their paths, or, when the
clipboard has no text, a copied picture (PNG, which the system's own picture
formats are read as, JPEG, GIF or WebP) named `chat_clipboard_image` by its
place among the draft's files. A batch that would pass eight attachments is
refused whole with `chat_attachment_limit`, as a drop or a pick is. Uploads go
to the Host that owns the draft or session. A dropped or pasted folder is
attached by its path and refused by the upload (`files_upload_source`), as the
reference has no folder upload either.

`live/attachments_*.go` owns draft transfer state and optional native picker,
remove and image callbacks through `composer.AttachmentCallbacks`. Existing
`Actions` and card layout stay unchanged. The first attachment captures a real
session and worktree without submitting a turn. At most two uploads run at once;
source size and BLAKE3 are verified before durable publication. Failures keep
the draft. Uncertain publication or discard keeps the original request for
explicit outcome inspection and retry. Removal retains the card until cleanup
finishes; sending drops controller ownership only after successful admission.

Ready images hydrate from verified Node streams with bounded bitmap decoding
outside the UI thread. Caches belong to each displayed conversation, including
split panes. File references and Request edit use ordinary draft tokens and
focus without sending. Native callbacks are composed by `cmd/sailry-desktop`.

## Tests

`internal/desktop/shell/shell_test.go`: send, Enter and stop; the model picker
(effort track, family rail, the effort column after one pointer move with
every row inside the popover in the frame the list opens, an effort's row
choosing a model the pointer is on with the one popover left open,
non-reasoning model; `pickerInShell`, its step in `composer_models_test.go`,
has the app's window at 1280×820, 1280×720, 760×820 and 760×560, at one and
two pixels a DIP: the popover in the window and every model and effort row
whole inside it as the list opens and at rest, the list 236 wide, the same
choice by an effort's row, and the controls' size again after Back;
`SAILRY_SNAPSHOTS` set saves
`.runtime/ui-shots/composer-model-picker-zh-CN-{dark,light}.png`); the
worktree menu; Tab queueing, the `@` picker into an agent page and chip,
and the Escape order (picker before stop); approvals and the question.
`internal/desktop/shell/composer_parity_test.go`, run as steps of
`TestComposerGuidesRunningTurn` and `TestCloseShortcut`: the packages' modes
in the menu, the cycle and the compact popover; the absent host choice; the queue
row states and the outgoing row; a child session's tray; pasted files,
pictures and text, dropped files and folders and the limit; `/compact` with a
focus; the stale toast; the header's Search and overview buttons and the
conversation's keys. `internal/desktop/live/composer_parity_test.go`, run as a
step of `TestComposerSendRouting` over a fixture Node, checks the commands
they send (`compact_context` with its focus, `set_session_config` with the
package's option, `guide_queued_turn`, the retried `remove_queued_turn`,
`list_conversation_assets` and its paging, `stop_turn`, a child's
`read_queued_turn` and `remove_queued_turn`); `testSessionForwarding` the
provider source names and the absent host choice; `testNativeAssistantSessions`
(`plugin_native_test.go`) an assistant panel's New and history.
`auditSessionOverview` (`composer_parity_integration_test.go`, a step of
`TestPluginUIIntegration`) opens the overview on a session of the real Node.
`internal/desktop/shell/footer_test.go` (`TestContextRingBesideModel`): the
session ring's size, place before the chip and the microphone, that it is no
control, its arc and tones by pixel, its tooltip, the rule without a session,
the folds under 680 and 440 with the places the packages' controls are asked
for, and Send without a model, blocked and with one, by the button and by
Enter, which leaves a draft its owner keeps.
`SAILRY_UI_SHOTS=<directory>` saves those steps' frames for review.
`composerMenus` (`composer_menus_test.go`, a step of the composer parity):
the permission menu in dark and light, the chosen row checked and unfilled
and the row under the pointer filled, by pixel; the Full access control and
description in the warning color; a package mode's tile tinted, the same in
two windows; the settings popover's permission page with the same rows.
`SAILRY_SNAPSHOTS` set saves `.runtime/ui-shots/composer-menus-zh-CN-{dark,light}.png`.
`composerModels` (`composer_models_test.go`, a step of the composer parity):
every model and effort row whole inside the one popover in the frame the
list opens and at rest, from the chip (960 wide) and from the compact
settings popover (440), at one and two pixels a DIP, with an effort's row
choosing the model the pointer moved onto; the effort column right of the
models, in dark and light, with a hairline between them by pixel; no menu
or second popover over a model with efforts; the column after one pointer
move and under the focus, with the effort a model's row keeps or falls back
to; a pointer at rest under the keys and under a row the list opens, and
its way to the column; the line and the unchanged panel for a model without
efforts; an effort's row choosing its model and leaving the popover open on
its controls; the scores by effort; Right, Tab, Left, Shift+Tab, Up, Down,
Enter and Escape; the narrow window; and no frame asked for at rest.
The Kit menu's test (`kit/menu_test.go`, `choice rows`) checks the row alone
and that an open menu at rest asks for no frame.
The ring tooltip step also keeps an unknown ring drawn and without a tooltip.
`internal/desktop/live/first_run_test.go` (`firstRunLocation`), with the
bundled packages mounted: one location control on a chat and on a plain
folder, the project and its branch on one line in a Git project.
`TestUsageCompact`: the small location buttons before the usage button between
the two widths, and no location on the bar below 440.
`internal/desktop/conversation/bench_test.go` Scroll builds from scenes without
pictures (`preview.PlainTurns`).
`internal/desktop/live/dictation_test.go` (in `TestWorkspaceHookSettings`):
the dictation's phases, its insertion at the caret, cancellation in each phase
and every failure, over a fake microphone and recognizer.

## Deviations

| Deviation | Reason |
|---|---|
| The worktrees package's `location` control is not drawn, and the native worktree menu gives way to the Git package's `branch` control (user decision 2026-11-11) | The reference draws the packages' controls only; here the project is native, so both showed the location twice |
| An unknown indicator has no tooltip | The statistics package sends `0` as its hint, which read as a bare `0` |
| A refused microphone is the `dictation_permission` toast | The reference's permission card is not part of the accepted composer |
| The recording meter is the capture level the live owner computes | The reference's Kit speech state supplies it; MyGo has none |
| Kit `DropdownButton` is a button and a chevron button with a menu | MyGo has no split button |
| Chips are `TextRanges` (primary color, weight 500), deleted whole by Backspace; no click on a chip | A MyGo text area draws no inline elements |
| The model score ring is a tooltip with axis bars | No ring component; the data is the same |
| A model's efforts are a column of the picker's panel, not a menu beside its row (user decision 2026-11-11) | The menu floated over the list and left the panel's side empty; the column is the one place to choose an effort |
| A model's row chooses it at once, at the effort chosen now or its default | With the menu gone a row no longer opens anything; the column shows that effort checked before the click |
| The popover takes the picker's size at once (`kit.Snapped`) instead of easing to it | A popover easing to the picker's size (150 ms, and none of it on a clock that stands still) had the effort column and the last rows past its edge, where a press closed the popover without choosing |
| The picker's rows are `ButtonBase` rows, the efforts with the option role in a listbox | A MyGo list or menu row has no place for the capability marks and the score with its tooltip, and the two columns share one row |
| `kit.Ring` draws the progress ring itself | Pinned MyGo has a progress bar and a spinner of spokes, no progress circle. The composer, `conversation.ContextRing` and the plugin controls share the one drawing |
| A ring's value changes at once | Kit's `ProgressCircle` eases a changed value over its normal duration |
| The preview's ring shows the scene's usage | The reference's preview ring is always empty; ours is the same `kit.Ring`, 15 across |
| An indicator's `ID` is the package's name and the entry's ID | Two packages may declare the same entry ID; the ring's key has to be unique |
| The model slot's package controls are not segments of the model chip | Out of this round; they stay the slot's own controls before the rings |
| Enter and Tab leave the editor what the owner left in `State.Draft` | The live owner keeps the draft until the send is confirmed; clearing the editor after the action lost it on the keyboard path only |
| The model name truncates at its end | The reference truncates it in the middle and measures the chip to fold the effort into an icon (`model_controls/fit.rs`); a MyGo text has an end ellipsis only |
| Member badges are initials; collision handling is left to the logic half | UI-first scope |
| File picker: list view only, no grid, context menu or go-to address | Kept to the attach flow |
| Drop target tint is `Muted` mixed 12% with `Primary` | No Kit drop-zone slot in the theme |
| Plugin indicators draw their progress arc using the shared semantic palette | Pinned MyGo has no matching progress-circle control |
| Plugin menu groups use disabled Kit rows and the standard menu row height | Kit has no separate menu-heading control; existing menu styling is retained |
| The Git changes badge is native, with the Git icon (Rust: the git plugin's Status button, Settings2 fallback icon) | Git is core here |
| The activity strip wraps when it is full | Rust keeps one row and lets capsules overflow |
| Live service labels use additive activity callbacks | Forwarder state stays with the captured Client while the accepted command DTO and capsule layout remain unchanged |
| Subagent elapsed uses an additive activity callback | Captured run timestamps remain outside the accepted `conversation.Child` fields |
| A subagent row's Stop uses additive activity callbacks (`StopChild`, `ChildBusy`) | Stopping one child was missing in both runtimes; the `Actions` contract stays unchanged and the preview shows no Stop |
| Modes are Code, Plan, Flagship (Rust `WorkMode`); explore/build/review/plan are only the welcome cards | Follows Rust |
| Child identicons are not shown in approvals | Not in the view model |
| Forms show no schema defaults or per-field messages | The question protocol carries no defaults; an invalid field disables Submit and a rejected answer toasts `question_invalid_answer`, as the reference does |
| The approvals popover stays open after a decision | The request shows its busy state there; the reference closes it |
| A live Status badge's panel has no buttons | The package draws its own panel (`Render`), so `PanelButton` rows exist only in preview data; `Badge(id, button)` has nothing to run for one |
| The overview's rows have file-kind icons, not picture thumbnails | The reference loads each output picture for its row; here a row opens the preview, which loads it |
| The overview does not reopen after a picture's viewer closes | The reference reopens it; the panel's preview stays open here instead |
| The assistant panel's history is `kit.Command` | The reference uses its searchable list dialog; the same search, check and Enter |
| Approval details' loading and retry, SSH trust prompts and child identicons are not shown | Not in the view model of this UI-only prototype |

Live file references resolve the captured worktree through the existing shared
Files owner. Opening an `@ Files` page loads that directory lazily; the open page
reads its current shared listing rather than retaining a stale copy. Folder
selection records a canonical `directory` reference; file selection records a
`file` reference. Both retain their exact relative paths when an edited or queued
message is restored. Attachment picks use the operating system's local file
chooser, matching the reference's `choose_attachments`; selected local bytes are
uploaded to the execution Node. The fixture browser remains isolated preview UI.
