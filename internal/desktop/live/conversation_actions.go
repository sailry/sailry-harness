package live

import (
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (x *conversationActions) state() (protocol.Session, protocol.ConversationSnapshot, bool) {
	if !x.owned() {
		return protocol.Session{}, protocol.ConversationSnapshot{}, false
	}
	s, ok := x.r.session(x.cv.ID)
	if !ok {
		return s, protocol.ConversationSnapshot{}, false
	}
	state, ok := x.r.controller.Conversation(s.ID)
	return s, state.Snapshot, ok
}
func (x *conversationActions) queued(id string) (protocol.ConversationPending, bool) {
	_, state, ok := x.state()
	if !ok {
		return protocol.ConversationPending{}, false
	}
	for _, p := range state.Page.Queue.Items {
		if p.Turn.String() == id {
			return p, true
		}
	}
	return protocol.ConversationPending{}, false
}
func (x *conversationActions) SendQueued(id string) {
	if x.readonly() {
		return
	}
	item, ok := x.queued(id)
	if !ok {
		return
	}
	if run, active := x.activeRun(); active {
		x.r.command(protocol.Command{Kind: "guide_queued_turn", Data: protocol.CommandGuideQueuedTurnPayload{Turn: item.Turn, Recipient: run.Turn, ExpectedRevision: item.Revision}}, "session_changed", nil)
	} else {
		x.r.command(protocol.Command{Kind: "send_queued_turn", Data: protocol.CommandSendQueuedTurnPayload{Turn: item.Turn, ExpectedRevision: item.Revision}}, "session_changed", nil)
	}
}
func (x *conversationActions) WithdrawGuidance(id string) {
	if x.readonly() {
		return
	}
	if p, ok := x.queued(id); ok {
		x.r.command(protocol.Command{Kind: "withdraw_guidance", Data: protocol.CommandWithdrawGuidancePayload{Turn: p.Turn, ExpectedRevision: p.Revision}}, "session_changed", nil)
	}
}
func (x *conversationActions) RemoveQueued(id string) {
	if x.readonly() {
		return
	}
	if p, ok := x.queued(id); ok {
		x.r.command(protocol.Command{Kind: "remove_queued_turn", Data: protocol.CommandRemoveQueuedTurnPayload{Turn: p.Turn, ExpectedRevision: p.Revision}}, "session_changed", nil)
	}
}
func (x *conversationActions) MoveQueued(id, before string) {
	if x.readonly() {
		return
	}
	s, state, ok := x.state()
	if !ok {
		return
	}
	item, ok := x.queued(id)
	if !ok {
		return
	}
	var target *protocol.TurnID
	if before != "" {
		if p, ok := x.queued(before); ok {
			target = &p.Turn
		} else {
			return
		}
	}
	x.r.command(protocol.Command{Kind: "move_queued_turn", Data: protocol.CommandMoveQueuedTurnPayload{Session: s.ID, ExpectedRevision: state.Page.Queue.Revision, Turn: item.Turn, Before: target}}, "session_changed", nil)
}
func (x *conversationActions) SaveQueued(id, text string) {
	if x.readonly() {
		return
	}
	item, ok := x.queued(id)
	if !ok {
		return
	}
	x.r.saveQueued(item, text, nil)
}
func (x *conversationActions) Compact() {
	if x.readonly() {
		return
	}
	if s, ok := x.r.session(x.cv.ID); ok {
		x.turn(protocol.Command{Kind: "compact_context", Data: protocol.CommandCompactContextPayload{Session: s.ID, ExpectedRevision: s.Revision}}, nil)
	}
}

// Decide sends the decision on a waiting approval of the conversation or of
// one of its subagents. One decision travels at a time; the request whose
// outcome is unknown stays for Retry.
func (x *conversationActions) Decide(id string, decision composer.Decision, reason string) {
	if !x.interactive() {
		return
	}
	f := x.r.interactionFlow(x.cv.ID)
	if f.approval != nil {
		kit.Toast(x.c, kit.Failed, i18n.T(x.c, "approval_busy"))
		return
	}
	approval, _, ok := x.r.waitingApproval(x.cv.ID, id)
	if !ok || f.settled[id] {
		kit.Toast(x.c, kit.Conflict, i18n.T(x.c, "approval_closed"))
		return
	}
	d := protocol.ConversationDecision{Kind: []string{"approve", "approve_session", "approve_prefix", "deny"}[min(max(int(decision), 0), 3)]}
	switch {
	case decision == composer.AllowSession && !approval.CanRemember:
		return
	case decision == composer.AllowPrefix:
		if !approval.CanRemember || approval.Prefix == nil {
			return
		}
		d.Data = *approval.Prefix
	case decision == composer.Deny && strings.TrimSpace(reason) != "":
		d.Kind, d.Data = "deny_with_reason", reason
	}
	attempt := &interactionAttempt{id: id, request: x.r.controller.Prepare(protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: approval.Session, Approval: approval.ID, Decision: d}})}
	for _, view := range x.cv.Waiting.Approvals {
		if view.ID == id {
			attempt.receipt = view
		}
	}
	f.approval = attempt
	x.r.sendInteraction(f, attempt)
}

