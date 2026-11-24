package live

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) watchChild(id protocol.SessionID) {
	r.conversationSeen[id] = true
	if _, ok := r.conversationWatches[id]; !ok {
		r.startConversationFeed(id, "")
	}
}

func (r *Runtime) projectChildComposer(cv *viewmodel.Conversation, comp *composer.State, ctx *composer.Context, language string) {
	ctx.Activity.Children = nil
	comp.Stoppable = false
	if comp.ActivityCallbacks != nil {
		comp.ActivityCallbacks.ChildElapsed, comp.ActivityCallbacks.StopChild, comp.ActivityCallbacks.ChildBusy = nil, nil, nil
		comp.ActivityCallbacks.StopChildren = nil
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
	snapshot := r.snapshot()
	targets := map[string]client.ConversationChildTarget{}
	var models map[protocol.SessionID]childModel
	if read := r.overviews[session.ID]; r.conversationCopies != nil && read != nil {
		models = read.childModels
		clear(models)
	}
	for target := range r.liveChildren(state) {
		child, childSession := target.Child, target.Session
		avatar := childAvatar(snapshot.Roles, child)
		if target.Listed {
			if models == nil {
				models = make(map[protocol.SessionID]childModel)
			}
			models[childSession.ID] = childModel{childSession.Config.Model, effortName(childSession.Config.Effort), avatar}
		}
		if runStatus(child.Run.Status).Active() {
			targets[childSession.ID.String()] = target
			title := liveChildTitle(&state.Snapshot.Page, target, language)
			if child.Run.Status == protocol.ConversationStatusRunning {
				view := childView(nil, child, childSession, language)
				view.Title = title
				view.Avatar = avatar
				view.Unsynced = !state.Connected
				ctx.Activity.Children = append(ctx.Activity.Children, view)
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
					view.Child = title
					ctx.Interactions.Approvals = append(ctx.Interactions.Approvals, view)
				}
			}
		}
	}
	enrichChildModels(cv, models)
	if read := r.overviews[session.ID]; r.conversationCopies != nil && read != nil {
		read.childModels = nil
		if len(models) > 0 && len(models) <= liveChildProjectionLimit {
			read.childModels = models
		}
	}
	if len(targets) == 0 {
		return
	}
	if comp.ActivityCallbacks == nil {
		comp.ActivityCallbacks = &composer.ActivityCallbacks{}
	}
	comp.ActivityCallbacks.ChildElapsed = func(id string, now time.Time) string {
		target, ok := targets[id]
		if !ok {
			return ""
		}
		return childElapsed(target.Child.Run, now)
	}
	comp.ActivityCallbacks.ChildBusy = func(id string) bool {
		target, ok := targets[id]
		return ok && (!target.Listed || r.childStops[target.Child.Run.Turn])
	}
	parent, revision := session.ID, state.Snapshot.Page.Revision
	comp.ActivityCallbacks.StopChild = func(id string) {
		if target, ok := targets[id]; ok && target.Listed {
			if target.Background != nil {
				fresh, ready := r.controller.Conversation(parent)
				if !ready || fresh.Snapshot.Page.Revision != revision {
					return
				}
				for current := range r.liveChildren(fresh) {
					if current.Child.Run.Turn == target.Child.Run.Turn && target.MatchesSession(current.Session) {
						r.stopChild(parent, target.Child.Run)
						return
					}
				}
				return
			}
			r.stopChild(parent, target.Child.Run)
		}
	}
	// A failed reply hands its subagents to background delivery; the
	// conversation's Stop still reaches them while they run.
	stop := comp.ActivityCallbacks.StopChild
	comp.ActivityCallbacks.StopChildren = func() {
		for id := range targets {
			stop(id)
		}
	}
	for _, target := range targets {
		if status := target.Child.Run.Status; target.Listed && (status == protocol.ConversationStatusQueued || status == protocol.ConversationStatusRunning) {
			comp.Stoppable = !comp.Running
		}
	}
}

// childApproval reports whether the parent's conversation decides a child's
// approval: every pending one the child owns, a host key to trust included.
// The child's own tab is read-only, so nothing else could answer it.
func childApproval(approval protocol.ConversationApproval, child protocol.SessionID) bool {
	return approval.State == protocol.ConversationApprovalStatePending && approval.Session == child
}

// stopChild stops one child run through stop_turn. Its parent and siblings
// continue; the parent's page reports the child's stopping and final status.
func (r *Runtime) stopChild(parent protocol.SessionID, run protocol.ConversationRun) {
	if r.childStops[run.Turn] || run.Status != protocol.ConversationStatusQueued && run.Status != protocol.ConversationStatusRunning {
		return
	}
	if r.childStops == nil {
		r.childStops = map[protocol.TurnID]bool{}
	}
	r.childStops[run.Turn] = true
	r.execute(protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: run.Turn}}, func(c *ui.Context, out protocol.Output, err error) {
		delete(r.childStops, run.Turn)
		r.reportKeys(c, err, "subagent_stop_failed", "session_changed", "subagent_stop_unknown")
		// Older hosts can acknowledge a terminal child whose spawn is outside
		// the live page without publishing it to the parent's loaded history.
		if ended, ok := detail[protocol.ConversationRun](out.Data); err == nil && ok && out.Kind == "run" && ended.Turn == run.Turn && ended.Session == run.Session && !runStatus(ended.Status).Active() {
			_ = r.controller.RefreshConversation(r.ctx, parent)
		}
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
	view := transcriptChild(page, child, locale)
	view.Model, view.Effort = session.Config.Model, effortName(session.Config.Effort)
	return view
}

func transcriptChild(page *protocol.ConversationPage, child protocol.ConversationChild, locale string) conversation.Child {
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
	view := conversation.Child{ID: child.Run.Session.String(), Turn: child.Run.Turn.String(), Title: childTitle(page, child, locale), Status: status}
	if child.Run.Error != nil {
		view.Error = child.Run.Error.Message
	}
	return view
}

type childModel struct{ model, effort, avatar string }

func enrichChildModels(cv *viewmodel.Conversation, views map[protocol.SessionID]childModel) {
	if len(views) == 0 {
		return
	}
	for ti := range cv.Turns {
		for bi := range cv.Turns[ti].Reply.Blocks {
			for ci := range cv.Turns[ti].Reply.Blocks[bi].Children {
				child := &cv.Turns[ti].Reply.Blocks[bi].Children[ci]
				if id, err := protocol.ParseSessionID(child.ID); err == nil {
					if view, ok := views[id]; ok {
						child.Model, child.Effort, child.Avatar = view.model, view.effort, view.avatar
					}
				}
			}
		}
	}
}
