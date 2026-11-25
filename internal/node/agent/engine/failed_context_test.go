package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestIncompleteThinkingDoesNotPoisonLaterTurns(t *testing.T) {
	t.Parallel()
	for _, stopped := range []bool{false, true} {
		name := "failed"
		if stopped {
			name = "stopped"
		}
		t.Run(name, func(t *testing.T) {
			const partial = "Unfinished private reasoning"
			var effects, failures, poisoned, outages atomic.Int32
			var mu sync.Mutex
			var failedRequests [][]byte
			server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				if request.Method != http.MethodPost || request.URL.Path != "/v1/messages" {
					writer.WriteHeader(http.StatusNotFound)
					return
				}
				raw, err := io.ReadAll(request.Body)
				if err != nil {
					t.Error(err)
					return
				}
				var body struct {
					Messages []struct {
						Role    string           `json:"role"`
						Content []map[string]any `json:"content"`
					} `json:"messages"`
				}
				if err := json.Unmarshal(raw, &body); err != nil {
					t.Error(err)
					writer.WriteHeader(http.StatusBadRequest)
					return
				}
				if strings.Contains(string(raw), "Ordinary 502 request") {
					outages.Add(1)
					writer.Header().Set("Content-Type", "application/json")
					writer.WriteHeader(http.StatusBadGateway)
					_, _ = io.WriteString(writer, `{"error":{"type":"upstream_bad_gateway","message":"Ordinary upstream outage"}}`)
					return
				}
				if strings.Contains(string(raw), "Independent request") && (strings.Contains(string(raw), "committed") || strings.Contains(string(raw), partial)) {
					t.Error("independent session inherited another session's history")
				}
				for _, message := range body.Messages {
					if message.Role == "assistant" && len(message.Content) == 0 {
						poisoned.Add(1)
						writer.Header().Set("Content-Type", "application/json")
						writer.WriteHeader(http.StatusBadGateway)
						_, _ = io.WriteString(writer, `{"error":{"type":"upstream_bad_gateway","message":"Upstream rejected empty assistant content"}}`)
						return
					}
				}
				writer.Header().Set("Content-Type", "text/event-stream")
				send := func(kind, value string) {
					_, _ = fmt.Fprintf(writer, "event: %s\ndata: %s\n\n", kind, value)
				}
				send("message_start", `{"type":"message_start","message":{"id":"message","type":"message","role":"assistant","model":"fixture","content":[],"usage":{"input_tokens":12,"output_tokens":0}}}`)
				if strings.Contains(string(raw), "Independent request") || strings.Contains(string(raw), "Follow-up request") {
					send("content_block_start", `{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Continued safely"}}`)
					send("content_block_stop", `{"type":"content_block_stop","index":0}`)
					send("message_delta", `{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}`)
					send("message_stop", `{"type":"message_stop"}`)
					return
				}
				if !strings.Contains(string(raw), `"tool_result"`) {
					send("content_block_start", `{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"committed","name":"probe","input":{}}}`)
					send("content_block_stop", `{"type":"content_block_stop","index":0}`)
					send("message_delta", `{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}`)
					send("message_stop", `{"type":"message_stop"}`)
					return
				}
				mu.Lock()
				failedRequests = append(failedRequests, raw)
				mu.Unlock()
				failures.Add(1)
				send("content_block_start", `{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}`)
				send("content_block_delta", `{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"`+partial+`"}}`)
				if stopped {
					writer.(http.Flusher).Flush()
					<-request.Context().Done()
					return
				}
				send("content_block_start", `{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"discarded","name":"probe","input":{}}}`)
				send("content_block_delta", `{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"unfinished\":"}}`)
				send("error", `{"type":"error","error":{"type":"upstream_bad_gateway","message":"Upstream 502 interrupted reasoning"}}`)
			}))
			t.Cleanup(server.Close)
			probe := objectTool("probe", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
				effects.Add(1)
				return ToolResult{Value: map[string]bool{"saved": true}}
			})
			f := newFixture(t, nil, probe)
			if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				_, err := db.ExecContext(f.ctx, `UPDATE providers SET body=json_set(body,'$.provider.api','anthropic','$.provider.endpoint',?1)`, server.URL+"/v1")
				return err
			}); err != nil {
				t.Fatal(err)
			}
			f.r.model = model.New
			reservation := f.r.feed.Reserve(f.session.ID, make(chan struct{}))
			defer reservation.Close()
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Page.Session, snapshot.Page.Revision = f.session.ID, 1
			stream := reservation.Subscribe(snapshot)
			advanceDone := make(chan struct{})
			defer close(advanceDone)
			go func() {
				for {
					select {
					case wait := <-f.r.clock.(*manualClock).waits:
						close(wait.advance)
					case <-advanceDone:
						return
					case <-f.ctx.Done():
						return
					}
				}
			}()
			f.start()
			first := f.submit("Complete the operation once")
			finish := canonical.Failed
			if stopped {
				for {
					update, err := stream.Next(f.ctx)
					if err != nil {
						t.Fatal(err)
					}
					frame, ok := update.Data.(*protocol.ConversationFrame)
					if !ok || frame.Change.Kind != "delta" {
						continue
					}
					raw, _ := json.Marshal(frame.Change.Data)
					if strings.Contains(string(raw), partial) {
						break
					}
				}
				f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: first})
				f.waitRun(first, protocol.ConversationStatusCancelled)
				finish = canonical.Interrupted
			} else {
				run := f.waitRun(first, protocol.ConversationStatusFailed)
				if run.Error == nil || !strings.Contains(run.Error.Message, "Upstream 502 interrupted reasoning") || failures.Load() != model.MainRetries+1 {
					t.Fatalf("upstream failure was hidden or changed its retry budget: run=%+v attempts=%d", run, failures.Load())
				}
				mu.Lock()
				for _, request := range failedRequests[1:] {
					if !reflect.DeepEqual(request, failedRequests[0]) {
						t.Error("retry changed the completed tool history")
					}
				}
				mu.Unlock()
			}
			var retained canonical.Record
			for _, record := range f.state().Records {
				if message := record.Message(); record.Turn == first && message != nil {
					for _, block := range message.Content {
						if thinking, ok := block.Data.(*canonical.Thinking); ok && thinking.Text == partial {
							if message.Finish == nil || *message.Finish != finish || thinking.Signature != nil {
								t.Fatalf("unexpected partial response: %+v", message)
							}
							retained = record
						}
					}
				}
			}
			if retained.ID == "" {
				t.Fatal("failure discarded the canonical partial reasoning")
			}
			requests, results := 0, 0
			for _, record := range f.state().Records {
				if message := record.Message(); message != nil {
					for _, block := range message.Content {
						switch value := block.Data.(type) {
						case *canonical.ToolRequest:
							requests++
							if value.ID != "committed" {
								t.Fatal("failed response committed an unfinished tool call")
							}
						case *canonical.ToolResponse:
							results++
							if value.ID != "committed" {
								t.Fatal("completed tool result no longer matches its call")
							}
						}
					}
				}
			}
			if requests != 1 || results != 1 {
				t.Fatalf("completed history lost its tool pair: requests=%d results=%d", requests, results)
			}
			var independent protocol.Session
			if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				request := protocol.NewRequest(f.node, protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &f.session.Config}})
				output, _, err := store.ExecuteCommand(f.ctx, db, f.node, f.node, f.r.profile, request, store.PreparedPaths{})
				if err == nil {
					independent = output.Data.(protocol.Session)
				}
				return err
			}); err != nil {
				t.Fatal(err)
			}
			input := protocol.DefaultConversationInput()
			input.Text = "Independent request"
			other := f.command("submit_turn", &protocol.CommandSubmitTurnPayload{Session: independent.ID, ExpectedRevision: independent.Revision, Message: input}).Data.(protocol.QueuedTurn).ID
			f.waitRun(other, protocol.ConversationStatusCompleted)
			f.waitRun(f.submit("Follow-up request"), protocol.ConversationStatusCompleted)
			if effects.Load() != 1 || poisoned.Load() != 0 {
				t.Fatalf("follow-up replayed effects or sent an empty assistant: effects=%d invalid_requests=%d", effects.Load(), poisoned.Load())
			}
			found := false
			for _, record := range f.state().Records {
				if record.ID == retained.ID {
					found = true
					if !reflect.DeepEqual(record, retained) {
						t.Fatal("follow-up mutated the retained canonical partial response")
					}
				}
			}
			if !found {
				t.Fatal("follow-up erased the failed response")
			}
			outage := f.waitRun(f.submit("Ordinary 502 request"), protocol.ConversationStatusFailed)
			if outage.Error == nil || outage.Error.Message != "Ordinary upstream outage (HTTP 502)" || outages.Load() != model.MainRetries+1 || effects.Load() != 1 {
				t.Fatalf("ordinary 502 was hidden or replayed an effect: run=%+v attempts=%d effects=%d", outage, outages.Load(), effects.Load())
			}
		})
	}
}
