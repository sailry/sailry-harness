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

func TestResponsesFailureRebuildsHistoryWithoutAResponseChain(t *testing.T) {
	t.Parallel()
	var effects, continuations atomic.Int32
	var mu sync.Mutex
	var failedRequests [][]byte
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Method != http.MethodPost || request.URL.Path != "/v1/responses" {
			writer.WriteHeader(http.StatusNotFound)
			return
		}
		raw, err := io.ReadAll(request.Body)
		if err != nil {
			t.Error(err)
			return
		}
		var body map[string]any
		if err := json.Unmarshal(raw, &body); err != nil {
			t.Error(err)
			return
		}
		if body["previous_response_id"] != nil || body["conversation"] != nil || body["store"] != false {
			t.Error("request reused server-side response state")
		}
		pending := map[string]bool{}
		calls, results := 0, 0
		for _, raw := range body["input"].([]any) {
			item := raw.(map[string]any)
			id, _ := item["call_id"].(string)
			switch item["type"] {
			case "function_call":
				calls++
				if id != "committed" || pending[id] {
					t.Error("unfinished or duplicate call escaped into the next request")
				}
				pending[id] = true
			case "function_call_output":
				results++
				if !pending[id] {
					t.Error("tool output has no matching call")
				}
				delete(pending, id)
			}
		}
		if len(pending) != 0 || calls != results {
			t.Error("request retained an unanswered call")
		}
		writer.Header().Set("Content-Type", "text/event-stream")
		send := func(value string) { _, _ = fmt.Fprintf(writer, "data: %s\n\n", value) }
		if strings.Contains(string(raw), "Independent Responses request") || strings.Contains(string(raw), "Same-session continuation") {
			if strings.Contains(string(raw), "Independent Responses request") && (calls != 0 || strings.Contains(string(raw), "Retained partial")) {
				t.Error("independent session inherited failed history")
			}
			if strings.Contains(string(raw), "Same-session continuation") && (calls != 1 || results != 1 || !strings.Contains(string(raw), "Retained partial")) {
				t.Error("same-session continuation lost completed tools or its partial answer")
			}
			continuations.Add(1)
			send(`{"type":"response.completed","response":{"id":"continued-response","object":"response","model":"fixture","created_at":1,"status":"completed","output":[{"type":"message","id":"answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Continued safely","annotations":[]}]}],"usage":{"input_tokens":12,"output_tokens":3}}}`)
			return
		}
		if calls == 0 {
			send(`{"type":"response.completed","response":{"id":"completed-step","object":"response","model":"fixture","created_at":1,"status":"completed","output":[{"type":"function_call","id":"tool-item","call_id":"committed","name":"probe","arguments":"{}","status":"completed"}],"usage":{"input_tokens":12,"output_tokens":3}}}`)
			return
		}
		mu.Lock()
		failedRequests = append(failedRequests, raw)
		mu.Unlock()
		send(`{"type":"response.created","response":{"id":"failed-response","object":"response","model":"fixture","created_at":1,"status":"in_progress","output":[]}}`)
		send(`{"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"reasoning-item","encrypted_content":"opaque-replay-state","summary":[{"type":"summary_text","text":"Retained reasoning"}]}}`)
		send(`{"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"unfinished-item","call_id":"unfinished-call","name":"probe","arguments":""}}`)
		send(`{"type":"response.function_call_arguments.delta","item_id":"unfinished-item","delta":"{"}`)
		send(`{"type":"response.output_text.delta","item_id":"partial-answer","delta":"Retained partial"}`)
		send(`{"type":"response.failed","response":{"id":"failed-response","object":"response","model":"fixture","created_at":1,"status":"failed","error":{"code":"upstream_bad_gateway","message":"Upstream 502 interrupted response"}}}`)
	}))
	t.Cleanup(server.Close)
	probe := objectTool("probe", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		effects.Add(1)
		return ToolResult{Value: map[string]bool{"saved": true}}
	})
	f := newFixture(t, nil, probe)
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(f.ctx, `UPDATE providers SET body=json_set(body,'$.provider.api','responses','$.provider.endpoint',?1)`, server.URL+"/v1")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	f.r.model = model.New
	f.start()
	first := f.submit("Complete the operation once")
	for range model.MainRetries {
		select {
		case wait := <-f.r.clock.(*manualClock).waits:
			close(wait.advance)
		case <-f.ctx.Done():
			t.Fatal("failed Responses stream did not enter bounded retry")
		}
	}
	run := f.waitRun(first, protocol.ConversationStatusFailed)
	if run.Error == nil || !strings.Contains(run.Error.Message, "Upstream 502 interrupted response") {
		t.Fatalf("upstream failure was hidden: %+v", run)
	}
	mu.Lock()
	if len(failedRequests) != model.MainRetries+1 {
		t.Errorf("failed requests=%d", len(failedRequests))
	}
	for _, request := range failedRequests[1:] {
		if !reflect.DeepEqual(request, failedRequests[0]) {
			t.Error("Responses retry changed its request history")
		}
	}
	mu.Unlock()
	before := f.state().Records
	mustHaveText(t, before, "Retained partial")
	f.waitRun(f.submit("Same-session continuation"), protocol.ConversationStatusCompleted)
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
	input.Text = "Independent Responses request"
	other := f.command("submit_turn", &protocol.CommandSubmitTurnPayload{Session: independent.ID, ExpectedRevision: independent.Revision, Message: input}).Data.(protocol.QueuedTurn).ID
	f.waitRun(other, protocol.ConversationStatusCompleted)
	if effects.Load() != 1 || continuations.Load() != 2 {
		t.Fatalf("continuation repeated effects or lost a session: effects=%d continuations=%d", effects.Load(), continuations.Load())
	}
	after := f.state().Records
	if len(after) < len(before) || !reflect.DeepEqual(after[:len(before)], before) {
		t.Fatal("continuation changed the existing canonical history")
	}
}
