package live

import (
	"fmt"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// interactionFlow is one conversation's decisions and answers on their way.
// The conversation holds one decision and one answer at a time; a request
// whose outcome is unknown is retained so Retry sends that same request.
type interactionFlow struct {
	approval, question *interactionAttempt
	// settled holds what the Node accepted, hidden until its stream stops
	// listing it as pending.
	settled map[string]bool
	// failedDetails holds the approval details whose read failed.
	failedDetails map[string]bool
	// listed holds the approvals last drawn, a subagent's among them: the
	// receipt of a decision is the card it was made on.
	listed []composer.Approval
}

type interactionAttempt struct {
	id       string
	request  protocol.Request
	busy     bool
	retained bool
	question bool
	receipt  composer.Approval
	done     func(protocol.Output)
}

func (r *Runtime) interactionFlow(id string) *interactionFlow {
	if r.interactionFlows == nil {
		r.interactionFlows = map[string]*interactionFlow{}
	}
	f := r.interactionFlows[id]
	if f == nil {
		f = &interactionFlow{settled: map[string]bool{}, failedDetails: map[string]bool{}}
		r.interactionFlows[id] = f
	}
	return f
}

// sendInteraction executes the attempt's request. Success settles it; a
// closed, changed or rejected request is dropped with its toast; an unknown
// outcome keeps the request for Retry, which resolves it.
func (r *Runtime) sendInteraction(f *interactionFlow, a *interactionAttempt) {
	if a.busy || f.approval != a && f.question != a {
		return
	}
	// Retry sends the kept request again and asks the Node what became of
	// it, so an outcome that can never be known ends the request.
	again := a.retained
	a.busy, a.retained = true, false
	r.async(func() {
		send := r.controller.Execute
		if again {
			send = r.controller.Settle
		}
		output, err := send(r.ctx, a.request)
		r.enqueue(func(c *ui.Context) {
			a.busy = false
			code := faultCode(err)
			a.retained = err != nil && !client.Released(err) && (code == "" || code == protocol.ErrorCodeOutcomeUnknown || code == protocol.ErrorCodeUnavailable || code == protocol.ErrorCodeBusy)
			if !a.retained {
				if f.approval == a {
					f.approval = nil
				}
				if f.question == a {
					f.question = nil
				}
			}
			closed := "approval_closed"
			if a.question {
				closed = "question_closed"
			}
			switch {
			case err == nil:
				f.settled[a.id] = true
				if a.done != nil {
					a.done(output)
				}
			case a.retained:
				toast := ui.Toast{Type: "warning", Title: i18n.T(c, "chat_action_unknown"), Timeout: kit.ToastDuration(c), Action: i18n.T(c, "chat_retry"), OnAction: func() {
					if r.controller.Connected() {
						r.sendInteraction(f, a)
					}
				}}
				if code == protocol.ErrorCodeBusy {
					toast.Type, toast.Title = "error", i18n.T(c, "chat_action_failed")
				}
				c.AddToast(toast)
			case code == protocol.ErrorCodeConflict || code == protocol.ErrorCodeNotFound:
				kit.Toast(c, kit.Conflict, i18n.T(c, closed))
			case a.question && code == protocol.ErrorCodeRevisionConflict:
				kit.Toast(c, kit.Conflict, i18n.T(c, "plan_config_changed"))
			case a.question && code == protocol.ErrorCodeInvalidRequest:
				kit.Toast(c, kit.Failed, i18n.T(c, "question_invalid_answer"))
			case code == protocol.ErrorCodeOutcomeUnknown:
				// Released: the Node can never tell what the request did.
				kit.Toast(c, kit.Uncertain, i18n.T(c, "chat_action_unknown"))
			default:
				kit.Toast(c, kit.Failed, i18n.T(c, "chat_action_failed"))
			}
		})
	})
}

// interactive reports whether the conversation can decide or answer now,
// telling why not when the host is away.
func (x *conversationActions) interactive() bool {
	if !x.owned() {
		return false
	}
	if !x.r.controller.Connected() {
		key := "chat_reconnecting"
		if faultCode(x.r.controller.Err()) == protocol.ErrorCodeBusy {
			key = "live_read_busy"
		}
		kit.Toast(x.c, kit.Failed, i18n.T(x.c, key))
		return false
	}
	session, ok := x.r.session(x.cv.ID)
	return ok && !delegated(session)
}

// waitingApproval finds a pending approval of the conversation or of one of
// its subagents, with the page that holds its call.
func (r *Runtime) waitingApproval(conversation, id string) (protocol.ConversationApproval, protocol.ConversationPage, bool) {
	parent, ok := r.session(conversation)
	if !ok {
		return protocol.ConversationApproval{}, protocol.ConversationPage{}, false
	}
	state, ok := r.controller.Conversation(parent.ID)
	if !ok {
		return protocol.ConversationApproval{}, protocol.ConversationPage{}, false
	}
	for _, approval := range state.Snapshot.Page.Approvals {
		if approval.ID.String() == id && approval.State == "pending" {
			return approval, state.Snapshot.Page, true
		}
	}
	if delegated(parent) {
		return protocol.ConversationApproval{}, protocol.ConversationPage{}, false
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
			if approval.ID.String() == id && childApproval(approval, session.ID) {
				return approval, details.Snapshot.Page, true
			}
		}
	}
	return protocol.ConversationApproval{}, protocol.ConversationPage{}, false
}

