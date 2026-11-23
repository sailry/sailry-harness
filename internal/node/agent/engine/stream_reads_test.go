package engine

import (
	"context"
	"encoding/json"
	"io"
	"slices"
	"sync/atomic"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestStreamReadsOverlapAndRespectSerialCalls(t *testing.T) {
	started := make(chan string, 4)
	finishStream, finishReads := make(chan struct{}), make(chan struct{})
	var mutated atomic.Bool
	read := objectTool("read", true, true, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
		started <- call.ID
		if call.ID == "after" {
			return ToolResult{Value: mutated.Load()}
		}
		select {
		case <-finishReads:
		case <-ctx.Done():
		}
		return ToolResult{Value: true}
	})
	read.StreamRead = true
	serial := objectTool("serial", true, false, func(context.Context, canonical.ToolRequest) ToolResult {
		mutated.Store(true)
		return ToolResult{Value: true}
	})
	model := &scriptedModel{scripts: []script{
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			response(call("a", "read", `{}`), call("b", "read", `{}`),
				fantasy.StreamPart{Type: fantasy.StreamPartTypeToolInputStart, ID: "ordered", ToolCallName: "serial"},
				call("after", "read", `{}`))(ctx, request, yield)
			select {
			case <-finishStream:
				response(call("ordered", "serial", `{}`), text("Reads have started"), finish())(ctx, request, yield)
			case <-ctx.Done():
			}
		},
		response(text("Done"), finish()),
	}}
	f := newFixture(t, model, read, serial)
	f.start()
	reservation := f.r.feed.Reserve(f.session.ID, f.ctx.Done())
	defer reservation.Close()
	subscription := reservation.Subscribe(protocol.ConversationSnapshot{})
	turn := f.submit("Read")
	var early []string
	for range 2 {
		select {
		case id := <-started:
			early = append(early, id)
		case <-f.ctx.Done():
			t.Fatal("independent reads did not start before the model stream finished")
		}
	}
	slices.Sort(early)
	if !slices.Equal(early, []string{"a", "b"}) || mutated.Load() {
		t.Fatalf("unexpected work before the model stream finished: %v, serial=%t", early, mutated.Load())
	}
	for _, record := range f.state().Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if block.Kind == "tool_request" || block.Kind == "tool_response" {
					t.Fatalf("provisional tool work reached history: %s", block.Kind)
				}
			}
		}
	}
	close(finishStream)
	committed, active := map[string]bool{}, map[string]string{}
	readFrame := func() *protocol.ConversationFrame {
		t.Helper()
		for {
			update, err := subscription.Next(f.ctx)
			if err != nil {
				t.Fatalf("waiting for streamed read activity: %v", err)
			}
			if update.Kind == "reset_required" {
				t.Fatal("conversation subscription lost its cursor")
			}
			if frame, ok := update.Data.(*protocol.ConversationFrame); ok {
				return frame
			}
		}
	}
	for len(active) < 2 {
		frame := readFrame()
		switch change := frame.Change.Data.(type) {
		case *protocol.ConversationEntry:
			for _, part := range change.Parts {
				if call, ok := part.Data.(*protocol.ConversationPartToolCallPayload); ok && call.ID != nil {
					committed[*call.ID] = true
				}
			}
		case *protocol.ConversationDraft:
			for _, part := range change.Parts {
				if part.Kind != "resource" {
					continue
				}
				var activity struct{ Type, ID, Name string }
				if err := json.Unmarshal(*part.Data.(*json.RawMessage), &activity); err != nil {
					t.Fatal(err)
				}
				if activity.Type != "tool_started" {
					continue
				}
				if change.Turn != turn || activity.Name != "read" || !committed[activity.ID] || (activity.ID != "a" && activity.ID != "b") {
					t.Fatalf("unexpected activity before the early reads settled: %+v", activity)
				}
				active[change.ID] = activity.ID
			}
		}
	}
	close(finishReads)
	for len(active) > 0 {
		if hidden, ok := readFrame().Change.Data.(*protocol.ConversationChangeHiddenPayload); ok && hidden.Turn == turn {
			delete(active, hidden.ID)
		}
	}
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if len(started) != 1 || <-started != "after" {
		t.Fatal("an early read ran a second time or the dependent read was skipped")
	}
	var ids []string
	for _, record := range f.state().Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if result, ok := block.Data.(*canonical.ToolResponse); ok {
					ids = append(ids, result.ID)
					if result.ID == "after" && string(result.Result.Value) != "true" {
						t.Fatal("the dependent read ran before the serial call")
					}
				}
			}
		}
	}
	if !slices.Equal(ids, []string{"a", "b", "ordered", "after"}) {
		t.Fatalf("result order = %v", ids)
	}
}

func TestStreamReadsAreCancelledBeforeRetry(t *testing.T) {
	started, stopped := make(chan struct{}), make(chan struct{})
	var attempts atomic.Int32
	read := objectTool("read", true, true, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
		if attempts.Add(1) == 1 {
			close(started)
			<-ctx.Done()
			close(stopped)
			return ToolResult{Value: "Discarded read"}
		}
		return ToolResult{Value: "Retried read"}
	})
	read.StreamRead = true
	model := &scriptedModel{scripts: []script{
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			if !yield(call("same-id", "read", `{}`)) {
				return
			}
			select {
			case <-started:
				yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: io.ErrUnexpectedEOF})
			case <-ctx.Done():
			}
		},
		response(call("same-id", "read", `{}`), finish()),
		response(text("Done"), finish()),
	}}
	f := newFixture(t, model, read)
	f.start()
	turn := f.submit("Read with retry")
	select {
	case wait := <-f.r.clock.(*manualClock).waits:
		select {
		case <-stopped:
		default:
			t.Fatal("retry began while the previous read was still running")
		}
		close(wait.advance)
	case <-f.ctx.Done():
		t.Fatal("the failed model stream was not retried")
	}
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	var results []string
	for _, record := range f.state().Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if result, ok := block.Data.(*canonical.ToolResponse); ok {
					results = append(results, string(result.Result.Value))
				}
			}
		}
	}
	if attempts.Load() != 2 || !slices.Equal(results, []string{`"Retried read"`}) {
		t.Fatalf("executions = %d, recorded results = %v", attempts.Load(), results)
	}
}