// question sends the response to a waiting question. An answer whose outcome
// is unknown is sent again as the same request; Cancel and Decline replace
// it.
func (x *conversationActions) question(id string, response protocol.ConversationQuestionResponse, done func(protocol.Output)) {
	if !x.interactive() {
		return
	}
	f := x.r.interactionFlow(x.cv.ID)
	if a := f.question; a != nil {
		if a.busy {
			return
		}
		if a.id == id && (response.Kind == "answer" || response.Kind == "start_coding") {
			x.r.sendInteraction(f, a)
			return
		}
	}
	q, _, ok := x.r.waitingQuestion(x.cv.ID, id)
	if !ok || f.settled[id] {
		kit.Toast(x.c, kit.Conflict, i18n.T(x.c, "question_closed"))
		return
	}
	f.question = &interactionAttempt{id: id, question: true, done: done, request: x.r.controller.Prepare(protocol.Command{Kind: "resolve_question", Data: protocol.CommandResolveQuestionPayload{Session: q.Session, Question: q.ID, Response: response}})}
	x.r.sendInteraction(f, f.question)
}

// Answer answers the waiting question in the shape its kind takes: a plan
// review starts coding or continues planning, a form sends its fields'
// values, a link opens its page and says so, a choice its options and a free
// answer its text.
func (x *conversationActions) Answer(id string, a composer.Answer) {
	_, spec, ok := x.r.waitingQuestion(x.cv.ID, id)
	if !ok {
		x.question(id, protocol.ConversationQuestionResponse{Kind: "answer"}, nil)
		return
	}
	answer := protocol.ConversationQuestionAnswer{Kind: "text", Data: a.Text}
	switch spec.Input.Kind {
	case "plan":
		if a.Start {
			s, ok := x.r.session(x.cv.ID)
			if !ok {
				return
			}
			message := protocol.DefaultConversationInput()
			message.Text = i18n.T(x.c, "plan_coding_message")
			conversation := x.cv.ID
			x.question(id, protocol.ConversationQuestionResponse{Kind: "start_coding", Data: protocol.ConversationQuestionResponseStartCodingPayload{ExpectedRevision: s.Revision, Message: message}}, func(out protocol.Output) {
				if accepted, ok := detail[protocol.ConversationQuestionAcceptedPlan](out.Data); ok {
					if current, known := x.r.session(conversation); !known || accepted.Session.Revision > current.Revision {
						if x.r.admittedSessions == nil {
							x.r.admittedSessions = map[string]protocol.Session{}
						}
						x.r.admittedSessions[conversation] = accepted.Session
					}
				}
			})
			return
		}
		answer.Data = ""
	case "form":
		form, _ := detail[protocol.ConversationQuestionInputFormPayload](spec.Input.Data)
		answer = protocol.ConversationQuestionAnswer{Kind: "form", Data: formAnswer(form, a.Values)}
	case "url":
		if f := x.r.interactionFlow(x.cv.ID); f.question == nil && x.r.controller.Connected() {
			if link, ok := detail[protocol.ConversationQuestionInputUrlPayload](spec.Input.Data); ok {
				x.c.OpenURL(link.Url)
			}
		}
		answer = protocol.ConversationQuestionAnswer{Kind: "opened"}
	case "choice":
		choices := protocol.ConversationQuestionAnswerChoicesPayload{Selected: []uint64{}}
		for _, i := range a.Choices {
			choices.Selected = append(choices.Selected, uint64(i))
		}
		if a.Text != "" {
			text := a.Text
			choices.Other = &text
		}
		answer = protocol.ConversationQuestionAnswer{Kind: "choices", Data: choices}
	}
	x.question(id, protocol.ConversationQuestionResponse{Kind: "answer", Data: answer}, nil)
}

