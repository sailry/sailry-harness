package live

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type questionReceiptTransport struct {
	conversationTestTransport
	observation     *conversationTestSubscription
	first           protocol.QuestionID
	firstDispatched chan struct{}
	completion      chan link.Response
}

func (f *questionReceiptTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "conversation" {
		return f.observation, nil
	}
	return f.conversationTestTransport.Subscribe(ctx, topic)
}

func (f *questionReceiptTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	admitted, err := f.conversationTestTransport.Dispatch(ctx, request)
	if request.Command.Kind == "resolve_question" {
		if input, ok := detail[protocol.CommandResolveQuestionPayload](request.Command.Data); ok && input.Question == f.first {
			admitted.Completion = f.completion
			select {
			case f.firstDispatched <- struct{}{}:
			default:
			}
		}
	}
	return admitted, err
}

func TestQuestionReceiptChecksOriginalRequest(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	node, id, turn := protocol.NodeID{1}, protocol.SessionID{2}, protocol.TurnID{3}
	session := protocol.DefaultSession()
	session.ID, session.Revision = id, 1
	session.Config.Mode, session.Config.Permission = protocol.WorkModePlan, protocol.PermissionAsk
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Sessions = node, []protocol.Session{session}
	first := protocol.ConversationQuestion{ID: protocol.QuestionID{4}, Session: id, Turn: turn, Entry: "first", State: protocol.ConversationQuestionState{Kind: "pending"}}
	second := protocol.ConversationQuestion{ID: protocol.QuestionID{5}, Session: id, Turn: turn, Entry: "second", State: protocol.ConversationQuestionState{Kind: "pending"}}
	transport := &questionReceiptTransport{
		conversationTestTransport: conversationTestTransport{snapshot: snapshot},
		observation:               &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})},
		first:                     first.ID,
		firstDispatched:           make(chan struct{}, 1),
		completion:                make(chan link.Response),
	}
	loseCompletion := sync.OnceFunc(func() { close(transport.completion) })
	defer loseCompletion()
	queued := protocol.DefaultQueuedTurn()
	queued.ID, queued.Session = protocol.TurnID{6}, id
	accepted := protocol.DefaultConversationQuestionAcceptedPlan()
	accepted.Question, accepted.Session, accepted.Turn = first, session, queued
	accepted.Question.State = protocol.ConversationQuestionState{Kind: "answered", Data: protocol.ConversationQuestionAnswer{Kind: "plan", Data: protocol.ConversationQuestionAnswerPlanPayload{Turn: queued.ID}}}
	accepted.Session.Revision++
	accepted.Session.Config.Mode = protocol.WorkModeCode
	result := protocol.Output{Kind: "plan_accepted", Data: accepted}
	acceptedSecond := second
	acceptedSecond.State = protocol.ConversationQuestionState{Kind: "answered", Data: protocol.ConversationQuestionAnswer{Kind: "text", Data: "Next answer"}}
	var inspected []protocol.CommandInspectRequestPayload
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "resolve_question":
			input, _ := detail[protocol.CommandResolveQuestionPayload](request.Command.Data)
			if input.Question == second.ID {
				return link.Response{Output: protocol.Output{Kind: "question", Data: acceptedSecond}}, true
			}
		case "inspect_request":
			input, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
			inspected = append(inspected, input)
			outcome := protocol.RequestOutcome{Kind: "admitted"}
			if len(inspected) > 1 {
				outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &result}}
			}
			return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: input.ID, Outcome: outcome}}}, true
		}
		return link.Response{}, false
	}
	inspections := func() int {
		transport.mu.Lock()
		defer transport.mu.Unlock()
		return len(inspected)
	}
	wakeup := make(chan struct{}, 1)
	signal := func() {
		select {
		case wakeup <- struct{}{}:
		default:
		}
	}
	wait := func(what string, ready func() bool) {
		t.Helper()
		for !ready() {
			select {
			case <-wakeup:
			case <-ctx.Done():
				t.Fatalf("timed out waiting for %s: %v", what, ctx.Err())
			}
		}
	}
	controller := client.New(transport)
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	release, err := controller.WatchConversation(ctx, id, signal)
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	publish := func(sequence uint64, question *protocol.ConversationQuestion, name string, spec protocol.ConversationQuestionSpec) {
		t.Helper()
		page := protocol.DefaultConversationPage()
		page.Session, page.Revision = id, 1
		page.Runs = []protocol.ConversationRun{{Session: id, Turn: turn, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning, Sequence: 1, Revision: 1}}
		if question != nil {
			summary := protocol.DefaultToolDetailSummary()
			summary.Question = new(&spec)
			call := protocol.DefaultConversationPartToolCallPayload()
			call.Name, call.Summary = name, &summary
			page.Questions = []protocol.ConversationQuestion{*question}
			page.Entries = []protocol.ConversationEntry{{ID: question.Entry, Turn: turn, Sequence: 1, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: call}}}}
		}
		observed := protocol.DefaultConversationSnapshot()
		observed.Node, observed.Sequence, observed.Page = node, sequence, page
		select {
		case transport.observation.updates <- protocol.Update{Kind: "conversation_snapshot", Data: observed}:
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
		wait("conversation snapshot", func() bool {
			seen, ok := controller.Conversation(id)
			return ok && seen.Connected && seen.Snapshot.Sequence == sequence
		})
	}

	text := func(key string) string { return i18n.In("en", key) }
	planPrompt := text("plan_execute_confirm")
	publish(1, &first, "plan_review", protocol.ConversationQuestionSpec{Input: protocol.ConversationQuestionInput{Kind: "plan"}})
	cv := &viewmodel.Conversation{ID: id.String(), Node: node.String()}
	r := &Runtime{ctx: ctx, controller: controller}
	r.SetInvalidate(signal)
	state := composer.New("")
	var projected composer.Interactions
	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		r.mu.Lock()
		callbacks := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, callback := range callbacks {
			callback(c)
		}
		actions := &conversationActions{r: r, c: c, cv: cv, comp: state}
		history, _ := controller.Conversation(id)
		_, _, waiting, _ := Conversation(history)
		view := &composer.Context{Docked: true, Interactions: waiting}
		r.projectInteractionStates(cv, view, "en")
		projected = view.Interactions
		ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, state, view, actions) })
		kit.ToastHost(c)
	}, 760, 460, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tester.SetLocale("en")
	click := func(label string) {
		t.Helper()
		if err := tester.Click(label); err != nil {
			t.Fatal(err, tester.Texts())
		}
		tester.Frame()
	}
	waitFrame := func(what string, ready func() bool) {
		t.Helper()
		wait(what, func() bool {
			tester.Frame()
			return ready()
		})
	}
	tester.Frame()
	click(text("question_reply"))
	click(text("interaction_start"))
	select {
	case <-transport.firstDispatched:
	case <-ctx.Done():
		t.Fatal("the first answer was not dispatched", tester.Texts())
	}
	if projected.Question == nil || !projected.Question.Busy {
		t.Fatal("the first answer was not in flight", tester.Texts())
	}
	publish(2, nil, "", protocol.ConversationQuestionSpec{})
	tester.Frame()
	busyReceipt := func() bool {
		return len(projected.QuestionReceipts) == 1 && projected.QuestionReceipts[0].ID == first.ID.String() && projected.QuestionReceipts[0].Busy
	}
	if projected.Question != nil || !busyReceipt() {
		t.Fatal("the closed card did not leave its busy receipt")
	}
	publish(3, &second, "ask_user", protocol.ConversationQuestionSpec{Prompt: "Next question prompt", Input: protocol.ConversationQuestionInput{
		Kind: "text", Data: protocol.ConversationQuestionInputTextPayload{MaxBytes: 1024},
	}})
	tester.Frame()
	if q := projected.Question; q == nil || q.ID != second.ID.String() || q.Busy || q.Disabled || q.Check != nil {
		t.Fatal("the old attempt blocked the next question", tester.Texts())
	}
	field, ok := tester.AccessByRole(ui.RoleTextField, text("interaction_input"))
	if !ok || field.Disabled {
		t.Fatal("the next question has no editable answer", tester.Texts())
	}
	b := field.Bounds
	tester.ClickAt(b.X+b.W/2, b.Y+b.H/2)
	tester.Type("Next answer")
	tester.Frame()
	click(text("interaction_submit"))
	waitFrame("the next answer", func() bool { return projected.Question == nil })
	if !busyReceipt() {
		t.Fatal("answering the next question did not leave the old receipt intact")
	}
	loseCompletion()
	canCheck := func() bool {
		receipts := projected.QuestionReceipts
		return len(receipts) == 1 && receipts[0].ID == first.ID.String() && !receipts[0].Busy && receipts[0].Check != nil
	}
	waitFrame("the lost answer's receipt", canCheck)
	if inspections() != 0 {
		t.Fatal("the lost answer was checked without a user action")
	}

	label := text("host_check_status")
	tester.Advance(10 * time.Second)
	tester.Frame()
	click(label)
	check := func() {
		t.Helper()
		if !tester.HasText(planPrompt) {
			t.Fatal("the receipt does not identify the closed question", tester.Texts())
		}
		for _, item := range tester.AccessAll() {
			if item.Role == ui.RoleButton && item.Name == label && !item.Expanded && !item.Disabled {
				b := item.Bounds
				tester.ClickAt(b.X+b.W/2, b.Y+b.H/2)
				return
			}
		}
		t.Fatal("the receipt has no enabled action", tester.Texts())
	}
	check()
	waitFrame("a pending inspection", func() bool { return inspections() == 1 && canCheck() })
	if _, applied := r.admittedSessions[cv.ID]; applied {
		t.Fatal("a pending inspection settled the answer")
	}
	check()
	waitFrame("the stored answer", func() bool { return len(projected.QuestionReceipts) == 0 })
	if recovered := r.admittedSessions[cv.ID]; recovered.Revision != accepted.Session.Revision || recovered.Config.Mode != accepted.Session.Config.Mode || !tester.HasText(text("activity_completed")) {
		t.Fatal("the stored result did not settle the original receipt", tester.Texts())
	}

	transport.mu.Lock()
	defer transport.mu.Unlock()
	var replies []protocol.Request
	for _, request := range transport.requests {
		if request.Command.Kind == "resolve_question" {
			replies = append(replies, request)
		}
	}
	if len(replies) != 2 || len(inspected) != 2 {
		t.Fatalf("wanted two answers and two read-only checks, got %d and %d", len(replies), len(inspected))
	}
	firstInput, _ := detail[protocol.CommandResolveQuestionPayload](replies[0].Command.Data)
	start, ok := detail[protocol.ConversationQuestionResponseStartCodingPayload](firstInput.Response.Data)
	if !ok || firstInput.Question != first.ID || firstInput.Response.Kind != "start_coding" || start.ExpectedRevision != session.Revision || start.Message.Text != text("plan_coding_message") {
		t.Fatalf("wrong plan answer request: %+v", firstInput)
	}
	secondInput, _ := detail[protocol.CommandResolveQuestionPayload](replies[1].Command.Data)
	answer, ok := detail[protocol.ConversationQuestionAnswer](secondInput.Response.Data)
	answerText, textOK := detail[string](answer.Data)
	if !ok || !textOK || secondInput.Question != second.ID || secondInput.Response.Kind != "answer" || answer.Kind != "text" || answerText != "Next answer" {
		t.Fatalf("wrong answer request for the next question: %+v", secondInput)
	}
	original, err := json.Marshal(replies[0])
	if err != nil {
		t.Fatal(err)
	}
	digest := blake3.Sum256(original)
	for _, query := range inspected {
		if query.ID != replies[0].ID || query.Digest != hex.EncodeToString(digest[:]) {
			t.Fatal("inspection did not use the original request ID and content")
		}
	}
}
