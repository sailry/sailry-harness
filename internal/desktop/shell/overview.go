package shell

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The overview card's room at the pane's end: the card, its inset from the
// edge and its gap from the transcript. A conversation shown alone floats it
// in three tiers of width. From overviewCenterMin the transcript is centered
// and the card sits in its end margin. Between overviewSideMin and that, the
// transcript moves toward the start just enough to keep the card's room,
// until at overviewSideMin the two stand side by side with no margin left.
// Narrower, or in a pane of a split, the overview is the header button's
// popover.
const (
	overviewSide      = composer.OverviewWidth + 2*overviewCardInset + 8
	overviewSideMin   = theme.ContentWidth + overviewSide
	overviewCenterMin = theme.ContentWidth + 2*overviewSide
)

// overviewAside is the room a transcript w wide keeps clear at its end for
// the card (conversation.State.Aside): none where centering already leaves
// it, then growing as w narrows, so the transcript shifts without a jump.
func overviewAside(w float32) float32 {
	return min(max(overviewCenterMin-w, 0), overviewSide)
}

// overviewCardInset is the floating card's distance from the pane's top and
// end; overviewCardMax its greatest height, past which it scrolls within.
const (
	overviewCardInset = 12
	overviewCardMax   = 520
)

// conversationOverview is cv's overview with what its transcript tells:
// the current turn's status and the tool call under way, and the files its
// turns changed, which act reviews.
func (a *App) conversationOverview(c *ui.Context, cv *preview.Conversation, act ConversationActions) *composer.Overview {
	o := a.sessionOverview(c, cv)
	if !o.Session && !a.sampled() {
		return o
	}
	if t := currentTurn(cv.Turns); t != nil {
		o.Turn, o.Status, o.Elapsed = true, t.Reply.Status, t.Reply.Elapsed
		if call, ok := runningCall(t); ok {
			o.Doing = call.Label
			if o.Doing == "" && call.LabelKey != "" {
				o.Doing = i18n.T(c, call.LabelKey)
			}
			o.DoingDetail, _, _ = strings.Cut(strings.TrimSpace(call.Detail), "\n")
		}
	}
	o.Changes = changedFiles(cv.Turns)
	if act != nil {
		o.OpenChange = act.Review
	}
	return o
}

// currentTurn is the turn running, else the last that started; nil for
// none. A compaction and a queued message are not turns of their own here.
func currentTurn(turns []conversation.Turn) *conversation.Turn {
	var last *conversation.Turn
	for i := len(turns) - 1; i >= 0; i-- {
		t := &turns[i]
		if t.Compaction != nil || t.Reply.Status == conversation.Queued {
			continue
		}
		if t.Reply.Status.Active() {
			return t
		}
		if last == nil {
			last = t
		}
	}
	return last
}

// runningCall is the last tool call of t still running.
func runningCall(t *conversation.Turn) (conversation.Call, bool) {
	for i := len(t.Reply.Blocks) - 1; i >= 0; i-- {
		calls := t.Reply.Blocks[i].Calls
		for j := len(calls) - 1; j >= 0; j-- {
			if calls[j].Running {
				return calls[j], true
			}
		}
	}
	return conversation.Call{}, false
}

// changedFiles are the files the turns changed and did not undo, each once
// in the order first changed: the lines of every turn's change summed, and
// the last turn that changed it.
func changedFiles(turns []conversation.Turn) []composer.OverviewChange {
	var out []composer.OverviewChange
	at := map[string]int{}
	for _, t := range turns {
		ch := t.Reply.Changes
		if ch == nil || ch.Undone[""] {
			continue
		}
		for _, f := range ch.Files {
			if ch.Undone[f.Path] {
				continue
			}
			i, seen := at[f.Path]
			if !seen {
				i = len(out)
				at[f.Path] = i
				out = append(out, composer.OverviewChange{Path: f.Path})
			}
			out[i].Turn = t.ID
			out[i].Added += f.Added
			out[i].Removed += f.Removed
		}
	}
	return out
}
