package live

import (
	"encoding/json"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
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
	x.r.execute(protocol.Command{Kind: "read_queued_turn", Data: protocol.CommandReadQueuedTurnPayload{Turn: item.Turn}}, func(c *ui.Context, out protocol.Output, err error) {
		if err != nil {
			x.r.report(c, err, "session_changed")
			return
		}
		value, ok := detail[protocol.ConversationQueuedMessage](out.Data)
		if !ok {
			return
		}
		value.Message.Text = text
		x.r.command(protocol.Command{Kind: "edit_queued_turn", Data: protocol.CommandEditQueuedTurnPayload{Turn: item.Turn, ExpectedRevision: item.Revision, Message: value.Message}}, "session_changed", nil)
	})
}
func (x *conversationActions) Compact() {
	if x.readonly() {
		return
	}
	if s, ok := x.r.session(x.cv.ID); ok {
		x.r.command(protocol.Command{Kind: "compact_context", Data: protocol.CommandCompactContextPayload{Session: s.ID, ExpectedRevision: s.Revision}}, "session_changed", nil)
	}
}
func (x *conversationActions) Decide(id string, decision composer.Decision, reason string) {
	if x.readonly() {
		return
	}
	if x.r.resolveChildApproval(x, id, decision, reason) {
		return
	}
	s, ok := x.r.session(x.cv.ID)
	if !ok {
		return
	}
	approval, err := protocol.ParseApprovalID(id)
	if err != nil {
		return
	}
	d := protocol.ConversationDecision{Kind: []string{"approve", "approve_session", "approve_prefix", "deny"}[min(max(int(decision), 0), 3)]}
	if decision == composer.AllowPrefix {
		for _, a := range x.cv.Waiting.Approvals {
			if a.ID == id {
				d.Data = a.Prefix
			}
		}
	}
	if decision == composer.Deny && reason != "" {
		d.Kind, d.Data = "deny_with_reason", reason
	}
	x.r.command(protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: s.ID, Approval: approval, Decision: d}}, "approval_closed", nil)
}
func (x *conversationActions) question(id string, response protocol.ConversationQuestionResponse) {
	if x.readonly() {
		return
	}
	s, ok := x.r.session(x.cv.ID)
	if !ok {
		return
	}
	question, err := protocol.ParseQuestionID(id)
	if err != nil {
		return
	}
	x.r.command(protocol.Command{Kind: "resolve_question", Data: protocol.CommandResolveQuestionPayload{Session: s.ID, Question: question, Response: response}}, "question_closed", nil)
}
func (x *conversationActions) Answer(id string, a composer.Answer) {
	answer := protocol.ConversationQuestionAnswer{Kind: "text", Data: a.Text}
	if q := x.cv.Waiting.Question; q != nil && len(q.Options) > 0 {
		choices := protocol.ConversationQuestionAnswerChoicesPayload{}
		for _, i := range a.Choices {
			choices.Selected = append(choices.Selected, uint64(i))
		}
		if a.Text != "" {
			text := a.Text
			choices.Other = &text
		}
		answer = protocol.ConversationQuestionAnswer{Kind: "choices", Data: choices}
	}
	x.question(id, protocol.ConversationQuestionResponse{Kind: "answer", Data: answer})
}
func (x *conversationActions) CancelQuestion(id string) {
	x.question(id, protocol.ConversationQuestionResponse{Kind: "cancel"})
}
func (x *conversationActions) DeclineQuestion(id string) {
	x.question(id, protocol.ConversationQuestionResponse{Kind: "decline"})
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
func (x *conversationActions) Retry(turn string) {
	s, state, ok := x.state()
	if !ok {
		return
	}
	id, err := protocol.ParseTurnID(turn)
	if err != nil {
		return
	}
	if len(state.Page.Queue.Items) > 0 || slices.ContainsFunc(state.Page.Runs, func(r protocol.ConversationRun) bool {
		return r.Status == "running" || r.Status == "queued" || r.Status == "stopping"
	}) {
		return
	}
	if !slices.ContainsFunc(state.Page.Runs, func(r protocol.ConversationRun) bool {
		return r.Turn == id && (r.Status == "failed" || r.Status == "cancelled" || r.Status == "interrupted")
	}) {
		return
	}
	x.r.command(protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: s.ID, ExpectedRevision: s.Revision, Message: sentInput(state.Page, id)}}, "session_changed", nil)
}

type conversationEdit struct {
	Turn, Head      protocol.TurnID
	History, Config uint64
}

func (x *conversationActions) Edit(turn string) {
	s, state, ok := x.state()
	if !ok || len(state.Page.Runs) == 0 {
		return
	}
	id, err := protocol.ParseTurnID(turn)
	if err != nil {
		return
	}
	input := sentInput(state.Page, id)
	x.comp.Draft = input.Text
	x.comp.References = nil
	for _, ref := range input.References {
		if value, ok := composerReference(ref); ok {
			x.comp.References = append(x.comp.References, value)
		}
	}
	if x.r.conversationEdits == nil {
		x.r.conversationEdits = map[string]conversationEdit{}
	}
	x.r.conversationEdits[x.cv.ID] = conversationEdit{Turn: id, Head: state.Page.Runs[len(state.Page.Runs)-1].Turn, History: state.Page.Revision, Config: s.Revision}
}

func (x *conversationActions) replace(m composer.Message, finish func(protocol.Output, bool)) bool {
	edit, ok := x.r.conversationEdits[x.cv.ID]
	if !ok {
		return false
	}
	s, exists := x.r.session(x.cv.ID)
	if !exists {
		return false
	}
	x.r.command(protocol.Command{Kind: "replace_turn", Data: protocol.CommandReplaceTurnPayload{Session: s.ID, Turn: edit.Turn, ExpectedHead: edit.Head, ExpectedHistoryRevision: edit.History, ExpectedRevision: edit.Config, Message: conversationInput(m)}}, "session_changed", func(out protocol.Output, ok bool) {
		if ok {
			delete(x.r.conversationEdits, x.cv.ID)
			_ = x.r.controller.RefreshConversation(x.r.ctx, s.ID)
		}
		finish(out, ok)
	})
	return true
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
