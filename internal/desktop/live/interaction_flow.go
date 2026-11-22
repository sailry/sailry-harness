package live

import (
	"fmt"
	"maps"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// interactionFlow keeps a decision and the attempts for individual questions.
// A question leaving the page does not settle its request or hold up the next
// question.
type interactionFlow struct {
	approval  *interactionAttempt
	questions map[string]*interactionAttempt
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
	prompt   string
	request  protocol.Request
	busy     bool
	retained bool
	question bool
	receipt  composer.Approval
	done     func(protocol.Output)
}

const (
	approvalDetailCacheLimit = 16
	approvalDetailTextLimit  = 64 << 10
)

type approvalDetailProjection struct {
	session    protocol.SessionID
	generation uint64
	locale     string
	view       composer.Approval
}

func cacheableApprovalDetail(view composer.Approval) bool {
	size := len(view.ID) + len(view.Title) + len(view.Context) + len(view.Description) + len(view.Prefix)
	if trust := view.Trust; trust != nil {
		size += len(trust.Host) + len(trust.Algorithm) + len(trust.Fingerprint)
	}
	size += cap(view.Notices)*16 + cap(view.Changes)*64
	for _, notice := range view.Notices {
		size += len(notice)
	}
	for _, change := range view.Changes {
		size += len(change.Path) + len(change.Destination)
	}
	return size <= approvalDetailTextLimit
}

func (r *Runtime) interactionFlow(id string) *interactionFlow {
	if r.interactionFlows == nil {
		r.interactionFlows = map[string]*interactionFlow{}
	}
	f := r.interactionFlows[id]
	if f == nil {
		f = &interactionFlow{questions: map[string]*interactionAttempt{}, settled: map[string]bool{}, failedDetails: map[string]bool{}}
		r.interactionFlows[id] = f
	}
	return f
}

// sendInteraction executes a new attempt. A retained question is only looked
// up: disappearance of its card is not evidence that the answer was applied.
func (r *Runtime) sendInteraction(f *interactionFlow, a *interactionAttempt) {
	if a.busy || f.approval != a && f.questions[a.id] != a {
		return
	}
	again := a.retained
	a.busy, a.retained = true, false
	r.async(func() {
		var output protocol.Output
		var err error
		var retained bool
		if again && a.question {
			var resolution client.Resolution
			resolution, err = r.controller.Resolve(r.ctx, a.request)
			retained = err != nil || resolution.State == client.RequestPending
			if err == nil {
				output, err = resolution.Result()
			}
		} else {
			send := r.controller.Execute
			if again {
				send = r.controller.Settle
			}
			output, err = send(r.ctx, a.request)
			code := faultCode(err)
			retained = err != nil && !client.Released(err) && (code == "" || code == protocol.ErrorCodeOutcomeUnknown || code == protocol.ErrorCodeUnavailable || code == protocol.ErrorCodeBusy)
		}
		r.enqueue(func(c *ui.Context) {
			a.busy, a.retained = false, retained
			code := faultCode(err)
			if !a.retained {
				if f.approval == a {
					f.approval = nil
				}
				if f.questions[a.id] == a {
					delete(f.questions, a.id)
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
				if again && a.question {
					kit.Toast(c, kit.Done, i18n.T(c, "activity_completed"))
				}
			case a.retained:
				if code == protocol.ErrorCodeBusy {
					kit.Toast(c, kit.Failed, i18n.T(c, "chat_action_failed"))
				} else {
					kit.Toast(c, kit.Uncertain, i18n.T(c, "chat_action_unknown"))
				}
			case code == protocol.ErrorCodeConflict || code == protocol.ErrorCodeNotFound:
				kit.Toast(c, kit.Conflict, i18n.T(c, closed))
			case a.question && code == protocol.ErrorCodeRevisionConflict:
				kit.Toast(c, kit.Conflict, i18n.T(c, "plan_config_changed"))
			case a.question && code == protocol.ErrorCodeInvalidRequest:
				kit.Toast(c, kit.Failed, i18n.T(c, "question_invalid_answer"))
			case code == protocol.ErrorCodeOutcomeUnknown:
				// Released: the Node can never tell what the request did.
				key := "chat_action_unknown"
				if a.question {
					key = "tool_interrupted"
				}
				kit.Toast(c, kit.Uncertain, i18n.T(c, key))
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
func (r *Runtime) waitingApproval(conversation, id string, read func(protocol.SessionID) (client.ConversationState, bool)) (protocol.ConversationApproval, protocol.ConversationPage, bool) {
	parent, ok := r.session(conversation)
	if !ok {
		return protocol.ConversationApproval{}, protocol.ConversationPage{}, false
	}
	state, ok := read(parent.ID)
	if !ok || state.Removed {
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
	for target := range r.liveChildren(state) {
		if !target.Listed {
			continue
		}
		details, ready := read(target.Session.ID)
		if !ready || details.Removed {
			continue
		}
		for _, approval := range details.Snapshot.Page.Approvals {
			if approval.ID.String() == id && childApproval(approval, target.Session.ID) {
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
	var read *overviewRead
	if r.conversationCopies != nil {
		if session, err := protocol.ParseSessionID(cv.ID); err == nil {
			read = r.overviews[session]
			if read == nil && len(ctx.Interactions.Approvals) != 0 {
				read = r.overviewReadFor(session)
			}
		}
	}
	f.listed = ctx.Interactions.Approvals
	approvals := make([]composer.Approval, 0, len(ctx.Interactions.Approvals))
	for _, view := range ctx.Interactions.Approvals {
		listed[view.ID] = true
		if f.settled[view.ID] {
			continue
		}
		if approval, page, ok := r.waitingApproval(cv.ID, view.ID, r.conversationState); ok {
			view = r.approvalDetails(read, f, view, approval, page, locale)
			view.Disabled = !connected || f.approval != nil || !slices.ContainsFunc(page.Runs, func(run protocol.ConversationRun) bool {
				return run.Turn == approval.Turn && run.Status == "running"
			})
		} else if read != nil {
			delete(read.approvals, view.ID)
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
	if read != nil {
		for id := range read.approvals {
			if !listed[id] || f.settled[id] {
				delete(read.approvals, id)
			}
		}
	}
	q := ctx.Interactions.Question
	if q != nil {
		listed[q.ID] = true
		if f.settled[q.ID] {
			ctx.Interactions.Question = nil
		} else {
			ask := *q
			ask.Disabled = !connected
			if a := f.questions[q.ID]; a != nil {
				ask.Busy, ask.Check = a.busy, again(a)
			}
			ctx.Interactions.Question = &ask
		}
	}
	ctx.Interactions.QuestionReceipts = nil
	for _, id := range slices.Sorted(maps.Keys(f.questions)) {
		if q != nil && q.ID == id {
			continue
		}
		a := f.questions[id]
		receipt := composer.QuestionReceipt{ID: id, Prompt: a.prompt, Busy: a.busy}
		if connected {
			receipt.Check = again(a)
		}
		ctx.Interactions.QuestionReceipts = append(ctx.Interactions.QuestionReceipts, receipt)
	}
	for id := range f.settled {
		if !listed[id] {
			delete(f.settled, id)
		}
	}
}

// approvalDetails projects the approval from its call's details once they are
// read, reading them when they are not.
func (r *Runtime) approvalDetails(read *overviewRead, f *interactionFlow, view composer.Approval, approval protocol.ConversationApproval, page protocol.ConversationPage, locale string) composer.Approval {
	var generation uint64
	if read != nil {
		generation = r.conversationCopies[page.Session].generation
		if cached, ok := read.approvals[view.ID]; ok {
			if generation != 0 && cached.generation == generation && cached.session == page.Session && cached.locale == locale {
				fresh := cached.view
				fresh.Child = view.Child
				return fresh
			}
			delete(read.approvals, view.ID)
		}
	}
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
		if generation != 0 && len(read.approvals) < approvalDetailCacheLimit && cacheableApprovalDetail(fresh) {
			if read.approvals == nil {
				read.approvals = make(map[string]approvalDetailProjection)
			}
			// Preserve only what the card draws, without keeping the tool payload
			// or its history alive after the approval or session disappears.
			read.approvals[view.ID] = approvalDetailProjection{session: page.Session, generation: generation, locale: locale, view: fresh}
		}
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
