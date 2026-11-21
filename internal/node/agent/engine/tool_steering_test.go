package engine

import (
	"context"
	"database/sql"
	"fmt"
	"strings"
	"sync/atomic"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestQueuedGuidanceWaitsForNoninterruptibleTool(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	var skipped atomic.Int32
	tool := objectTool("noninterruptible", false, false, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
		close(started)
		select {
		case <-release:
			return ToolResult{Value: "write finished"}
		case <-ctx.Done():
			t.Error("steering cancelled the write's execution context")
			return toolFault(ctx.Err())
		}
	})
	later := objectTool("unneeded", true, false, func(context.Context, canonical.ToolRequest) ToolResult {
		skipped.Add(1)
		return ToolResult{Value: "unexpected call"}
	})
	m := &scriptedModel{scripts: []script{
		response(call("effect", "noninterruptible", `{}`), call("later", "unneeded", `{}`), finish()),
		response(text("Follow-up"), finish()),
	}}
	f := newFixture(t, m, tool, later)
	f.start()
	turn := f.submit("Start the original task")
	select {
	case <-started:
	case <-f.ctx.Done():
		t.Fatal("write did not start")
	}
	first := f.queueForSteering("First new direction")
	second := f.queueForSteering("Second new direction")
	f.steerQueued(turn, first)
	f.steerQueued(turn, second)
	beforeRelease := m.count()
	close(release)
	if beforeRelease != 1 {
		t.Fatalf("model continued before the write settled: %d requests", beforeRelease)
	}
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	for _, item := range []protocol.ConversationPending{first, second} {
		if run := f.waitRun(item.Turn, protocol.ConversationStatusCompleted); run.Joined == nil || *run.Joined != turn {
			t.Fatalf("guidance was not joined to the running turn: %+v", run)
		}
	}
	if skipped.Load() != 0 || m.count() != 2 {
		t.Fatalf("skipped calls = %d, model requests = %d", skipped.Load(), m.count())
	}
	var success bool
	for _, record := range f.state().Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if result, ok := block.Data.(*canonical.ToolResponse); ok && result.ID == "effect" {
					success = result.Status == canonical.ToolSuccess && strings.Contains(string(result.Result.Value), "write finished")
				}
			}
		}
	}
	if !success {
		t.Fatal("the write's successful receipt was lost")
	}
	m.mu.Lock()
	input := promptText(m.calls[1])
	m.mu.Unlock()
	if firstAt, secondAt := strings.Index(input, first.Preview), strings.Index(input, second.Preview); firstAt < 0 || firstAt >= secondAt {
		t.Fatalf("selected messages did not reach the next request in queue order: %s", input)
	}
}

func TestQueuedGuidanceInterruptsParallelTools(t *testing.T) {
	started, cancelled := make(chan string, 2), make(chan string, 2)
	tool := objectTool("parallel_read", true, true, func(ctx context.Context, req canonical.ToolRequest) ToolResult {
		started <- req.ID
		<-ctx.Done()
		if !Steered(ctx) {
			t.Error("parallel call was not steered")
		}
		cancelled <- req.ID
		return toolFault(ctx.Err())
	})
	m := &scriptedModel{scripts: []script{
		response(call("a", "parallel_read", `{}`), call("b", "parallel_read", `{}`), finish()),
		response(text("Follow-up"), finish()),
	}}
	f := newFixture(t, m, tool)
	f.start()
	turn := f.submit("Run parallel tools")
	for range 2 {
		select {
		case <-started:
		case <-f.ctx.Done():
			t.Fatal("the parallel calls did not both start")
		}
	}
	item := f.queueForSteering("Redirect both calls")
	f.steerQueued(turn, item)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if len(cancelled) != 2 || m.count() != 2 {
		t.Fatalf("cancelled calls = %d, model requests = %d", len(cancelled), m.count())
	}
}

func TestQueuedGuidanceCanWithdrawBeforeToolsStart(t *testing.T) {
	streaming, release := make(chan struct{}), make(chan struct{})
	var calls atomic.Int32
	tool := objectTool("keep_working", false, false, func(context.Context, canonical.ToolRequest) ToolResult {
		calls.Add(1)
		return ToolResult{Value: "original work"}
	})
	m := &scriptedModel{scripts: []script{
		func(ctx context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
			close(streaming)
			select {
			case <-release:
				_ = yield(call("original", "keep_working", `{}`)) && yield(finish())
			case <-ctx.Done():
				t.Error("queued guidance interrupted the active model stream")
			}
		},
		response(text("Original work finished"), finish()),
	}}
	f := newFixture(t, m, tool)
	f.start()
	turn := f.submit("Keep the original task")
	select {
	case <-streaming:
	case <-f.ctx.Done():
		t.Fatal("model stream did not start")
	}
	item := f.queueForSteering("Direction I will withdraw")
	update := f.steerQueued(turn, item)
	for _, pending := range update.Queue.Items {
		if pending.Turn == item.Turn {
			item = pending
			break
		}
	}
	f.command("withdraw_guidance", &protocol.CommandWithdrawGuidancePayload{Turn: item.Turn, ExpectedRevision: item.Revision})
	close(release)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if calls.Load() != 1 || m.count() != 2 {
		t.Fatalf("original tool calls = %d, model requests = %d", calls.Load(), m.count())
	}
	if run := f.waitRun(item.Turn, protocol.ConversationStatusQueued); run.Joined != nil {
		t.Fatalf("withdrawn input was joined: %+v", run)
	}
}

