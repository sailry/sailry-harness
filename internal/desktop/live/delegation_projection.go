package live

import (
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
	"time"

	"github.com/egoist/mygo/ui"
)

func (r *Runtime) watchChild(id protocol.SessionID) {
	r.conversationSeen[id] = true
	if _, ok := r.conversationWatches[id]; !ok {
		if release, err := r.controller.WatchConversation(r.ctx, id, r.conversationChanged); err == nil {
			r.conversationWatches[id] = release
		}
	}
}

func (r *Runtime) projectChildComposer(cv *viewmodel.Conversation, comp *composer.State, ctx *composer.Context, language string) {
	ctx.Activity.Children = nil
	if comp.ActivityCallbacks != nil {
		comp.ActivityCallbacks.ChildElapsed, comp.ActivityCallbacks.StopChild, comp.ActivityCallbacks.ChildBusy = nil, nil, nil
	}
	session, ok := r.session(cv.ID)
	if !ok {
		return
	}
	if delegated(session) {
		ctx.Readonly, ctx.Child = true, true
		ctx.Interactions = composer.Interactions{}
		ctx.Activity.Children = nil
		return
	}
	state, ok := r.conversationState(session.ID)
	if !ok {
		return
	}
	runs := map[string]protocol.ConversationRun{}
	for _, child := range client.LatestConversationChildren(state.Snapshot.Page.Children) {
		childSession, valid := client.ConversationChildSession(r.snapshot(), session.ID, child)
		if !valid {
			continue
		}
		runs[childSession.ID.String()] = child.Run
		if child.Run.Status == "queued" || child.Run.Status == "running" || child.Run.Status == "stopping" {
			if child.Run.Status == protocol.ConversationStatusRunning {
				ctx.Activity.Children = append(ctx.Activity.Children, childView(&state.Snapshot.Page, child, childSession, language))
			}
			// The parent's page carries each child's run. Only a child waiting
			// on approval needs its own conversation, for the pending cards.
			if childSession.Activity.Waiting == nil || *childSession.Activity.Waiting != protocol.ActivityWaitingApproval {
				continue
			}
			r.watchChild(childSession.ID)
			if details, ready := r.conversationState(childSession.ID); ready {
				for _, approval := range details.Snapshot.Page.Approvals {
					if !childApproval(approval, childSession.ID) {
						continue
					}
					view := projectApproval(approval, callAt(details.Snapshot.Page, approval.Entry, approval.Index), language)
					view.Child = childTitle(&state.Snapshot.Page, child, language)
					ctx.Interactions.Approvals = append(ctx.Interactions.Approvals, view)
				}
			}
		}
	}
	if comp.ActivityCallbacks == nil {
		comp.ActivityCallbacks = &composer.ActivityCallbacks{}
	}
	comp.ActivityCallbacks.ChildElapsed = func(id string, now time.Time) string {
		run, ok := runs[id]
		if !ok {
			return ""
		}
		return childElapsed(run, now)
	}
	comp.ActivityCallbacks.ChildBusy = func(id string) bool {
		run, ok := runs[id]
		return ok && r.childStops[run.Turn]
	}
	comp.ActivityCallbacks.StopChild = func(id string) {
		if run, ok := runs[id]; ok {
			r.stopChild(run)
		}
	}
	enrichChildModels(cv, state.Snapshot.Page, r.snapshot(), language)
}

// childApproval reports whether the parent's conversation decides a child's
// approval: every pending one the child owns, a host key to trust included.
// The child's own tab is read-only, so nothing else could answer it.
func childApproval(approval protocol.ConversationApproval, child protocol.SessionID) bool {
	return approval.State == protocol.ConversationApprovalStatePending && approval.Session == child
}

// stopChild stops one child run through stop_turn. Its parent and siblings
// continue; the parent's page reports the child's stopping and final status.
func (r *Runtime) stopChild(run protocol.ConversationRun) {
	if r.childStops[run.Turn] || run.Status != protocol.ConversationStatusQueued && run.Status != protocol.ConversationStatusRunning {
		return
	}
	if r.childStops == nil {
		r.childStops = map[protocol.TurnID]bool{}
	}
	r.childStops[run.Turn] = true
	r.execute(protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: run.Turn}}, func(c *ui.Context, _ protocol.Output, err error) {
		delete(r.childStops, run.Turn)
		r.reportKeys(c, err, "subagent_stop_failed", "session_changed", "subagent_stop_unknown")
	})
}

func childElapsed(run protocol.ConversationRun, now time.Time) string {
	if run.StartedMs == nil {
		return ""
	}
	end := now
	if run.Status != "queued" && run.Status != "running" && run.Status != "stopping" {
		if run.FinishedMs == nil {
			return ""
		}
		end = time.UnixMilli(*run.FinishedMs)
	}
	return conversation.Elapsed(max(0, end.Sub(time.UnixMilli(*run.StartedMs))))
}

func childView(page *protocol.ConversationPage, child protocol.ConversationChild, session protocol.Session, locale string) conversation.Child {
	status := conversation.ChildCompleted
	switch child.Run.Status {
	case "queued":
		status = conversation.ChildQueued
	case "running":
		status = conversation.ChildRunning
	case "stopping":
		status = conversation.ChildStopping
	case "continued":
		status = conversation.ChildContinued
	case "failed":
		status = conversation.ChildFailed
	case "interrupted":
		status = conversation.ChildInterrupted
	case "cancelled":
		status = conversation.ChildCancelled
	}
	return conversation.Child{ID: session.ID.String(), Title: childTitle(page, child, locale), Model: session.Config.Model, Effort: effortName(session.Config.Effort), Status: status}
}

func enrichChildModels(cv *viewmodel.Conversation, page protocol.ConversationPage, snapshot protocol.Snapshot, locale string) {
	parent, err := protocol.ParseSessionID(cv.ID)
	if err != nil {
		return
	}
	views := map[string]conversation.Child{}
	for _, child := range client.LatestConversationChildren(page.Children) {
		if session, ok := client.ConversationChildSession(snapshot, parent, child); ok {
			views[session.ID.String()] = childView(&page, child, session, locale)
		}
	}
	for ti := range cv.Turns {
		for bi := range cv.Turns[ti].Reply.Blocks {
			for ci := range cv.Turns[ti].Reply.Blocks[bi].Children {
				child := &cv.Turns[ti].Reply.Blocks[bi].Children[ci]
				if view, ok := views[child.ID]; ok {
					child.Model, child.Effort = view.Model, view.Effort
				}
			}
		}
	}
}
