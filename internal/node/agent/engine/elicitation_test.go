package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"sync/atomic"
	"testing"

	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMCPElicitationLifecycle(t *testing.T) {
	for _, mode := range []string{"form", "url", "cancel"} {
		t.Run(mode, func(t *testing.T) {
			var fixture *engineFixture
			results := make(chan *sdk.ElicitResult, 1)
			tool := objectTool("remote", true, false, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
				parameters := &sdk.ElicitParams{Mode: "form", Message: "Choose", RequestedSchema: map[string]any{
					"type": "object", "properties": map[string]any{"name": map[string]any{"type": "string", "minLength": 2}}, "required": []any{"name"},
				}}
				if mode == "url" {
					parameters = &sdk.ElicitParams{Mode: "url", Message: "Authorize", URL: "https://example.test/authorize", ElicitationID: "authorization"}
				}
				result, err := fixture.r.Elicit(ctx, fixture.currentTurn(), call, &sdk.ElicitRequest{Params: parameters})
				if err != nil {
					return toolFault(err)
				}
				results <- result
				return ToolResult{Value: map[string]any{"action": result.Action}}
			})
			model := &scriptedModel{scripts: []script{response(call("parent", "remote", `{}`), finish()), response(text("Done"), finish())}}
			fixture = newFixture(t, model, tool)
			fixture.start()
			turn := fixture.submit("Perform the remote task")
			question := fixture.waitingQuestion()
			state := fixture.state()
			var input *canonical.Elicitation
			for _, record := range state.Records {
				if notice, ok := record.Data.(*canonical.Notice); ok && notice.Kind == "elicitation" {
					input, _ = notice.Data.(*canonical.Elicitation)
				}
			}
			if input == nil || input.Parent.ID == nil || *input.Parent.ID != "parent" || input.Spec.Input.Kind != map[bool]string{true: "url", false: "form"}[mode == "url"] {
				t.Fatal("MCP input lost its exact canonical parent")
			}
			reply := protocol.ConversationQuestionResponse{Kind: "cancel"}
			if mode == "form" {
				values := map[string]json.RawMessage{"name": json.RawMessage(`"Accepted"`)}
				reply = protocol.ConversationQuestionResponse{Kind: "answer", Data: &protocol.ConversationQuestionAnswer{Kind: "form", Data: &values}}
			} else if mode == "url" {
				reply = protocol.ConversationQuestionResponse{Kind: "answer", Data: &protocol.ConversationQuestionAnswer{Kind: "opened"}}
			}
			fixture.command("resolve_question", &protocol.CommandResolveQuestionPayload{Session: fixture.session.ID, Question: question.ID, Response: reply})
			fixture.waitRun(turn, protocol.ConversationStatusCompleted)
			result := <-results
			if mode == "cancel" && result.Action != "cancel" || mode != "cancel" && result.Action != "accept" {
				t.Fatalf("MCP result = %+v", result)
			}
			canonicalResults := 0
			for _, record := range fixture.state().Records {
				if notice, ok := record.Data.(*canonical.Notice); ok && notice.Kind == "elicitation_result" {
					canonicalResults++
				}
				if message := record.Message(); message != nil && message.Role == canonical.Assistant {
					for _, block := range message.Content {
						if call, ok := block.Data.(*canonical.ToolRequest); ok && call.Name == "mcp_elicitation" {
							t.Fatal("MCP input entered a provider tool batch")
						}
					}
				}
			}
			if canonicalResults != 1 {
				t.Fatalf("canonical MCP results = %d", canonicalResults)
			}
		})
	}
	t.Run("cleanup failure settles as uncertain before completion", func(t *testing.T) {
		var closed atomic.Int32
		tool := objectTool("remote", true, false, func(context.Context, canonical.ToolRequest) ToolResult {
			return ToolResult{Value: map[string]bool{"ok": true}}
		})
		tool.Close = func() error { closed.Add(1); return errors.New("fixture cleanup failure") }
		model := &scriptedModel{scripts: []script{response(text("Done"), finish())}}
		fixture := newFixture(t, model, tool)
		fixture.start()
		turn := fixture.submit("Task")
		for {
			var run protocol.ConversationRun
			if err := fixture.db.Node.Read(fixture.ctx, func(db *sql.Conn) error {
				var err error
				run, err = store.ReadAgentRun(fixture.ctx, db, turn)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if run.Status == protocol.ConversationStatusFailed {
				if run.Error == nil || run.Error.Code != protocol.ErrorCodeOutcomeUnknown || closed.Load() != 1 {
					t.Fatalf("cleanup settlement = %+v, closes = %d", run, closed.Load())
				}
				break
			}
			select {
			case <-fixture.changed:
			case <-fixture.ctx.Done():
				t.Fatal("cleanup failure was not settled")
			}
		}
	})
}

func (fixture *engineFixture) currentTurn() protocol.TurnID {
	fixture.r.mu.Lock()
	defer fixture.r.mu.Unlock()
	for turn := range fixture.r.active {
		return turn
	}
	return protocol.TurnID{}
}