func TestQueuedGuidanceTakesNextBoundedModelInput(t *testing.T) {
	streaming, release := make(chan struct{}), make(chan struct{})
	m := &scriptedModel{scripts: []script{
		func(ctx context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
			close(streaming)
			select {
			case <-release:
				_ = yield(text("First reply")) && yield(finish())
			case <-ctx.Done():
				t.Error("queued guidance interrupted the active model stream")
			}
		},
		response(text("Selected input handled"), finish()),
		response(text("Last input handled"), finish()),
	}}
	f := newFixture(t, m)
	f.start()
	turn := f.submit("Original task")
	select {
	case <-streaming:
	case <-f.ctx.Done():
		t.Fatal("model stream did not start")
	}
	for i := range 16 {
		f.command("guide_turn", &protocol.CommandGuideTurnPayload{
			Recipient: turn, ExpectedRevision: f.session.Revision, Message: protocol.ConversationInput{Text: fmt.Sprintf("Earlier guidance %02d", i)},
		})
	}
	item := f.queueForSteering("Explicitly selected guidance")
	f.steerQueued(turn, item)
	close(release)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if m.count() != 3 {
		t.Fatalf("model requests = %d, want a reply and two batches", m.count())
	}
	m.mu.Lock()
	first, next := promptText(m.calls[1]), promptText(m.calls[2])
	m.mu.Unlock()
	if !strings.Contains(first, item.Preview) || strings.Contains(first, "Earlier guidance 15") || !strings.Contains(next, "Earlier guidance 15") {
		t.Fatalf("selected guidance or bounded remainder was not delivered: %s\n%s", first, next)
	}
}

func TestQueuedGuidanceCancelsPendingInteraction(t *testing.T) {
	t.Run("approval", func(t *testing.T) {
		var calls atomic.Int32
		tool := objectTool("needs_approval", false, false, func(context.Context, canonical.ToolRequest) ToolResult {
			calls.Add(1)
			return ToolResult{Value: "unexpected execution"}
		})
		tool.Approval = true
		m := &scriptedModel{scripts: []script{response(call("write", "needs_approval", `{}`), finish()), response(text("Follow-up"), finish())}}
		f := newFixture(t, m, tool)
		f.start()
		turn := f.submit("Request approval")
		approval := f.waitingApproval()
		f.steerQueued(turn, f.queueForSteering("No need to do that"))
		f.waitRun(turn, protocol.ConversationStatusCompleted)
		if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
			var err error
			approval, err = store.ReadApproval(f.ctx, db, approval.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if approval.State != protocol.ConversationApprovalStateCancelled || calls.Load() != 0 || m.count() != 2 {
			t.Fatalf("approval after steering = %s, tool calls = %d, model requests = %d", approval.State, calls.Load(), m.count())
		}
	})

	t.Run("question", func(t *testing.T) {
		questionCall := call("question", "ask_user", `{"prompt":"What next?","input":{"kind":"text"}}`)
		m := &scriptedModel{scripts: []script{response(questionCall, finish()), response(text("Follow-up"), finish())}}
		f := newFixture(t, m)
		f.start()
		turn := f.submit("Ask a question")
		question := f.waitingQuestion()
		f.steerQueued(turn, f.queueForSteering("Continue with the new task"))
		f.waitRun(turn, protocol.ConversationStatusCompleted)
		if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
			var err error
			question, err = store.ReadQuestion(f.ctx, db, question.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if question.State.Kind != "cancelled" || m.count() != 2 {
			t.Fatalf("question after steering = %s, model requests = %d", question.State.Kind, m.count())
		}
	})
}

func (f *engineFixture) queueForSteering(text string) protocol.ConversationPending {
	f.t.Helper()
	output := f.command("queue_turn", &protocol.CommandQueueTurnPayload{
		Session: f.session.ID, ExpectedRevision: f.session.Revision, Message: protocol.ConversationInput{Text: text},
	})
	turn := output.Data.(protocol.QueuedTurn).ID
	var queue protocol.ConversationQueue
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		queue, err = store.ReadQueue(f.ctx, db, f.session.ID)
		return err
	}); err != nil {
		f.t.Fatal(err)
	}
	for _, item := range queue.Items {
		if item.Turn == turn {
			return item
		}
	}
	f.t.Fatalf("queued input %s is missing from %+v", turn, queue)
	return protocol.ConversationPending{}
}

func (f *engineFixture) steerQueued(recipient protocol.TurnID, item protocol.ConversationPending) protocol.ConversationQueueUpdate {
	f.t.Helper()
	return f.command("guide_queued_turn", &protocol.CommandGuideQueuedTurnPayload{Turn: item.Turn, Recipient: recipient, ExpectedRevision: item.Revision}).Data.(protocol.ConversationQueueUpdate)
}
