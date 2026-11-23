package live

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSendQueuedAfterStopChecksOriginalRequest(t *testing.T) {
	session, recipient, queued := protocol.SessionID{1}, protocol.TurnID{2}, protocol.TurnID{3}
	other, original := protocol.TurnID{4}, protocol.NewRequestID()
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Sequence = protocol.NodeID{1}, 1
	snapshot.Page.Session, snapshot.Page.Revision = session, 1
	run := protocol.ConversationRun{Session: session, Turn: recipient, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning, Sequence: 1, Revision: 1}
	snapshot.Page.Runs = []protocol.ConversationRun{run}
	first := protocol.ConversationPending{Turn: other, Kind: protocol.ConversationRunKindTask, Revision: 1, ConfigRevision: 1, Ready: true, Preview: "First queued input"}
	chosen := protocol.ConversationPending{Turn: queued, Request: &original, Kind: protocol.ConversationRunKindTask, Revision: 7, ConfigRevision: 1, Ready: true, Preview: "Chosen queued input"}
	snapshot.Page.Queue = protocol.ConversationQueue{Revision: 1, Items: []protocol.ConversationPending{first, chosen}}
	r, cv, transport, frame := watchCallbackTranscript(t, snapshot)
	paused := snapshot.Page.Queue
	paused.Revision, paused.Paused, paused.PauseReason = paused.Revision+1, true, new(protocol.ConversationQueuePauseStopped)
	resumed := protocol.ConversationQueue{Revision: paused.Revision + 2, Items: []protocol.ConversationPending{chosen, first}}
	result := protocol.Output{Kind: "queue", Data: protocol.ConversationQueueUpdate{Session: session, Queue: resumed}}
	var sends []protocol.Request
	var inspected []protocol.RequestID
	transport.mu.Lock()
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "inspect_request":
			query, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
			inspected = append(inspected, query.ID)
			outcome := protocol.RequestOutcome{Kind: "admitted"}
			if len(inspected) > 1 {
				outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &result}}
			}
			return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: outcome}}}, true
		}
		if request.Command.Durable() {
			sends = append(sends, request)
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "fixture lost the queue command reply"}}, true
		}
		return link.Response{}, false
	}
	transport.mu.Unlock()

	comp := composer.New(cv.ID)
	tester := ui.NewTesterWith(func(c *ui.Context) {
		// NewTesterWith renders before SetLocale. Mount the English feed
		// only once the window has the same locale as its prepared rows.
		if c.Locale() != "en" {
			return
		}
		theme.Apply(c, "", theme.Light)
		r.mu.Lock()
		callbacks := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, callback := range callbacks {
			callback(c)
		}
		r.beginConversationFrame()
		defer r.finishConversationFrame()
		r.conversationSeen[session] = true
		state, _ := r.conversationState(session)
		r.projectWatchedConversation(c, cv, session, state)
		actions := &conversationActions{r: r, c: c, cv: cv, comp: comp}
		view := &composer.Context{Queue: cv.Queue, Activity: composer.Activity{Connected: true}}
		r.projectComposerQueue(actions, view)
		ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, comp, view, actions) })
	}, 840, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tester.SetLocale("en")
	tester.Frame()
	tester.Frame()
	unfoldQueue(t, tester)
	pressChosen := func() {
		t.Helper()
		nodes := tester.AccessAll()
		var row ui.Rect
		for _, node := range nodes {
			if node.Name == chosen.Preview && node.Bounds.W > row.W {
				row = node.Bounds
			}
		}
		for _, node := range nodes {
			bounds := node.Bounds
			x, y := bounds.X+bounds.W/2, bounds.Y+bounds.H/2
			if node.Role == ui.RoleButton && node.Name == i18n.In("en", "queue_send_now") && !node.Disabled &&
				x >= row.X && x <= row.X+row.W && y >= row.Y && y <= row.Y+row.H {
				tester.ClickAt(x, y)
				tester.Frame()
				r.workers.Wait()
				tester.Frame()
				return
			}
		}
		t.Fatalf("chosen queue row has no enabled send button: %q", tester.Texts())
	}
	assertOriginal := func(lookups int) {
		t.Helper()
		transport.mu.Lock()
		defer transport.mu.Unlock()
		if len(sends) != 1 || sends[0].Command.Kind != "send_queued_turn" {
			t.Fatalf("queue commands = %+v, want one send_queued_turn", sends)
		}
		request := sends[0]
		input, valid := detail[protocol.CommandSendQueuedTurnPayload](request.Command.Data)
		if !valid || request.Target != snapshot.Node || request.ID == original || input.Turn != queued || input.ExpectedRevision != chosen.Revision {
			t.Fatalf("send used the wrong queue item or request: %+v", request)
		}
		if len(inspected) != lookups {
			t.Fatalf("lookups = %v, want %d of %v", inspected, lookups, request.ID)
		}
		for _, id := range inspected {
			if id != request.ID {
				t.Fatalf("lookup used %v, want original command %v", id, request.ID)
			}
		}
	}
	// The action sees the stopping frame before the next render or pause frame.
	run.Status = protocol.ConversationStatusStopping
	frame("run", run)
	pressChosen()
	assertOriginal(1)

	// The old turn ends and releases guidance before the reply is recovered.
	frame("queue", paused)
	run.Status = protocol.ConversationStatusCancelled
	frame("run", run)
	released := chosen
	released.Revision++
	released.Guidance = nil
	frame("queue", protocol.ConversationQueue{Revision: resumed.Revision + 1, Items: []protocol.ConversationPending{released, first}})
	tester.Frame()
	pressChosen()
	assertOriginal(2)

	// A failed read leaves the last coherent queue available to the composer.
	// Its diagnostic must not release the accepted command for another send.
	wake := make(chan struct{}, 1)
	r.SetInvalidate(func() {
		select {
		case wake <- struct{}{}:
		default:
		}
	})
	state, _ := r.controller.Conversation(session)
	if items := state.Snapshot.Page.Queue.Items; state.Snapshot.Page.Queue.Paused || len(items) != 2 ||
		items[0].Turn != queued || items[0].Request == nil || *items[0].Request != original || items[1].Turn != other {
		t.Fatalf("resumed queue did not retain the original selection and input: %+v", state.Snapshot.Page.Queue)
	}
	transport.history.updates <- protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{
		Node: protocol.NodeID{9}, Session: session, Sequence: state.Snapshot.Sequence + 1,
		Change: protocol.ConversationChange{Kind: "queue", Data: resumed},
	}}
	deadline := time.After(5 * time.Second)
	for cv.Session.Diagnostic == nil {
		tester.Frame()
		if cv.Session.Diagnostic != nil {
			break
		}
		select {
		case <-wake:
		case <-deadline:
			t.Fatal("the failed conversation read was not projected")
		}
	}
	if cv.Session.Diagnostic.Title != "chat_sync_failed" {
		t.Fatalf("unexpected read diagnostic: %+v", cv.Session.Diagnostic)
	}
	pressChosen()
	assertOriginal(2)
}