// waitingQuestion finds a pending question of the conversation and what it
// asks.
func (r *Runtime) waitingQuestion(conversation, id string) (protocol.ConversationQuestion, protocol.ConversationQuestionSpec, bool) {
	session, ok := r.session(conversation)
	if !ok {
		return protocol.ConversationQuestion{}, protocol.ConversationQuestionSpec{}, false
	}
	state, ok := r.controller.Conversation(session.ID)
	if !ok {
		return protocol.ConversationQuestion{}, protocol.ConversationQuestionSpec{}, false
	}
	for _, q := range state.Snapshot.Page.Questions {
		if q.ID.String() != id || q.State.Kind != "pending" {
			continue
		}
		call := callAt(state.Snapshot.Page, q.Entry, q.Index)
		if call == nil || call.Summary == nil || call.Summary.Question == nil || *call.Summary.Question == nil {
			break
		}
		return q, **call.Summary.Question, true
	}
	return protocol.ConversationQuestion{}, protocol.ConversationQuestionSpec{}, false
}

// projectInteractionStates gives the waiting approvals and question their
// states: details loading or failed, disabled, a decision or answer on its
// way, and the receipt of a decision whose outcome is unknown. What the Node
// already accepted is hidden.
func (r *Runtime) projectInteractionStates(cv *viewmodel.Conversation, ctx *composer.Context, locale string) {
	f := r.interactionFlows[cv.ID]
	if f == nil {
		if len(ctx.Interactions.Approvals) == 0 && ctx.Interactions.Question == nil {
			return
		}
		f = r.interactionFlow(cv.ID)
	}
	connected := r.controller.Connected()
	again := func(a *interactionAttempt) func() {
		if !a.retained || a.busy {
			return nil
		}
		return func() {
			if r.controller.Connected() {
				r.sendInteraction(f, a)
			}
		}
	}
	listed := map[string]bool{}
	f.listed = ctx.Interactions.Approvals
	approvals := make([]composer.Approval, 0, len(ctx.Interactions.Approvals))
	for _, view := range ctx.Interactions.Approvals {
		listed[view.ID] = true
		if f.settled[view.ID] {
			continue
		}
		if approval, page, ok := r.waitingApproval(cv.ID, view.ID); ok {
			view = r.approvalDetails(f, view, approval, page, locale)
			view.Disabled = !connected || f.approval != nil || !slices.ContainsFunc(page.Runs, func(run protocol.ConversationRun) bool {
				return run.Turn == approval.Turn && run.Status == "running"
			})
		}
		if a := f.approval; a != nil && a.id == view.ID {
			view.Busy, view.Again = a.busy, again(a)
		}
		approvals = append(approvals, view)
	}
	if a := f.approval; a != nil && !listed[a.id] && a.retained {
		receipt := a.receipt
		receipt.Receipt, receipt.Busy, receipt.Again = true, a.busy, again(a)
		approvals = append(approvals, receipt)
	}
	ctx.Interactions.Approvals = approvals
	if q := ctx.Interactions.Question; q != nil {
		listed[q.ID] = true
		if f.settled[q.ID] {
			ctx.Interactions.Question = nil
		} else {
			ask := *q
			ask.Disabled = !connected
			if a := f.question; a != nil && a.id == q.ID {
				ask.Busy, ask.Retry = a.busy, a.retained && !a.busy
			}
			ctx.Interactions.Question = &ask
		}
	}
	for id := range f.settled {
		if !listed[id] {
			delete(f.settled, id)
		}
	}
}

// approvalDetails projects the approval from its call's details once they are
// read, reading them when they are not.
func (r *Runtime) approvalDetails(f *interactionFlow, view composer.Approval, approval protocol.ConversationApproval, page protocol.ConversationPage, locale string) composer.Approval {
	call := callAt(page, approval.Entry, approval.Index)
	if call == nil || call.Summary == nil || !call.Summary.HasDetails {
		return view
	}
	ref := protocol.ToolDetailReference{Session: page.Session, Turn: approval.Turn, Entry: approval.Entry, Index: approval.Index, Call: call.ID, Revision: call.Summary.Revision}
	if loaded, ok := r.controller.ConversationDetail(ref); ok {
		summary := *call.Summary
		summary.Resolved = loaded.Resolved
		detailed := *call
		detailed.Summary = &summary
		fresh := projectApproval(approval, &detailed, locale)
		fresh.Child = view.Child
		return fresh
	}
	key := fmt.Sprintf("detail/%s/%s/%d/%s", ref.Session, ref.Entry, ref.Index, ref.Revision)
	if f.failedDetails[key] {
		view.Failed = true
		view.Reload = func() { delete(f.failedDetails, key) }
		return view
	}
	view.Loading = true
	if r.busy[key] || !r.controller.Connected() {
		return view
	}
	r.busy[key] = true
	r.async(func() {
		_, err := r.controller.ReadConversationDetail(r.ctx, ref)
		r.enqueue(func(c *ui.Context) {
			delete(r.busy, key)
			if err != nil {
				f.failedDetails[key] = true
				r.report(c, err, "session_changed")
			}
		})
	})
	return view
}
