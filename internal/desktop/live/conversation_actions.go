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

// viewState shares the frame's observed source for presentation. Command
// admission uses state instead, reading the Client again at the action boundary.
func (x *conversationActions) viewState() (protocol.Session, protocol.ConversationSnapshot, bool) {
	if !x.owned() {
		return protocol.Session{}, protocol.ConversationSnapshot{}, false
	}
	s, ok := x.r.session(x.cv.ID)
	if !ok {
		return s, protocol.ConversationSnapshot{}, false
	}
	state, ok := x.r.conversationState(s.ID)
	return s, state.Snapshot, ok
}
func (x *conversationActions) queued(id string) (protocol.ConversationPending, bool) {
	_, state, ok := x.state()
	if !ok {
		return protocol.ConversationPending{}, false
	}
	return pendingQueueItem(state.Page.Queue, id)
}
func (x *conversationActions) SendQueued(id string) {
	if x.readonly() {
		return
	}
	session, state, ok := x.state()
	if !ok {
		return
	}
	page := state.Page
	item, ok := pendingQueueItem(page.Queue, id)
	if !ok {
		return
	}
	x.r.reconcileQueuedSends(session.ID, page.Queue)
	if x.r.queuedSends[session.ID][item.Turn] != nil {
		x.r.sendQueuedTurn(session.ID, page.Queue.Revision, item)
		return
	}
	if run, active := activeConversationRun(page); active && !page.Queue.Paused && run.Status != protocol.ConversationStatusStopping && !item.Notice && item.Kind == protocol.ConversationRunKindTask {
		if !acceptsGuidance(run) {
			kit.Toast(x.c, kit.Conflict, i18n.T(x.c, "session_changed"))
			return
		}
		x.queueCommand(protocol.Command{Kind: "guide_queued_turn", Data: protocol.CommandGuideQueuedTurnPayload{Turn: item.Turn, Recipient: run.Turn, ExpectedRevision: item.Revision}})
		return
	}
	x.r.sendQueuedTurn(session.ID, page.Queue.Revision, item)
}