func TestStreamReadsAreCancelledWithTheTurn(t *testing.T) {
	started, stopped := make(chan struct{}), make(chan struct{})
	read := objectTool("read", true, true, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
		close(started)
		<-ctx.Done()
		close(stopped)
		return ToolResult{Value: "Discarded read"}
	})
	read.StreamRead = true
	model := &scriptedModel{scripts: []script{func(ctx context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
		if yield(call("provisional", "read", `{}`)) {
			<-ctx.Done()
		}
	}}}
	f := newFixture(t, model, read)
	f.start()
	turn := f.submit("Read until stopped")
	select {
	case <-started:
	case <-f.ctx.Done():
		t.Fatal("the read did not start during the model stream")
	}
	f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: turn})
	f.waitRun(turn, protocol.ConversationStatusCancelled)
	select {
	case <-stopped:
	default:
		t.Fatal("the turn settled before its provisional read exited")
	}
	for _, record := range f.state().Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if block.Kind == "tool_request" || block.Kind == "tool_response" {
					t.Fatalf("stopped stream left a provisional %s", block.Kind)
				}
			}
		}
	}
}

func TestStreamReadsRejectRewrittenDuplicateCallID(t *testing.T) {
	var seenIDs []string
	read := objectTool("read", true, true, func(_ context.Context, call canonical.ToolRequest) ToolResult {
		seenIDs = append(seenIDs, call.ID)
		return ToolResult{Value: call.ID}
	})
	read.StreamRead = true
	model := &scriptedModel{scripts: []script{
		response(call("dup", "read", `{}`), finish()),
		response(call("dup", "read", `{}`), finish()),
		response(text("Done"), finish()),
	}}
	f := newFixture(t, model, read)
	f.start()
	turn := f.submit("Read twice with reused call ID")
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	var responseIDs []string
	for _, record := range f.state().Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if resp, ok := block.Data.(*canonical.ToolResponse); ok {
					responseIDs = append(responseIDs, resp.ID)
				}
			}
		}
	}
	if len(responseIDs) != 2 || responseIDs[0] != "dup" || responseIDs[1] == "dup" {
		t.Fatalf("duplicate call ID was not rewritten: %v", responseIDs)
	}
	if len(seenIDs) < 2 || seenIDs[len(seenIDs)-1] != responseIDs[1] {
		t.Fatalf("tool executed with stale ID %v instead of rewritten ID %s", seenIDs, responseIDs[1])
	}
}

func TestStreamReadsContinuePastUpdatePlan(t *testing.T) {
	started := make(chan string, 2)
	finishStream := make(chan struct{})
	read := objectTool("read", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		started <- "read"
		return ToolResult{Value: true}
	})
	read.StreamRead = true
	plan := objectTool("update_plan", true, false, func(context.Context, canonical.ToolRequest) ToolResult {
		return ToolResult{Value: true}
	})
	model := &scriptedModel{scripts: []script{
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			response(call("p1", "update_plan", `{}`), call("r1", "read", `{}`))(ctx, request, yield)
			select {
			case <-finishStream:
				response(text("Planned"), finish())(ctx, request, yield)
			case <-ctx.Done():
			}
		},
		response(text("Done"), finish()),
	}}
	f := newFixture(t, model, plan, read)
	f.start()
	turn := f.submit("Plan and read")
	select {
	case <-started:
	case <-f.ctx.Done():
		t.Fatal("stream read did not start past update_plan")
	}
	close(finishStream)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
}

func TestStreamReadsContinuePastConcurrentReadOnlyTool(t *testing.T) {
	readStarted := make(chan string, 2)
	finishStream := make(chan struct{})
	var fetchRan atomic.Bool
	fetch := objectTool("web_fetch", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		fetchRan.Store(true)
		return ToolResult{Value: "fetched"}
	})
	read := objectTool("read", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		readStarted <- "read"
		return ToolResult{Value: "file"}
	})
	read.StreamRead = true
	model := &scriptedModel{scripts: []script{
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			response(call("f1", "web_fetch", `{}`), call("r1", "read", `{}`))(ctx, request, yield)
			select {
			case <-finishStream:
				response(text("Loaded"), finish())(ctx, request, yield)
			case <-ctx.Done():
			}
		},
		response(text("Done"), finish()),
	}}
	f := newFixture(t, model, fetch, read)
	f.start()
	turn := f.submit("Fetch and read")
	select {
	case <-readStarted:
	case <-f.ctx.Done():
		t.Fatal("stream read did not start past concurrent read-only tool")
	}
	if fetchRan.Load() {
		t.Fatal("non-StreamRead tool executed before the model stream committed")
	}
	close(finishStream)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if !fetchRan.Load() {
		t.Fatal("non-StreamRead tool did not execute after the stream committed")
	}
	var ids []string
	for _, record := range f.state().Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if resp, ok := block.Data.(*canonical.ToolResponse); ok {
					ids = append(ids, resp.ID)
				}
			}
		}
	}
	if !slices.Equal(ids, []string{"f1", "r1"}) {
		t.Fatalf("result order = %v, want [f1 r1]", ids)
	}
}