// formAnswer encodes the form's values by field name: an option answers its
// value, several their values.
func formAnswer(form protocol.ConversationQuestionInputFormPayload, values map[string]any) map[string]json.RawMessage {
	out := map[string]json.RawMessage{}
	for _, field := range form.Fields {
		value, ok := values[field.Name]
		if !ok {
			continue
		}
		if choice, ok := detail[protocol.ConversationQuestionFormInputChoicePayload](field.Input.Data); ok && field.Input.Kind == "choice" {
			option := func(i int) string {
				if i < 0 || i >= len(choice.Options) {
					return ""
				}
				return choice.Options[i].Value
			}
			switch chosen := value.(type) {
			case int:
				value = option(chosen)
			case []int:
				names := make([]string, 0, len(chosen))
				for _, i := range chosen {
					names = append(names, option(i))
				}
				value = names
			}
		}
		if encoded, err := json.Marshal(value); err == nil {
			out[field.Name] = encoded
		}
	}
	return out
}
func (x *conversationActions) CancelQuestion(id string) {
	x.question(id, protocol.ConversationQuestionResponse{Kind: "cancel"}, nil)
}
func (x *conversationActions) DeclineQuestion(id string) {
	x.question(id, protocol.ConversationQuestionResponse{Kind: "decline"}, nil)
}
func (x *conversationActions) Fork(turn string) {
	if !x.owned() {
		return
	}
	s, ok := x.r.session(x.cv.ID)
	if !ok {
		return
	}
	through, err := protocol.ParseTurnID(turn)
	if err != nil {
		return
	}
	x.r.command(protocol.Command{Kind: "fork_conversation", Data: protocol.CommandForkConversationPayload{Session: s.ID, Through: through, ExpectedRevision: s.Revision}}, "session_changed", func(out protocol.Output, ok bool) {
		if ok {
			if v, valid := detail[protocol.Session](out.Data); valid {
				x.r.project()
				x.r.OpenSession(v.ID.String())
			}
		}
	})
}
func (x *conversationActions) Rewind(turn string) {
	s, state, ok := x.state()
	if !ok || len(state.Page.Runs) == 0 {
		return
	}
	through, err := protocol.ParseTurnID(turn)
	if err != nil {
		return
	}
	head := state.Page.Runs[len(state.Page.Runs)-1].Turn
	x.r.command(protocol.Command{Kind: "rewind_conversation", Data: protocol.CommandRewindConversationPayload{Session: s.ID, Through: &through, ExpectedHead: head, ExpectedRevision: state.Page.Revision}}, "session_changed", func(_ protocol.Output, ok bool) {
		if ok {
			_ = x.r.controller.RefreshConversation(x.r.ctx, s.ID)
		}
	})
}

