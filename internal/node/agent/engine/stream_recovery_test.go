package engine

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestStreamRecoveryClearsDraftWithoutReplayingCompletedTools(t *testing.T) {
	t.Parallel()
	for _, event := range []fantasy.StreamPart{
		text("Discarded text"),
		{Type: fantasy.StreamPartTypeReasoningDelta, ID: "thought", Delta: "Discarded reasoning"},
		{Type: fantasy.StreamPartTypeToolCall, ID: "discarded", ToolCallName: "probe", ToolCallInput: `{}`},
		{Type: fantasy.StreamPartTypeToolInputStart, ID: "discarded", ToolCallName: "probe"},
		{Type: fantasy.StreamPartTypeToolInputDelta, ID: "discarded", Delta: `{"path":`},
	} {
		t.Run(string(event.Type), func(t *testing.T) {
			var executed atomic.Int32
			probe := objectTool("probe", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
				executed.Add(1)
				return ToolResult{Value: map[string]bool{"ok": true}}
			})
			m := &scriptedModel{scripts: []script{
				response(call("committed", "probe", `{}`), finish()),
				response(text("Discarded attempt"), event, fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: io.ErrUnexpectedEOF}),
				response(text("Final answer"), finish()),
			}}
			f := newFixture(t, m, probe)
			reservation := f.r.feed.Reserve(f.session.ID, make(chan struct{}))
			defer reservation.Close()
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Page.Session = f.session.ID
			snapshot.Page.Revision = 1
			stream := reservation.Subscribe(snapshot)
			projection := client.NewConversationProjection(f.node, f.session.ID, 1)
			f.start()
			turn := f.submit("Task")
			var wait clockWait
			select {
			case wait = <-f.r.clock.(*manualClock).waits:
			case <-f.ctx.Done():
				t.Fatal("the interrupted step was not retried")
			}
			var draftID, retryID string
			for retryID == "" {
				update, err := stream.Next(f.ctx)
				if err != nil {
					t.Fatal(err)
				}
				if _, err := projection.Apply(1, update); err != nil {
					t.Fatal(err)
				}
				current := projection.Snapshot()
				for _, draft := range current.Drafts {
					draftID = draft.ID
				}
				for _, entry := range current.Page.Entries {
					for _, part := range entry.Parts {
						if part.Kind != "resource" {
							continue
						}
						raw, err := json.Marshal(part.Data)
						if err != nil {
							t.Fatal(err)
						}
						var resource struct{ Type string }
						if json.Unmarshal(raw, &resource) == nil && resource.Type == "model_retry" {
							retryID = entry.ID
						}
					}
				}
			}
			if current := projection.Snapshot(); draftID == "" || retryID != draftID || len(current.Drafts) != 0 {
				t.Fatalf("retry did not replace the controller draft: draft=%q retry=%q remaining=%+v", draftID, retryID, current.Drafts)
			}
			current := f.r.feed.Reserve(f.session.ID, make(chan struct{}))
			if len(current.Drafts) != 0 {
				t.Fatal("the retry left provisional output in a reconnect snapshot")
			}
			current.Close()
			if executed.Load() != 1 {
				t.Fatalf("failed step executed a tool or lost the previous result: %d", executed.Load())
			}
			close(wait.advance)
			f.waitRun(turn, protocol.ConversationStatusCompleted)
			if executed.Load() != 1 || m.count() != 3 {
				t.Fatalf("completed tool was replayed: executions=%d requests=%d", executed.Load(), m.count())
			}
			m.mu.Lock()
			sameRequest := reflect.DeepEqual(m.calls[1].Prompt, m.calls[2].Prompt)
			m.mu.Unlock()
			if !sameRequest {
				t.Fatal("recovery rewrote the completed step's history")
			}
			state := f.state()
			mustHaveText(t, state.Records, "Final answer")
			requests, responses, retries := 0, 0, 0
			for _, record := range state.Records {
				message := record.Message()
				if message == nil {
					continue
				}
				if execution := record.Metadata.Execution; execution != nil && execution.Retry != nil {
					retries++
					if record.ID != retryID || len(message.Content) != 0 || record.Metadata.Runtime.AgentVisible {
						t.Fatal("retry retained provisional content or lost the draft identity")
					}
				}
				for _, block := range message.Content {
					switch value := block.Data.(type) {
					case *canonical.TextContent:
						if strings.Contains(value.Text, "Discarded") {
							t.Fatal("a failed attempt entered canonical history")
						}
					case *canonical.Thinking:
						t.Fatal("failed reasoning entered canonical history")
					case *canonical.ToolRequest:
						requests++
						if value.ID != "committed" {
							t.Fatal("a failed attempt persisted its tool call")
						}
					case *canonical.ToolResponse:
						responses++
					}
				}
			}
			if requests != 1 || responses != 1 || retries != 1 {
				t.Fatalf("requests=%d responses=%d retries=%d", requests, responses, retries)
			}
		})
	}
}

func TestStreamRecoveryExhaustionKeepsOnlyFinalPartialAndStructuredFailure(t *testing.T) {
	t.Parallel()
	m := &scriptedModel{}
	for index := range model.MainRetries + 1 {
		m.scripts = append(m.scripts, response(text(fmt.Sprintf("Partial attempt %d", index))))
	}
	f := newFixture(t, m)
	f.start()
	turn := f.submit("Task")
	for range model.MainRetries {
		select {
		case wait := <-f.r.clock.(*manualClock).waits:
			close(wait.advance)
		case <-f.ctx.Done():
			t.Fatal("truncated response did not use its bounded retry budget")
		}
	}
	run := f.waitRun(turn, protocol.ConversationStatusFailed)
	if run.Error == nil || run.Error.Details == nil || run.Error.Details.Kind != "model_recovery" || m.count() != model.MainRetries+1 {
		t.Fatalf("truncated recovery lost its terminal outcome: run=%+v requests=%d", run, m.count())
	}
	var texts []string
	errors, retries := 0, 0
	for _, record := range f.state().Records {
		message := record.Message()
		if message == nil || message.Role != canonical.Assistant {
			continue
		}
		if execution := record.Metadata.Execution; execution != nil && execution.Retry != nil {
			retries++
		}
		if message.Error != nil {
			errors++
			if len(message.Content) != 0 || !message.Error.RetryExhausted {
				t.Fatal("terminal failure was duplicated as assistant text or lost exhaustion")
			}
		}
		for _, block := range message.Content {
			if text, ok := block.Data.(*canonical.TextContent); ok {
				texts = append(texts, text.Text)
			}
		}
	}
	if want := []string{fmt.Sprintf("Partial attempt %d", model.MainRetries)}; !reflect.DeepEqual(texts, want) || errors != 1 || retries != model.MainRetries {
		t.Fatalf("terminal history: texts=%v errors=%d retries=%d", texts, errors, retries)
	}
}
