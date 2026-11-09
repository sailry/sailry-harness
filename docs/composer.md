# Composer

The input under the transcript: the capsules of what the run waits on, the
draft, attachment and dictation buttons, the model chip and its picker,
send and stop, and the context bar with the branch menu
(sailry-public `conversation/live/composer.rs`, `model_controls`,
`live/approvals*.rs`, `live/questions/view.rs`, `theme/composer.rs`).

## Package and files

`internal/composer`

| File | Holds |
|---|---|
| `composer.go` | `State`, `Model`, `View`, the input, model picker, send/stop, branch menu |
| `interactions.go` | **The contract** for interactions and `Actions`; the approval and question capsules and popovers |

## Entry point

```go
func View(c *ui.Context, s *State, models []Model, in Interactions, actions Actions)
```

## View model

| Type / field | Meaning |
|---|---|
| `Model{ID, Name, Provider, Reasoning}` | A model the picker lists; `Reasoning` enables the effort slider |
| `State.Running` | Set by the page from the run: Stop replaces Send |
| `State.Draft`, `Model`, `Effort`, `Branch` | UI state the user chooses; read them on send |
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
- Approvals: a list up to 256 high, rows separated by a top border. Each row
  ends with **Deny** (ghost, menu: `approval_deny_reason`) and **Allow**
  (`approval_allow_once`; primary on the first row; menu with the session
  and prefix choices when `Remember`). Deny with reason shows a reason field
  with `approval_reason_cancel` and `approval_reject`.
- Question: the prompt (medium), choices 12 apart, the free answer, then
  Decline (when `Declinable`), `interaction_cancel` and `interaction_submit`
  (disabled until there is an answer). A new question ID starts a new draft.

## Actions

```go
type Actions interface {
	Send(text string)
	Stop()
	Decide(id string, d Decision, reason string)
	Answer(id string, a Answer)
	CancelQuestion(id string)
	DeclineQuestion(id string)
}
```

| Method | Fires | Expected outcome |
|---|---|---|
| `Send` | Send or Enter with a non-empty draft, not running | Submit the turn and `ScrollToEnd` the transcript; failure toasts and keeps the text (put it back in `Draft`) |
| `Stop` | Stop while running | Cancel the run; uncertain outcomes toast |
| `Decide` | Allow, a menu choice, Deny, or Deny with reason | Resolve the approval; a decision the run no longer waits on toasts the conflict |
| `Answer` | Submit with an answer (`Choices` by index, `Text`) | Resolve the question; failure toasts and keeps the draft |
| `CancelQuestion` | Cancel | Cancel the question |
| `DeclineQuestion` | Decline | Decline it |

The preview toasts `approval_preview_result` for decisions and
`preview_notice` for answers; nothing runs.

## Wiring a backend

```go
type live struct{ client *sailry.Client; c *ui.Context; session string }

func (l *live) Decide(id string, d composer.Decision, reason string) {
	if err := l.client.Decide(l.session, id, decision(d), reason); err != nil {
		l.c.Toast(failure(l.c, err))
	}
}

composer.View(c, comp, models, interactions(snapshot), &live{client, c, session.ID})
```

## UI state vs backend data

| UI state (`State`) | Backend data |
|---|---|
| Draft, model, effort, branch | Models |
| Picker and popovers open | `Running` |
| Reason drafts, question draft | Approvals, question |

## Tests

`internal/shell/shell_test.go`: send and Enter, stop, the model picker and
effort, the branch menu, approvals (Allow resolves one and toasts) and the
question (choose and submit).

## Deviations

| Deviation | Reason |
|---|---|
| Kit `DropdownButton` is a button and a chevron button with a menu | MyGo has no split button |
| Approval details' loading and retry, SSH trust prompts and child identicons are not shown | Not in the view model of this UI-only prototype |