func sentInput(page protocol.ConversationPage, turn protocol.TurnID) protocol.ConversationInput {
	input := protocol.DefaultConversationInput()
	found := false
	for _, entry := range page.Entries {
		if entry.Turn != turn || entry.Author != "user" {
			continue
		}
		found = true
		for _, part := range entry.Parts {
			switch part.Kind {
			case "text":
				if v, ok := detail[string](part.Data); ok {
					input.Text += v
				}
			case "reference":
				if v, ok := detail[protocol.ConversationReference](part.Data); ok {
					input.References = append(input.References, v)
				}
			case "attachment":
				if v, ok := detail[protocol.Attachment](part.Data); ok {
					input.Attachments = append(input.Attachments, v.ID)
				}
			}
		}
	}
	if !found {
		for _, run := range page.Runs {
			if run.Turn == turn && run.Input != nil {
				return run.Input.Message
			}
		}
	}
	return input
}

// pageBusy reports a queued or unfinished turn; Retry and Edit wait for it.
func pageBusy(page protocol.ConversationPage) bool {
	return len(page.Queue.Items) > 0 || slices.ContainsFunc(page.Runs, func(r protocol.ConversationRun) bool {
		return r.Status == "running" || r.Status == "queued" || r.Status == "stopping"
	})
}

func (x *conversationActions) Retry(turn string) {
	s, state, ok := x.state()
	if !ok {
		return
	}
	id, err := protocol.ParseTurnID(turn)
	if err != nil || pageBusy(state.Page) {
		return
	}
	if !slices.ContainsFunc(state.Page.Runs, func(r protocol.ConversationRun) bool {
		return r.Turn == id && (r.Status == "failed" || r.Status == "cancelled" || r.Status == "interrupted")
	}) {
		return
	}
	x.turn(protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: s.ID, ExpectedRevision: s.Revision, Message: sentInput(state.Page, id)}}, nil)
}

// conversationEdit marks a composer that holds a sent message for ReplaceTurn.
// The accepted UI has no inline editor or cancel control, so the marker ends
// when that composer empties, its conversation leaves the screen, the message
// is guided or queued instead, or the Node settles the replacement.
type conversationEdit struct {
	Turn, Head protocol.TurnID
	History    uint64
	Input      protocol.ConversationInput
	comp       *composer.State
}

func (x *conversationActions) Edit(turn string) {
	_, state, ok := x.state()
	id, err := protocol.ParseTurnID(turn)
	if !ok || err != nil || len(state.Page.Runs) == 0 {
		return
	}
	input := sentInput(state.Page, id)
	var references []composer.Reference
	for _, ref := range input.References {
		if value, ok := composerReference(ref); ok {
			references = append(references, value)
		}
	}
	// A draft already in the composer is kept, and a message the composer
	// cannot show has nothing to edit there.
	if x.readonly() || pageBusy(state.Page) || x.comp.Draft != "" || len(x.comp.References) > 0 || len(x.comp.Attachments) > 0 || input.Text == "" && len(references) == 0 {
		x.unavailable()
		return
	}
	x.comp.Draft, x.comp.References = input.Text, references
	if x.r.conversationEdits == nil {
		x.r.conversationEdits = map[string]conversationEdit{}
	}
	x.r.conversationEdits[x.cv.ID] = conversationEdit{Turn: id, Head: state.Page.Runs[len(state.Page.Runs)-1].Turn, History: state.Page.Revision, Input: input, comp: x.comp}
}

// editedInput keeps what the composer cannot show: the sent attachments and
// the references it has no chip for.
func editedInput(sent, input protocol.ConversationInput) protocol.ConversationInput {
	for _, ref := range sent.References {
		if _, shown := composerReference(ref); !shown {
			input.References = append(input.References, ref)
		}
	}
	input.Attachments = append(slices.Clone(sent.Attachments), input.Attachments...)
	return input
}

