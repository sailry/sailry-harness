package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOneRequestPerStep(t *testing.T) {
	for _, scenario := range []string{"tool then response", "bounded empty responses", "output limit prevents empty retry", "partial failure", "redacted before text",
		"failed stream with a tool call", "tool panic", "turn panic", "credential renewed mid-turn"} {
		t.Run(scenario, func(t *testing.T) {
			m := &scriptedModel{}
			var executed atomic.Int32
			var expire func()
			probe := objectTool("probe", true, true, func(_ context.Context, call canonical.ToolRequest) ToolResult {
				if call.ID == "boom" {
					panic("fixture tool panic")
				}
				if call.ID == "first" && expire != nil {
					expire()
				}
				executed.Add(1)
				return ToolResult{Value: map[string]bool{"ok": true}}
			})
			prepared := objectTool("prepared", true, true, func(context.Context, canonical.ToolRequest) ToolResult { return ToolResult{} })
			prepared.Prepare = func(context.Context, canonical.ToolRequest) error { panic("fixture turn panic") }
			switch scenario {
			case "tool then response":
				m.scripts = []script{response(call("title", "set_session_title", `{"title":" Current task "}`), finish()), response(text("Done"), finish())}
			case "bounded empty responses":
				for range 4 {
					m.scripts = append(m.scripts, response(finish()))
				}
			case "output limit prevents empty retry":
				m.scripts = []script{response(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonLength})}
			case "partial failure":
				m.scripts = []script{response(text("Partial"), fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.InvalidResponse, Message: "fixture failure"}})}
			case "failed stream with a tool call":
				m.scripts = []script{response(text("Partial"), call("partial", "probe", `{}`), fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.RateLimited, Message: "fixture failure"}})}
			case "tool panic":
				m.scripts = []script{response(call("boom", "probe", `{}`), call("fine", "probe", `{}`), finish()), response(text("Done"), finish())}
			case "turn panic":
				m.scripts = []script{response(call("stuck", "prepared", `{}`), finish()), response(text("Recovered"), finish())}
			case "credential renewed mid-turn":
				m.scripts = []script{response(call("first", "probe", `{}`), finish()), response(call("second", "probe", `{}`), finish()), response(text("Done"), finish())}
			case "redacted before text":
				m.scripts = []script{response(fantasy.StreamPart{Type: fantasy.StreamPartTypeReasoningEnd, ID: "redacted", ProviderMetadata: fantasy.ProviderMetadata{anthropic.Name: &anthropic.ReasoningOptionMetadata{RedactedData: "opaque"}}}, text("Answer"), finish())}
			}
			f := newFixture(t, m, probe, prepared)
			var guard sync.Mutex
			var built []string
			authorizations, minted := 0, 0
			if scenario == "credential renewed mid-turn" {
				// An account token is minted for one hour and expires while the
				// first tool runs; an unexpired token costs no provider request.
				timer := f.r.clock.(*manualClock)
				token, expires := "", time.Time{}
				expire = func() {
					timer.mu.Lock()
					timer.now = timer.now.Add(2 * time.Hour)
					timer.mu.Unlock()
				}
				f.r.authorizeProvider = func(_ context.Context, inv *store.AgentInvocation) error {
					guard.Lock()
					defer guard.Unlock()
					authorizations++
					if now := timer.Now(); !now.Before(expires) {
						minted++
						token, expires = "token-"+string(rune('0'+minted)), now.Add(time.Hour)
					}
					secret := protocol.NewSecret(`{"kind":"copilot","access":"` + token + `","endpoint":"https://api.githubcopilot.com","renewal_pending":false}`)
					inv.Provider.Authentication, inv.Provider.Endpoint, inv.Secret = protocol.AuthenticationCopilot, "https://api.githubcopilot.com", &secret
					return nil
				}
				f.r.model = func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
					guard.Lock()
					var grant struct{ Access string }
					if err := json.Unmarshal([]byte(binding.Secret.Expose()), &grant); err != nil {
						return nil, err
					}
					built = append(built, grant.Access)
					guard.Unlock()
					return m, nil
				}
			}
			f.start()
			turn := f.submit("Task")
			status := protocol.ConversationStatusCompleted
			if scenario == "partial failure" || scenario == "failed stream with a tool call" || scenario == "turn panic" {
				status = protocol.ConversationStatusFailed
			}
			run := f.waitRun(turn, status)
			if scenario == "turn panic" {
				if run.Error == nil || run.Error.Code != protocol.ErrorCodeOutcomeUnknown {
					t.Fatalf("panicked turn failure=%+v", run.Error)
				}
				// The worker survives and the settled call does not poison the next request.
				f.waitRun(f.submit("Again"), protocol.ConversationStatusCompleted)
			}
			expected := map[string]int{"tool then response": 2, "bounded empty responses": 4, "tool panic": 2, "turn panic": 2, "credential renewed mid-turn": 3}[scenario]
			if expected == 0 {
				expected = 1
			}
			if m.count() != expected {
				t.Fatalf("requests=%d, want %d", m.count(), expected)
			}
			state := f.state()
			responses := map[string]string{}
			requests := 0
			for _, record := range state.Records {
				if message := record.Message(); message != nil {
					for _, block := range message.Content {
						switch value := block.Data.(type) {
						case *canonical.ToolRequest:
							requests++
						case *canonical.ToolResponse:
							responses[value.ID] = string(value.Status) + " " + string(value.Result.Value)
						}
					}
				}
			}
			switch scenario {
			case "failed stream with a tool call":
				mustHaveText(t, state.Records, "Partial")
				if requests != 0 || len(responses) != 0 || executed.Load() != 0 {
					t.Fatalf("failed response ran its call: requests=%d responses=%v executed=%d", requests, responses, executed.Load())
				}
				if failure, _ := json.Marshal(run.Error); !strings.Contains(string(failure), `"`+string(protocol.ModelFailureRateLimited)+`"`) {
					t.Fatalf("failure lost its kind: %s", failure)
				}
			case "tool panic":
				mustHaveText(t, state.Records, "Done")
				if executed.Load() != 1 || !strings.HasPrefix(responses["boom"], string(canonical.ToolErrorStatus)) || !strings.Contains(responses["boom"], "failed unexpectedly") || strings.Contains(responses["boom"], "fixture tool panic") {
					t.Fatalf("panicked call result=%q executed=%d", responses["boom"], executed.Load())
				}
			case "turn panic":
				mustHaveText(t, state.Records, "Recovered")
				if !strings.Contains(responses["stuck"], "interrupted before completing") {
					t.Fatalf("failed turn left its call unsettled: %v", responses)
				}
			case "credential renewed mid-turn":
				guard.Lock()
				defer guard.Unlock()
				if !reflect.DeepEqual(built, []string{"token-1", "token-2"}) || minted != 2 || authorizations < 4 {
					t.Fatalf("transports=%v minted=%d authorizations=%d", built, minted, authorizations)
				}
			case "tool then response":
				mustHaveText(t, state.Records, "Done")
				if state.Context.Title == nil || *state.Context.Title != "Current task" {
					t.Fatal("title did not commit through the canonical result")
				}
			case "partial failure":
				mustHaveText(t, state.Records, "Partial")
			case "bounded empty responses":
				mustHaveText(t, state.Records, "The model returned an empty response. Please resend your message to continue.")
			case "redacted before text":
				found := false
				for _, record := range state.Records {
					if message := record.Message(); message != nil {
						for _, block := range message.Content {
							if value, ok := block.Data.(*canonical.RedactedThinking); ok {
								found = len(value.Before) == 1 && value.Before[0] != nil && *value.Before[0] == 1
							}
						}
					}
				}
				if !found {
					t.Fatal("redacted carrier lost its position before the answer")
				}
			}
		})
	}
}
func TestParallelReadOnlyOrder(t *testing.T) {
	for _, count := range []int{2, 9} {
		t.Run(string(rune('0'+count)), func(t *testing.T) {
			started := make(chan string, 16)
			release := map[string]chan struct{}{}
			var running, maxRunning atomic.Int32
			tool := objectTool("read", true, true, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
				current := running.Add(1)
				for old := maxRunning.Load(); current > old && !maxRunning.CompareAndSwap(old, current); old = maxRunning.Load() {
				}
				started <- call.ID
				select {
				case <-release[call.ID]:
				case <-ctx.Done():
				}
				running.Add(-1)
				return ToolResult{Value: map[string]string{"id": call.ID}}
			})
			parts := []fantasy.StreamPart{}
			for i := 0; i < count; i++ {
				parts = append(parts, call(string(rune('a'+i)), "read", `{}`))
				release[string(rune('a'+i))] = make(chan struct{})
			}
			parts = append(parts, finish())
			m := &scriptedModel{scripts: []script{response(parts...), response(text("Done"), finish())}}
			f := newFixture(t, m, tool)
			f.start()
			turn := f.submit("Read")
			for range min(count, 8) {
				select {
				case <-started:
				case <-f.ctx.Done():
					t.Fatal("readonly batch did not run concurrently")
				}
			}
			if count > 8 {
				// One finished call frees its slot: the ninth starts while the
				// other seven, the first included, are still running.
				close(release["b"])
				select {
				case id := <-started:
					if id != "i" || running.Load() != 8 {
						t.Fatalf("freed slot started %q with %d running", id, running.Load())
					}
				case <-f.ctx.Done():
					t.Fatal("a slow call delayed the start of the ninth")
				}
			}
			for id, done := range release {
				if count <= 8 || id != "b" {
					close(done)
				}
			}
			f.waitRun(turn, protocol.ConversationStatusCompleted)
			if maxRunning.Load() != int32(min(count, 8)) {
				t.Fatalf("concurrency=%d", maxRunning.Load())
			}
			var got []string
			for _, record := range f.state().Records {
				if message := record.Message(); message != nil {
					for _, block := range message.Content {
						if result, ok := block.Data.(*canonical.ToolResponse); ok && result.Name == "read" {
							got = append(got, result.ID)
						}
					}
				}
			}
			var want []string
			for i := 0; i < count; i++ {
				want = append(want, string(rune('a'+i)))
			}
			if !reflect.DeepEqual(got, want) {
				t.Fatalf("result order=%v", got)
			}
		})
	}
	t.Run("stopped turn starts no call beyond the window", func(t *testing.T) {
		var starts atomic.Int32
		started, stopped, hold := make(chan struct{}, 16), make(chan struct{}, 16), make(chan struct{})
		tool := objectTool("read", true, true, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
			starts.Add(1)
			started <- struct{}{}
			<-ctx.Done()
			stopped <- struct{}{}
			<-hold
			return ToolResult{}
		})
		parts := []fantasy.StreamPart{}
		for i := range 9 {
			parts = append(parts, call(string(rune('a'+i)), "read", `{}`))
		}
		f := newFixture(t, &scriptedModel{scripts: []script{response(append(parts, finish())...)}}, tool)
		f.start()
		turn := f.submit("Read")
		for range 8 {
			select {
			case <-started:
			case <-f.ctx.Done():
				t.Fatal("window did not fill")
			}
		}
		f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: turn})
		for range 8 {
			select {
			case <-stopped:
			case <-f.ctx.Done():
				t.Fatal("running calls were not cancelled")
			}
		}
		close(hold)
		f.waitRun(turn, protocol.ConversationStatusInterrupted)
		if starts.Load() != 8 {
			t.Fatalf("stopped turn started %d calls", starts.Load())
		}
	})
	t.Run("tied stream slots keep arrival order", func(t *testing.T) {
		// A provider index can equal the position of a part without one.
		for range 32 {
			out := streamOutput{slots: map[string]int{}, indices: map[string]uint64{}}
			var want []string
			for i := range 8 {
				id := string(rune('a' + i))
				out.slot(id, "text").Data = &canonical.TextContent{Text: id}
				out.indices[id+"\x00text"] = 0
				want = append(want, id)
			}
			out.order()
			var got []string
			for _, block := range out.blocks {
				got = append(got, block.Data.(*canonical.TextContent).Text)
			}
			if !reflect.DeepEqual(got, want) {
				t.Fatalf("slot order=%v", got)
			}
		}
	})
	t.Run("mutating barriers", func(t *testing.T) {
		var mu sync.Mutex
		var order []string
		record := func(name string) func(context.Context, canonical.ToolRequest) ToolResult {
			return func(context.Context, canonical.ToolRequest) ToolResult {
				mu.Lock()
				order = append(order, name)
				mu.Unlock()
				return ToolResult{Value: map[string]bool{"ok": true}}
			}
		}
		m := &scriptedModel{scripts: []script{response(call("a", "first", `{}`), call("b", "write", `{}`), call("c", "last", `{}`), finish()), response(text("Done"), finish())}}
		f := newFixture(t, m, objectTool("first", true, true, record("first")), objectTool("write", false, false, record("write")), objectTool("last", true, true, record("last")))
		f.start()
		turn := f.submit("Task")
		f.waitRun(turn, protocol.ConversationStatusCompleted)
		if !reflect.DeepEqual(order, []string{"first", "write", "last"}) {
			t.Fatalf("barrier order=%v", order)
		}
	})
}
func TestGuidanceJoinsAtBoundary(t *testing.T) {
	received, release := make(chan struct{}), make(chan struct{})
	m := &scriptedModel{scripts: []script{func(ctx context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
		if !yield(text("Working")) {
			return
		}
		close(received)
		select {
		case <-release:
		case <-ctx.Done():
			return
		}
		yield(finish())
	}, response(text("Follow-up handled"), finish())}}
	f := newFixture(t, m)
	f.start()
	turn := f.submit("Initial")
	select {
	case <-received:
	case <-f.ctx.Done():
		t.Fatal("model did not start")
	}
	input := protocol.DefaultConversationInput()
	input.Text = "Guidance"
	output := f.command("guide_turn", &protocol.CommandGuideTurnPayload{Recipient: turn, ExpectedRevision: f.session.Revision, Message: input})
	guided := output.Data.(protocol.QueuedTurn).ID
	if m.count() != 1 {
		t.Fatal("guidance interrupted the active HTTP owner")
	}
	close(release)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	run := f.waitRun(guided, protocol.ConversationStatusCompleted)
	if run.Joined == nil || *run.Joined != turn {
		t.Fatal("guidance lost its response owner")
	}
	mustHaveText(t, f.state().Records, "Guidance")
	mustHaveText(t, f.state().Records, "Follow-up handled")
	if m.count() != 2 {
		t.Fatalf("requests=%d", m.count())
	}
}
func TestQueueOrder(t *testing.T) {
	m := &scriptedModel{scripts: []script{response(text("First reply"), finish()), response(text("Second reply"), finish())}}
	f := newFixture(t, m)
	f.start()
	input := protocol.DefaultConversationInput()
	input.Text = "First"
	first := f.command("queue_turn", &protocol.CommandQueueTurnPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision, Message: input}).Data.(protocol.QueuedTurn)
	input.Text = "Second"
	second := f.command("queue_turn", &protocol.CommandQueueTurnPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision, Message: input}).Data.(protocol.QueuedTurn)
	if m.count() != 0 {
		t.Fatal("held queue executed")
	}
	f.command("start_queued_turn", &protocol.CommandStartQueuedTurnPayload{Turn: first.ID})
	f.waitRun(first.ID, protocol.ConversationStatusCompleted)
	if m.count() != 1 {
		t.Fatal("unready queue item executed")
	}
	f.command("start_queued_turn", &protocol.CommandStartQueuedTurnPayload{Turn: second.ID})
	f.waitRun(second.ID, protocol.ConversationStatusCompleted)
	var inputs []string
	for _, record := range f.state().Records {
		if record.Provenance.Author == "user" {
			inputs = append(inputs, record.Message().Content[0].Data.(*canonical.TextContent).Text)
		}
	}
	if !reflect.DeepEqual(inputs, []string{"First", "Second"}) {
		t.Fatalf("queue order=%v", inputs)
	}
}
func TestStopSealsInterrupted(t *testing.T) {
	partial := make(chan struct{})
	closed := make(chan struct{})
	m := &scriptedModel{scripts: []script{func(ctx context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
		defer close(closed)
		if !yield(text("Keep this partial")) {
			return
		}
		close(partial)
		<-ctx.Done()
	}}}
	f := newFixture(t, m)
	f.start()
	turn := f.submit("Task")
	select {
	case <-partial:
	case <-f.ctx.Done():
		t.Fatal("no partial")
	}
	f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: turn})
	f.waitRun(turn, protocol.ConversationStatusInterrupted)
	select {
	case <-closed:
	default:
		t.Fatal("HTTP stream owner was not released")
	}
	state := f.state()
	mustHaveText(t, state.Records, "Keep this partial")
	for _, record := range state.Records {
		if message := record.Message(); message != nil && message.Role == canonical.Assistant && len(message.Content) > 0 {
			if message.Finish == nil || *message.Finish != canonical.Interrupted {
				t.Fatal("partial was not sealed interrupted")
			}
		}
	}
	if m.count() != 1 {
		t.Fatal("stopped model was replayed")
	}
	t.Run("steered foreground wait with a pending call", func(t *testing.T) {
		childStarted := make(chan struct{})
		var once sync.Once
		gated := objectTool("gated", false, false, func(context.Context, canonical.ToolRequest) ToolResult {
			return ToolResult{Value: map[string]bool{"ok": true}}
		})
		gated.Approval = true
		steered := make(chan bool, 1)
		step := func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			prompt, results := "", 0
			for _, message := range request.Prompt {
				for _, part := range message.Content {
					if value, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
						prompt += value.Text + "\n"
					}
					if part.GetType() == fantasy.ContentTypeToolResult {
						results++
					}
				}
			}
			switch {
			case !strings.Contains(prompt, "Parent task"):
				once.Do(func() { close(childStarted) })
				<-ctx.Done()
			case results == 0:
				_ = yield(call("spawn", "spawn_agent", `{"task":"Inspect the child scope"}`)) && yield(finish())
			default:
				steered <- strings.Contains(prompt, "New direction")
				_ = yield(call("gated", "gated", `{}`)) && yield(finish())
			}
		}
		fixture := newFixture(t, &scriptedModel{scripts: []script{step, step, step}}, gated)
		fixture.start()
		turn := fixture.submit("Parent task")
		select {
		case <-childStarted:
		case <-fixture.ctx.Done():
			t.Fatal("foreground child did not start")
		}
		input := protocol.DefaultConversationInput()
		input.Text = "New direction"
		fixture.command("guide_turn", &protocol.CommandGuideTurnPayload{Recipient: turn, ExpectedRevision: fixture.session.Revision, Message: input})
		select {
		case joined := <-steered:
			if !joined {
				t.Fatal("request after guidance carried neither the guidance nor the child result")
			}
		case <-fixture.ctx.Done():
			t.Fatal("guidance did not continue the waiting turn")
		}
		fixture.waitingApproval()
		fixture.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: turn})
		fixture.waitRun(turn, protocol.ConversationStatusInterrupted)
		var order []string
		for _, record := range fixture.state().Records {
			if message := record.Message(); message != nil {
				for _, block := range message.Content {
					switch value := block.Data.(type) {
					case *canonical.ToolResponse:
						order = append(order, "response "+value.ID)
					case *canonical.TextContent:
						if strings.Contains(value.Text, "was cancelled before it finished") {
							order = append(order, "cancellation note")
						}
					}
				}
			}
		}
		if !reflect.DeepEqual(order, []string{"response spawn", "response gated", "cancellation note"}) {
			t.Fatalf("stopped turn settlement=%v", order)
		}
	})
	t.Run("stop before worker registration", func(t *testing.T) {
		model := &scriptedModel{}
		fixture := newFixture(t, model)
		claimed, release := make(chan struct{}), make(chan struct{})
		publish := fixture.r.publish
		var once sync.Once
		fixture.r.publish = func() { once.Do(func() { close(claimed); <-release }); publish() }
		fixture.start()
		turn := fixture.submit("Task")
		select {
		case <-claimed:
		case <-fixture.ctx.Done():
			t.Fatal("claim did not commit")
		}
		// Force the stored stop directly while the publication callback is held.
		// Normal controller writes share that guard; this exercises the recheck
		// independently without waiting on the deliberately blocked publisher.
		err := fixture.db.Node.Write(fixture.ctx, func(db *sql.Conn) error {
			request := protocol.NewRequest(fixture.node, protocol.Command{Kind: "stop_turn", Data: &protocol.CommandStopTurnPayload{Turn: turn}})
			_, _, err := store.ExecuteAgentCommand(fixture.ctx, db, fixture.node, fixture.r.profile, fixture.node, request, fixture.r.clock.Now())
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		close(release)
		fixture.waitRun(turn, protocol.ConversationStatusInterrupted)
		if model.count() != 0 {
			t.Fatal("stopped claim started a model request")
		}
	})
}
func TestRestartNoReplay(t *testing.T) {
	m := &scriptedModel{}
	f := newFixture(t, m)
	turn := f.submit("Task")
	err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		inv, _, err := store.ClaimNext(f.ctx, db, f.r.clock.Now())
		if err != nil {
			return err
		}
		if inv == nil {
			return context.Canceled
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if err = f.r.Recover(f.ctx); err != nil {
		t.Fatal(err)
	}
	f.start()
	run := f.waitRun(turn, protocol.ConversationStatusInterrupted)
	if run.Error == nil || run.Error.Code != protocol.ErrorCodeOutcomeUnknown || !strings.Contains(run.Error.Message, "not replayed") {
		t.Fatal("restart lost uncertainty")
	}
	if m.count() != 0 {
		t.Fatal("recovery replayed model")
	}
	t.Run("unanswered call does not reach the next request", func(t *testing.T) {
		m := &scriptedModel{scripts: []script{response(text("Second answer"), finish())}}
		f := newFixture(t, m)
		first := f.submit("First")
		// The process ended after the model's tool request committed and
		// before its result did.
		err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
			inv, _, err := store.ClaimNext(f.ctx, db, f.r.clock.Now())
			if err != nil || inv == nil {
				return errors.Join(err, context.Canceled)
			}
			for _, record := range []canonical.Record{
				inputRecord(first, inv.Message, f.r),
				canonical.NewRecord(first, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &canonical.ToolRequest{ID: "crashed", Name: "probe", Arguments: json.RawMessage(`{}`)}}), f.r.clock.Now()),
			} {
				if _, _, err = store.AppendCanonical(f.ctx, db, f.session.ID, record); err != nil {
					return err
				}
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
		if err = f.r.Recover(f.ctx); err != nil {
			t.Fatal(err)
		}
		f.start()
		f.waitRun(first, protocol.ConversationStatusInterrupted)
		f.waitRun(f.submit("Second"), protocol.ConversationStatusCompleted)
		for _, message := range m.calls[0].Prompt {
			for _, part := range message.Content {
				if part.GetType() == fantasy.ContentTypeToolCall || part.GetType() == fantasy.ContentTypeToolResult {
					t.Fatalf("request carried an unanswered call: %#v", part)
				}
			}
		}
	})
}
func TestCompactionAtThreshold(t *testing.T) {
	configured := uint32(700)
	for _, row := range []struct {
		tokens     uint64
		percent    uint8
		configured *uint32
		want       bool
	}{{800, 80, nil, false}, {801, 80, nil, true}, {0, 80, nil, false}, {1000, 100, nil, false}, {1000, 0, nil, false}, {700, 80, &configured, false}, {701, 80, &configured, true}} {
		t.Run(fmt.Sprintf("%d tokens at %d%% configured %t", row.tokens, row.percent, row.configured != nil), func(t *testing.T) {
			binding := model.Binding{Config: protocol.SessionConfig{Model: "fixture"}, Provider: protocol.ConversationProvider{Models: []protocol.ConversationModel{{ID: "fixture", Context: 1000, CompactionTokens: row.configured}}}}
			usage := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.Assistant), manualNow())
			usage.Message().Usage = &canonical.Usage{Total: &row.tokens}
			input := canonical.NewRecord(usage.Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Hidden pending input")), manualNow())
			input.Metadata.Runtime = &canonical.Runtime{AgentVisible: false}
			// The reported total must belong to the last visible, non-assistant
			// message for the threshold to apply. A hidden pending message after
			// it contributes no tokens and cannot change the exact boundary.
			usage.Message().Role = canonical.User
			state := store.AgentState{Records: []canonical.Record{usage, input}}
			if got := shouldCompact(binding, state, row.percent); got != row.want {
				t.Fatalf("threshold=%v, want %v", got, row.want)
			}
		})
	}
}
func manualNow() time.Time { return time.Date(2026, 11, 10, 0, 0, 0, 0, time.UTC) }
func TestReactiveCompactionAtMostTwo(t *testing.T) {
	overflow := fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.ContextOverflow, Message: "context full"}}
	summary := response(text(`{"user_intent":["Continue task"],"pending_tasks":["Finish"]}`), finish())
	m := &scriptedModel{scripts: []script{response(overflow), summary, response(overflow), summary, response(overflow)}}
	f := newFixture(t, m)
	f.start()
	turn := f.submit("Task")
	f.waitRun(turn, protocol.ConversationStatusFailed)
	if m.count() != 5 {
		t.Fatalf("reactive request count=%d", m.count())
	}
	if n := f.readQuery("SELECT count(*) FROM agent_events WHERE json_extract(body,'$.kind')='compaction'"); n != 2 {
		t.Fatalf("compactions=%d", n)
	}
}
func TestCompactionKeepsRecords(t *testing.T) {
	m := &scriptedModel{scripts: []script{response(text("Original answer"), finish()), response(text(`{"user_intent":["Task"],"files":[{"path":"a.go","summary":"pending"}]}`), finish())}}
	f := newFixture(t, m)
	f.start()
	f.waitRun(f.submit("Original question"), protocol.ConversationStatusCompleted)
	before := f.readQuery("SELECT count(*) FROM agent_events")
	output := f.command("compact_context", &protocol.CommandCompactContextPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision})
	turn := output.Data.(protocol.QueuedTurn).ID
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	state := f.state()
	mustHaveText(t, state.Records, "Original question")
	mustHaveText(t, state.Records, "Original answer")
	if f.readQuery("SELECT count(*) FROM agent_events") <= before {
		t.Fatal("compaction removed immutable records")
	}
	for _, record := range state.Records {
		if record.Kind == "compaction" {
			raw, err := json.Marshal(record)
			if err != nil || !strings.Contains(string(raw), `"boundary"`) {
				t.Fatal("compaction boundary missing")
			}
			return
		}
	}
	t.Fatal("no compaction record")
}

func TestSystemPromptByteStable(t *testing.T) {
	m := &scriptedModel{scripts: []script{response(text("First"), finish()), response(text("Second"), finish())}}
	f := newFixture(t, m)
	f.start()
	f.waitRun(f.submit("One"), protocol.ConversationStatusCompleted)
	f.waitRun(f.submit("Two"), protocol.ConversationStatusCompleted)
	m.mu.Lock()
	defer m.mu.Unlock()
	first := m.calls[0].Prompt[0].Content[0].(fantasy.TextPart).Text
	second := m.calls[1].Prompt[0].Content[0].(fantasy.TextPart).Text
	if first != second {
		t.Fatal("system prompt changed across turns")
	}
	if strings.Contains(first, "Session environment") || strings.Contains(first, "Session title") {
		t.Fatal("dynamic turn state entered the system prompt")
	}
}
