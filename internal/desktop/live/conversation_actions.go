package live

import (
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
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
	if err != nil || x.controls(through).NoFork {
		return
	}
	id := x.cv.ID
	x.r.conversationPending[id] = through
	x.r.command(protocol.Command{Kind: "fork_conversation", Data: protocol.CommandForkConversationPayload{Session: s.ID, Through: through, ExpectedRevision: s.Revision}}, "session_changed", func(out protocol.Output, ok bool) {
		delete(x.r.conversationPending, id)
		if ok {
			if v, valid := detail[protocol.Session](out.Data); valid {
				x.r.project()
				x.r.OpenSession(v.ID.String())
			}
		}
	})
}

// Rewind keeps the conversation through turn. The Node saves the removed
// history as a backup session, which the outcome's toast opens.
func (x *conversationActions) Rewind(turn string) {
	s, state, ok := x.state()
	if !ok || len(state.Page.Runs) == 0 {
		return
	}
	through, err := protocol.ParseTurnID(turn)
	if err != nil {
		return
	}
	if x.controls(through).NoRewind {
		// The history moved while the confirmation was open.
		kit.Toast(x.c, kit.Conflict, i18n.T(x.c, "chat_history_changed"))
		return
	}
	id := x.cv.ID
	head := state.Page.Runs[len(state.Page.Runs)-1].Turn
	x.r.conversationPending[id] = through
	x.r.execute(protocol.Command{Kind: "rewind_conversation", Data: protocol.CommandRewindConversationPayload{Session: s.ID, Through: &through, ExpectedHead: head, ExpectedRevision: state.Page.Revision}}, func(c *ui.Context, out protocol.Output, err error) {
		delete(x.r.conversationPending, id)
		if err != nil {
			x.r.reportKeys(c, err, "chat_action_failed", "chat_history_changed", "chat_action_unknown")
			return
		}
		_ = x.r.controller.RefreshConversation(x.r.ctx, s.ID)
		toast := ui.Toast{ID: "rewound/" + id, Type: "success", Title: i18n.T(c, "chat_rewound"), Timeout: kit.ToastDuration(c)}
		if rewound, valid := detail[protocol.ConversationRewind](out.Data); valid && rewound.Backup != nil {
			backup := rewound.Backup.ID.String()
			toast.Action = i18n.T(c, "chat_rewind_backup")
			toast.OnAction = func() {
				x.r.project()
				x.r.OpenSession(backup)
			}
		}
		c.AddToast(toast)
	})
}

// controls is which actions of a turn apply now.
func (x *conversationActions) controls(turn protocol.TurnID) conversation.Controls {
	return x.pageControls()(turn)
}