// turnRetry is the one request a conversation retains after an unknown
// outcome. Sending again reuses it only while the composer and the observed
// runs and queue are what they were, so a later message with the same text is
// a new request.
type turnRetry struct {
	key, observed, draft string
	request              protocol.RequestID
	comp                 *composer.State
	done                 func(protocol.Output, bool)
}

func turnObserved(page protocol.ConversationPage) string {
	last := protocol.ConversationRun{}
	if len(page.Runs) > 0 {
		last = page.Runs[len(page.Runs)-1]
	}
	return fmt.Sprint(page.Revision, len(page.Runs), last.Turn, last.Status, page.Queue.Revision, len(page.Queue.Items))
}

// turn sends a command that adds to or replaces the session's turns.
func (x *conversationActions) turn(command protocol.Command, done func(protocol.Output, bool)) {
	id := x.cv.ID
	encoded, _ := json.Marshal(command)
	key := "turn/" + id + "\x00" + string(encoded)
	observed, draft := "", composerFingerprint(x.comp)
	if _, state, ok := x.state(); ok {
		observed = turnObserved(state.Page)
	}
	if retry, ok := x.r.turnRetries[id]; ok && (retry.key != key || retry.observed != "" && retry.observed != observed) {
		x.r.forgetTurnRetry(id)
	}
	x.r.executeScoped("turn/"+id, command, func(c *ui.Context, output protocol.Output, err error) {
		if request, uncertain := x.r.uncertain[key]; uncertain {
			if x.r.turnRetries == nil {
				x.r.turnRetries = map[string]turnRetry{}
			}
			x.r.turnRetries[id] = turnRetry{key: key, observed: observed, draft: draft, request: request.ID, comp: x.comp, done: done}
		} else {
			delete(x.r.turnRetries, id)
		}
		x.r.report(c, err, "session_changed")
		if done != nil {
			done(output, err == nil)
		}
	})
}

func (r *Runtime) forgetTurnRetry(id string) {
	if retry, ok := r.turnRetries[id]; ok {
		delete(r.uncertain, retry.key)
		delete(r.turnRetries, id)
	}
}

// settleTurnRetry ends a retained request that can no longer be a retry: its
// turn was admitted, the history or queue moved, or the draft was changed. An
// admitted turn completes the send that lost its result.
func (r *Runtime) settleTurnRetry(id string, comp *composer.State, turns []protocol.TurnSummary, page protocol.ConversationPage) {
	retry, ok := r.turnRetries[id]
	if !ok || r.busy[retry.key] {
		return
	}
	landed := slices.ContainsFunc(turns, func(turn protocol.TurnSummary) bool { return turn.Request == retry.request }) ||
		slices.ContainsFunc(page.Queue.Items, func(item protocol.ConversationPending) bool {
			return item.Request != nil && *item.Request == retry.request
		})
	if retry.observed == "" {
		// The request was sent before its new session's history was read.
		retry.observed = turnObserved(page)
		r.turnRetries[id] = retry
	}
	if !landed && retry.observed == turnObserved(page) && (retry.comp != comp || retry.draft == composerFingerprint(comp)) {
		return
	}
	r.forgetTurnRetry(id)
	if landed && retry.done != nil {
		retry.done(protocol.Output{}, true)
	}
}

func composerFingerprint(s *composer.State) string {
	type reference struct {
		ID, Label string
		Kind      composer.ReferenceKind
	}
	refs := make([]reference, 0, len(s.References))
	for _, ref := range s.References {
		refs = append(refs, reference{ref.ID, ref.Label, ref.Kind})
	}
	b, _ := json.Marshal(struct {
		Text       string
		References []reference
	}{s.Draft, refs})
	return string(b)
}

func (x *conversationActions) readonly() bool {
	if !x.owned() {
		return true
	}
	session, ok := x.r.session(x.cv.ID)
	return !x.r.controller.Connected() || ok && delegated(session)
}