// A subagent's session is read-only, but its queue tray still withdraws
// guidance and removes a message while its Host is connected.
func (x *conversationActions) WithdrawGuidance(id string) {
	if !x.owned() || !x.r.controller.Connected() {
		return
	}
	if p, ok := x.queued(id); ok {
		x.queueCommand(protocol.Command{Kind: "withdraw_guidance", Data: protocol.CommandWithdrawGuidancePayload{Turn: p.Turn, ExpectedRevision: p.Revision}})
	}
}
func (x *conversationActions) RemoveQueued(id string) {
	if !x.owned() || !x.r.controller.Connected() {
		return
	}
	if p, ok := x.queued(id); ok {
		x.queueCommand(protocol.Command{Kind: "remove_queued_turn", Data: protocol.CommandRemoveQueuedTurnPayload{Turn: p.Turn, ExpectedRevision: p.Revision}})
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
	x.queueCommand(protocol.Command{Kind: "move_queued_turn", Data: protocol.CommandMoveQueuedTurnPayload{Session: s.ID, ExpectedRevision: state.Page.Queue.Revision, Turn: item.Turn, Before: target}})
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
func (x *conversationActions) Compact() { x.compact("") }

// compact compacts the conversation's context, keeping focus in view when
// the user wrote one after /compact.
func (x *conversationActions) compact(focus string) {
	if x.readonly() {
		return
	}
	if s, ok := x.r.session(x.cv.ID); ok {
		payload := protocol.CommandCompactContextPayload{Session: s.ID, ExpectedRevision: s.Revision}
		if focus != "" {
			payload.Focus = &focus
		}
		x.turn(protocol.Command{Kind: "compact_context", Data: payload}, nil)
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
	approval, _, ok := x.r.waitingApproval(x.cv.ID, id, x.r.controller.Conversation)
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
	for _, view := range f.listed {
		if view.ID == id {
			attempt.receipt = view
		}
	}
	f.approval = attempt
	x.r.sendInteraction(f, attempt)
}

// question sends a response, or checks the original request if its outcome
// is still unknown.
func (x *conversationActions) question(id string, response protocol.ConversationQuestionResponse, done func(protocol.Output)) {
	if !x.interactive() {
		return
	}
	f := x.r.interactionFlow(x.cv.ID)
	if a := f.questions[id]; a != nil {
		x.r.sendInteraction(f, a)
		return
	}
	q, spec, ok := x.r.waitingQuestion(x.cv.ID, id)
	if !ok || f.settled[id] {
		kit.Toast(x.c, kit.Conflict, i18n.T(x.c, "question_closed"))
		return
	}
	prompt := spec.Prompt
	if spec.Input.Kind == "plan" {
		prompt = i18n.T(x.c, "plan_execute_confirm")
	}
	a := &interactionAttempt{id: id, question: true, prompt: prompt, done: done, request: x.r.controller.Prepare(protocol.Command{Kind: "resolve_question", Data: protocol.CommandResolveQuestionPayload{Session: q.Session, Question: q.ID, Response: response}})}
	f.questions[id] = a
	x.r.sendInteraction(f, a)
}

// questionLink is the address a link question may open. The link came from an
// MCP server through a Node, so this Desktop checks it again.
func questionLink(input protocol.ConversationQuestionInput) (string, bool) {
	link, ok := detail[protocol.ConversationQuestionInputUrlPayload](input.Data)
	if !ok || !protocol.QuestionLinkAllowed(link.Url) {
		return "", false
	}
	return link.Url, true
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
		if f := x.r.interactionFlow(x.cv.ID); f.questions[id] == nil && !f.settled[id] && x.r.controller.Connected() {
			link, ok := questionLink(spec.Input)
			if !ok || x.r.openExternalConversationURL(x.c, x.cv.ID, link) != nil {
				// One this Desktop would not open is not answered as opened.
				return
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
	s, state, ok := x.state()
	if !ok {
		return
	}
	through, err := protocol.ParseTurnID(turn)
	if err != nil || x.controlsFrom(s, state, ok)(through).NoFork {
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

// Rewind keeps the conversation through turn and saves the removed history
// as a backup session.
func (x *conversationActions) Rewind(turn string) {
	s, state, ok := x.state()
	if !ok || len(state.Page.Runs) == 0 {
		return
	}
	through, err := protocol.ParseTurnID(turn)
	if err != nil {
		return
	}
	if x.controlsFrom(s, state, ok)(through).NoRewind {
		// The history moved while the confirmation was open.
		kit.Toast(x.c, kit.Conflict, i18n.T(x.c, "chat_history_changed"))
		return
	}
	id := x.cv.ID
	head := state.Page.Runs[len(state.Page.Runs)-1].Turn
	x.r.conversationPending[id] = through
	x.r.execute(protocol.Command{Kind: "rewind_conversation", Data: protocol.CommandRewindConversationPayload{Session: s.ID, Through: &through, ExpectedHead: head, ExpectedRevision: state.Page.Revision}}, func(c *ui.Context, _ protocol.Output, err error) {
		delete(x.r.conversationPending, id)
		if err != nil {
			x.r.reportKeys(c, err, "chat_action_failed", "chat_history_changed", "chat_action_unknown")
			return
		}
		_ = x.r.controller.RefreshConversation(x.r.ctx, s.ID)
		c.AddToast(ui.Toast{ID: "rewound/" + id, Type: "success", Title: i18n.T(c, "chat_rewound"), Timeout: kit.ToastDuration(c)})
	})
}

// controls is which actions of a turn apply now.
func (x *conversationActions) controls(turn protocol.TurnID) conversation.Controls {
	session, state, ok := x.state()
	return x.controlsFrom(session, state, ok)(turn)
}

// pageControls reads the conversation once and answers which actions of each
// turn apply: a read-only (delegated or unowned) session hides Edit and the
// fork menu; the rest wait for a connected, idle conversation.
func (x *conversationActions) pageControls() func(protocol.TurnID) conversation.Controls {
	session, state, ok := x.viewState()
	if facts := x.r.conversationCache[session.ID].controls; ok && facts != nil {
		return x.controlsWith(session, facts, true)
	}
	return x.controlsFrom(session, state, ok)
}

type conversationControlFacts struct {
	busy                  bool
	head                  protocol.TurnID
	ended, sent, editable map[protocol.TurnID]bool
}

func controlFacts(page protocol.ConversationPage) *conversationControlFacts {
	ended, sent, editable := map[protocol.TurnID]bool{}, map[protocol.TurnID]bool{}, map[protocol.TurnID]bool{}
	for _, run := range page.Runs {
		ended[run.Turn] = run.Status != protocol.ConversationStatusQueued && run.Status != protocol.ConversationStatusRunning && run.Status != protocol.ConversationStatusStopping
		editable[run.Turn] = run.Kind == protocol.ConversationRunKindTask && run.Joined == nil
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
	return &conversationControlFacts{pageBusy(page), head, ended, sent, editable}
}

func (x *conversationActions) controlsFrom(session protocol.Session, state protocol.ConversationSnapshot, ok bool) func(protocol.TurnID) conversation.Controls {
	return x.controlsWith(session, controlFacts(state.Page), ok)
}

func (x *conversationActions) controlsWith(session protocol.Session, f *conversationControlFacts, ok bool) func(protocol.TurnID) conversation.Controls {
	if !ok || delegated(session) {
		return func(protocol.TurnID) conversation.Controls {
			return conversation.Controls{ReadOnly: true, NoEdit: true, NoFork: true, NoRewind: true, NoWorktree: true}
		}
	}
	edit, replacing := x.r.conversationEdits[x.cv.ID]
	pending, operating := x.r.conversationPending[x.cv.ID]
	blocked := !x.r.controller.Connected() || operating || replacing && edit.pending
	movable := session.Delegation == nil && len(x.comp.Attachments) == 0 && !f.busy
	return func(turn protocol.TurnID) conversation.Controls {
		_, found := f.ended[turn]
		return conversation.Controls{
			NoEdit:     blocked || f.busy || !f.editable[turn] || !f.sent[turn],
			NoFork:     blocked || !f.ended[turn],
			NoRewind:   blocked || f.busy || !found || f.head == turn,
			NoWorktree: blocked || !f.ended[turn] || !movable,
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
	session, state, ok := x.state()
	id, err := protocol.ParseTurnID(turn)
	if !ok || err != nil || x.controlsFrom(session, state, ok)(id).NoEdit {
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
	x.turn(protocol.Command{Kind: "replace_turn", Data: protocol.CommandReplaceTurnPayload{Session: session.ID, Turn: edit.Turn, ExpectedHead: edit.Head, ExpectedHistoryRevision: edit.History, ExpectedRevision: session.Revision, Message: editedInput(edit.Input, text)}}, func(_ protocol.Output, err error) {
		current, editing := x.r.conversationEdits[id]
		if !editing || current.Turn != edit.Turn {
			return
		}
		if err == nil {
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
	key, session, observed, draft string
	request                       protocol.RequestID
	admission                     <-chan protocol.TurnSummary
	stopAdmission                 func()
	comp                          *composer.State
	done                          func(protocol.Output, error)
}

func turnObserved(page protocol.ConversationPage) string {
	if page.Session == (protocol.SessionID{}) {
		// A just-mounted feed can report Loading without a history page.
		return ""
	}
	last := protocol.ConversationRun{}
	for i := len(page.Runs) - 1; i >= 0; i-- {
		if page.Runs[i].Joined == nil {
			last = page.Runs[i]
			break
		}
	}
	return fmt.Sprint(page.Revision, last.Turn, last.Status, page.Queue.Revision)
}

// turn sends a command that adds to or replaces the session's turns.
func (x *conversationActions) turn(command protocol.Command, done func(protocol.Output, error)) protocol.RequestID {
	id := x.cv.ID
	scope := "turn/" + id
	if x.settled != nil {
		scope = "form/" + id
	}
	encoded, _ := json.Marshal(command)
	key := scope + "\x00" + string(encoded)
	// A composer has one editable draft. Each submitted form has its own
	// reservation, identified in the command by artifact, revision and form.
	slot := id
	var observed, draft string
	if x.settled != nil {
		slot = key
	} else {
		draft = composerFingerprint(x.comp)
		if _, state, ok := x.state(); ok {
			observed = turnObserved(state.Page)
		}
		if retry, ok := x.r.turnRetries[slot]; ok {
			if retry.key != key || retry.observed != "" && observed != "" && retry.observed != observed {
				x.r.forgetTurnRetry(slot)
			} else if observed == "" {
				observed = retry.observed
			}
		}
	}
	var sent protocol.RequestID
	request := x.r.executeObserved(scope, command, func(request protocol.RequestID) {
		sent = request
		retry := x.r.turnRetries[slot]
		if retry.request != request {
			x.r.forgetTurnRetry(slot)
			retry.admission, retry.stopAdmission = x.r.controller.WatchTaskAdmission(request)
		}
		retry.key, retry.session, retry.request, retry.comp, retry.done = key, id, request, x.comp, done
		retry.observed, retry.draft = observed, draft
		if x.r.turnRetries == nil {
			x.r.turnRetries = map[string]turnRetry{}
		}
		x.r.turnRetries[slot] = retry
	}, func(c *ui.Context, output protocol.Output, err error) {
		if retry, ok := x.r.turnRetries[slot]; ok && retry.request == sent {
			if _, uncertain := x.r.uncertain[key]; uncertain || x.settled != nil && faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
				if retry.admitted(x.r.snapshot().Turns, nil) {
					x.r.forgetTurnRetry(slot)
					err = nil
				}
			} else {
				x.r.forgetTurnRetry(slot)
			}
		}
		if code := faultCode(err); err != nil && x.restore != nil && x.restoreDraft != composerFingerprint(x.comp) && code != protocol.ErrorCodeOutcomeUnknown {
			x.restoreFailedDraft(c, err)
		} else {
			x.r.report(c, err, "session_changed")
		}
		if done != nil {
			done(output, err)
		}
	})
	if request == (protocol.RequestID{}) && sent != (protocol.RequestID{}) {
		x.r.forgetTurnRetry(slot)
	}
	return request
}

func (x *conversationActions) restoreFailedDraft(c *ui.Context, err error) {
	x.comp.KeepFailedDraft(*x.restore, x.owned())
	x.r.reportKeys(c, err, "chat_action_failed", "session_changed", "chat_action_unknown")
}

func (r *Runtime) forgetTurnRetry(id string) {
	if retry, ok := r.turnRetries[id]; ok {
		if retry.stopAdmission != nil {
			retry.stopAdmission()
		}
		r.dropUncertainCommand(retry.key)
		delete(r.turnRetries, id)
	}
}

func (retry *turnRetry) admitted(turns []protocol.TurnSummary, queue []protocol.ConversationPending) bool {
	select {
	case <-retry.admission:
		return true
	default:
	}
	return slices.ContainsFunc(turns, func(turn protocol.TurnSummary) bool { return turn.Request == retry.request }) ||
		slices.ContainsFunc(queue, func(item protocol.ConversationPending) bool {
			return item.Request != nil && *item.Request == retry.request
		})
}

func (r *Runtime) confirmTurnRetry(slot string, retry turnRetry) {
	r.forgetTurnRetry(slot)
	if retry.done != nil {
		retry.done(protocol.Output{}, nil)
	}
}

// settleTurnRetry ends a retained request that can no longer be a retry: its
// turn was admitted, the history or queue moved, or the draft was changed. An
// admitted turn completes the send that lost its result.
func (r *Runtime) settleTurnRetry(id string, comp *composer.State, turns []protocol.TurnSummary, page protocol.ConversationPage) {
	if len(page.Queue.Items) > 0 {
		for slot, retry := range r.turnRetries {
			if slot != id && retry.session == id && !r.busy[retry.key] && retry.admitted(turns, page.Queue.Items) {
				r.confirmTurnRetry(slot, retry)
			}
		}
	}
	retry, ok := r.turnRetries[id]
	if !ok || r.busy[retry.key] {
		return
	}
	if retry.admitted(turns, page.Queue.Items) {
		r.confirmTurnRetry(id, retry)
		return
	}
	observed := turnObserved(page)
	if retry.observed == "" && observed != "" {
		// The request was sent before its new session's history was read.
		retry.observed = observed
		r.turnRetries[id] = retry
	}
	if (observed == "" || retry.observed == observed) && (retry.comp != comp || retry.draft == composerFingerprint(comp)) {
		return
	}
	r.forgetTurnRetry(id)
}

func composerFingerprint(s *composer.State) string {
	type reference struct {
		ID, Label, Source, Insert string
		Kind                      composer.ReferenceKind
	}
	refs := make([]reference, 0, len(s.References))
	for _, ref := range s.References {
		refs = append(refs, reference{ref.ID, ref.Label, ref.Source, ref.Insert, ref.Kind})
	}
	// Upload progress and publication change an attachment in place. Draft
	// identity follows the owned cards, including files not yet assigned an ID.
	attachments := make([]string, 0, len(s.Attachments))
	for _, attachment := range s.Attachments {
		attachments = append(attachments, fmt.Sprintf("%p", attachment))
	}
	b, _ := json.Marshal(struct {
		Text        string
		References  []reference
		Attachments []string
	}{s.Draft, refs, attachments})
	return string(b)
}

func (x *conversationActions) readonly() bool {
	if !x.owned() {
		return true
	}
	session, ok := x.r.session(x.cv.ID)
	return !x.r.controller.Connected() || ok && delegated(session)
}
