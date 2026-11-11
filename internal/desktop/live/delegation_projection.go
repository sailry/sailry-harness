package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
	"time"
)

func (r *Runtime) watchChild(id protocol.SessionID) {
	r.conversationSeen[id] = true
	if _, ok := r.conversationWatches[id]; !ok {
		if release, err := r.controller.WatchConversation(r.ctx, id, r.conversationChanged); err == nil {
			r.conversationWatches[id] = release
		}
	}
}

func (r *Runtime) projectChildComposer(cv *viewmodel.Conversation, comp *composer.State, ctx *composer.Context, locales ...string) {
	ctx.Activity.Children = nil
	if comp.ActivityCallbacks != nil {
		comp.ActivityCallbacks.ChildElapsed = nil
	}
	session, ok := r.session(cv.ID)
	if !ok {
		return
	}
	if delegated(session) {
		ctx.Readonly = true
		ctx.Interactions = composer.Interactions{}
		ctx.Activity.Children = nil
		return
	}
	state, ok := r.conversationState(session.ID)
	if !ok {
		return
	}
	language := "en"
	if r.Preferences != nil && r.Preferences.Data.Language != nil && *r.Preferences.Data.Language == "chinese" {
		language = "zh-CN"
	}
	if len(locales) > 0 {
		language = locales[0]
	}
	runs := map[string]protocol.ConversationRun{}
	for _, child := range client.LatestConversationChildren(state.Snapshot.Page.Children) {
		childSession, valid := client.ConversationChildSession(r.snapshot(), session.ID, child)
		if !valid {
			continue
		}
		runs[childSession.ID.String()] = child.Run
		if child.Run.Status == "queued" || child.Run.Status == "running" || child.Run.Status == "stopping" {
			r.watchChild(childSession.ID)
			if child.Run.Status == protocol.ConversationStatusRunning {
				ctx.Activity.Children = append(ctx.Activity.Children, childView(&state.Snapshot.Page, child, childSession, language))
			}
			if details, ready := r.conversationState(childSession.ID); ready {
				for _, approval := range details.Snapshot.Page.Approvals {
					if approval.State != "pending" || approval.Startup != nil || approval.Ssh != nil || approval.Session != childSession.ID {
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
	enrichChildModels(cv, state.Snapshot.Page, r.snapshot(), language)
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

func (r *Runtime) resolveChildApproval(x *conversationActions, id string, decision composer.Decision, reason string) bool {
	parent, ok := r.session(x.cv.ID)
	if !ok || delegated(parent) {
		return false
	}
	state, ok := r.controller.Conversation(parent.ID)
	if !ok {
		return false
	}
	for _, child := range client.LatestConversationChildren(state.Snapshot.Page.Children) {
		session, valid := client.ConversationChildSession(r.snapshot(), parent.ID, child)
		if !valid {
			continue
		}
		details, ready := r.controller.Conversation(session.ID)
		if !ready {
			continue
		}
		for _, approval := range details.Snapshot.Page.Approvals {
			if approval.ID.String() == id {
				if approval.State != "pending" || approval.Startup != nil || approval.Ssh != nil || approval.Session != session.ID {
					return true
				}
				value := protocol.ConversationDecision{Kind: []string{"approve", "approve_session", "approve_prefix", "deny"}[min(max(int(decision), 0), 3)]}
				if decision == composer.AllowPrefix && approval.Prefix != nil {
					value.Data = *approval.Prefix
				}
				if decision == composer.Deny && reason != "" {
					value.Kind, value.Data = "deny_with_reason", reason
				}
				r.executeScoped("child-approval/"+id, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: session.ID, Approval: approval.ID, Decision: value}}, func(c *ui.Context, _ protocol.Output, err error) { r.report(c, err, "approval_closed") })
				return true
			}
		}
	}
	return false
}