// pageControls reads the conversation once and answers which actions of each
// turn apply: a read-only (delegated or unowned) session hides Edit and the
// fork menu; the rest wait for a connected, idle conversation.
func (x *conversationActions) pageControls() func(protocol.TurnID) conversation.Controls {
	session, state, ok := x.state()
	if !ok || delegated(session) {
		return func(protocol.TurnID) conversation.Controls {
			return conversation.Controls{ReadOnly: true, NoEdit: true, NoFork: true, NoRewind: true, NoWorktree: true}
		}
	}
	page := state.Page
	edit, replacing := x.r.conversationEdits[x.cv.ID]
	pending, operating := x.r.conversationPending[x.cv.ID]
	blocked := !x.r.controller.Connected() || operating || replacing && edit.pending
	busy := pageBusy(page)
	movable := x.r.canMoveWorkspace(x.cv, x.comp)
	ended, sent := map[protocol.TurnID]bool{}, map[protocol.TurnID]bool{}
	for _, run := range page.Runs {
		ended[run.Turn] = run.Status != protocol.ConversationStatusQueued && run.Status != protocol.ConversationStatusRunning && run.Status != protocol.ConversationStatusStopping
		if input := run.Input; input != nil && (input.Message.Text != "" || len(input.Message.References) > 0 || len(input.Message.Attachments) > 0) {
			sent[run.Turn] = true
		}
	}
	for _, entry := range page.Entries {
		if entry.Author == "user" && len(entry.Parts) > 0 {
			sent[entry.Turn] = true
		}
	}
	var head protocol.TurnID
	if len(page.Runs) > 0 {
		head = page.Runs[len(page.Runs)-1].Turn
	}
	return func(turn protocol.TurnID) conversation.Controls {
		_, found := ended[turn]
		return conversation.Controls{
			NoEdit:     blocked || busy || len(page.Runs) == 0 || !sent[turn],
			NoFork:     blocked || !ended[turn],
			NoRewind:   blocked || busy || !found || head == turn,
			NoWorktree: blocked || !ended[turn] || !movable,
			Busy:       operating && pending == turn,
		}
	}
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

// conversationEdit is a sent message being edited in its bubble: the turn,
// the history it was read from and what it sent. Only SaveEdit replaces the
// turn, so an abandoned edit never changes what the composer sends. pending
// is its replacement in flight.
type conversationEdit struct {
	Turn, Head protocol.TurnID
	History    uint64
	Input      protocol.ConversationInput
	pending    bool
}

// Edit opens the editor of a sent message. Its button is disabled where it
// cannot apply, so a refused edit says nothing.
func (x *conversationActions) Edit(turn string) {
	_, state, ok := x.state()
	id, err := protocol.ParseTurnID(turn)
	if !ok || err != nil || x.controls(id).NoEdit {
		return
	}
	if current, editing := x.r.conversationEdits[x.cv.ID]; editing && current.pending {
		return
	}
	x.r.conversationEdits[x.cv.ID] = conversationEdit{Turn: id, Head: state.Page.Runs[len(state.Page.Runs)-1].Turn, History: state.Page.Revision, Input: sentInput(state.Page, id)}
}

// CancelEdit closes the editor; a replacement in flight keeps it.
func (x *conversationActions) CancelEdit(turn string) {
	if edit, ok := x.r.conversationEdits[x.cv.ID]; ok && edit.Turn.String() == turn && !edit.pending {
		delete(x.r.conversationEdits, x.cv.ID)
	}
}

// SaveEdit replaces the edited turn with text against the history the editor
// was opened on. The editor stays, with its text, until the Node accepts the
// replacement: a failure, a conflict or an unknown outcome only toasts.
func (x *conversationActions) SaveEdit(turn, text string) {
	id := x.cv.ID
	edit, ok := x.r.conversationEdits[id]
	if !ok || edit.Turn.String() != turn || edit.pending {
		return
	}
	session, exists := x.r.session(id)
	if !exists || x.readonly() {
		x.unavailable()
		return
	}
	edit.pending = true
	x.r.conversationEdits[id] = edit
	x.r.localRuntime().count(protocol.FeatureUseComposerEdited)
	x.turn(protocol.Command{Kind: "replace_turn", Data: protocol.CommandReplaceTurnPayload{Session: session.ID, Turn: edit.Turn, ExpectedHead: edit.Head, ExpectedHistoryRevision: edit.History, ExpectedRevision: session.Revision, Message: editedInput(edit.Input, text)}}, func(_ protocol.Output, ok bool) {
		current, editing := x.r.conversationEdits[id]
		if !editing || current.Turn != edit.Turn {
			return
		}
		if ok {
			delete(x.r.conversationEdits, id)
			_ = x.r.controller.RefreshConversation(x.r.ctx, session.ID)
			return
		}
		current.pending = false
		x.r.conversationEdits[id] = current
	})
}

// editedInput is the sent message with its text replaced. The attachments
// stay; a reference written in the sent text stays while the new text still
// names it.
func editedInput(sent protocol.ConversationInput, text string) protocol.ConversationInput {
	input := sent
	input.Text = text
	input.References = nil
	for _, ref := range sent.References {
		token := "@" + strings.TrimPrefix(ref.Label, "@")
		if !strings.Contains(sent.Text, token) || strings.Contains(text, token) {
			input.References = append(input.References, ref)
		}
	}
	input.Attachments = slices.Clone(sent.Attachments)
	return input
}

// Start opens the unsent conversation of this one's location with the quoted
// selection added to its draft.
func (x *conversationActions) Start(text string) {
	root := x.r.localRuntime()
	var project *viewmodel.Project
	if session, ok := x.r.session(x.cv.ID); ok && session.Project != nil {
		for _, current := range root.App.Data.Projects {
			if current.ID == session.Project.String() && current.Host == x.cv.Node {
				project = current
			}
		}
	}
	root.newDraft(project)
	comp := root.App.Composer(root.App.Sidebar.Selected)
	if comp.Draft != "" && !strings.HasSuffix(comp.Draft, "\n\n") {
		comp.Draft += "\n\n"
	}
	comp.Draft += text
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
		if code := faultCode(err); err != nil && x.restore != nil && x.restoreDraft != composerFingerprint(x.comp) && code != protocol.ErrorCodeOutcomeUnknown {
			x.offerRestore(c, err)
		} else {
			x.r.report(c, err, "session_changed")
		}
		if done != nil {
			done(output, err == nil)
		}
	})
}

// offerRestore reports a failed send whose composer no longer holds what was
// sent: the toast's action puts the submission back into an empty composer.
func (x *conversationActions) offerRestore(c *ui.Context, err error) {
	message, comp := *x.restore, x.comp
	toast := ui.Toast{Type: "error", Title: i18n.T(c, "chat_action_failed"), Timeout: kit.ToastDuration(c), Action: i18n.T(c, "chat_restore_draft")}
	if code := faultCode(err); code == protocol.ErrorCodeConflict || code == protocol.ErrorCodeRevisionConflict {
		toast.Type, toast.Title = "warning", i18n.T(c, "session_changed")
	}
	toast.OnAction = func() {
		x.r.enqueue(func(c *ui.Context) {
			switch {
			case comp.Draft != "" || len(comp.References) > 0 || len(comp.Attachments) > 0:
				kit.Toast(c, kit.Failed, i18n.T(c, "queue_draft_present"))
			case x.readonly():
				kit.Toast(c, kit.Failed, i18n.T(c, "chat_restore_unavailable"))
			default:
				comp.Draft = message.Text
				comp.References = slices.Clone(message.References)
				comp.Attachments = slices.Clone(message.Attachments)
			}
		})
	}
	c.AddToast(toast)
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
