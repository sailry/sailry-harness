package engine

import (
	"context"
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDiscoveryMatchesCapturedLabelsAndCapabilities(t *testing.T) {
	tool := jsonTool("task_records", "Manage calendar entries", `{"type":"object"}`, true, true, nil)
	tool.Deferred = true
	tool.SearchLabels = []string{"Reminders", "待办提醒", "알림"}
	visible := jsonTool("visible_task_records", "Manage calendar entries", `{"type":"object"}`, true, true, nil)
	visible.SearchLabels = tool.SearchLabels
	for _, query := range []string{"待办提醒", "提醒", "先查看待办提醒", "알림", "Reminders", "calendar", "select:task_records,task_records,unavailable_tool,visible_task_records"} {
		t.Run(query, func(t *testing.T) {
			matched := builtinMatches([]Tool{tool, visible}, query, 8)
			if len(matched) != 1 || matched[0].Definition.Name != tool.Definition.Name {
				t.Fatalf("query %q matched %+v", query, matched)
			}
		})
	}
	for _, query := range []string{"", "   ", "please use the tools", "不存在的功能", "select:unavailable_tool"} {
		if matched := builtinMatches([]Tool{tool, visible}, query, 8); len(matched) != 0 {
			t.Fatalf("query %q discovered unrelated tools: %+v", query, matched)
		}
	}
	dbTool := jsonTool("db_query", "Run SQL against a database", `{"type":"object"}`, true, true, nil)
	dbTool.Deferred = true
	if matched := builtinMatches([]Tool{tool, dbTool}, "db", 8); len(matched) != 1 || matched[0].Definition.Name != "db_query" {
		t.Fatalf("2-char query \"db\" matched %+v, want db_query", matched)
	}
}

func TestDiscoveryMergesCapabilityInstructionsAcrossTurns(t *testing.T) {
	for _, hosted := range []bool{false, true} {
		name := "declared instructions"
		if hosted {
			name = "duplicate host declaration stays scoped to the offered tool"
		}
		t.Run(name, func(t *testing.T) {
			alpha := objectTool("alpha", true, true, nil)
			beta := objectTool("beta", true, true, nil)
			alpha.Deferred, beta.Deferred = true, true
			const alphaContext = "Use the alpha capability with its captured workspace."
			const betaContext = "Use the beta capability with its captured account."
			const unavailableContext = "The unavailable capability must never enter this request."
			if hosted {
				alpha.Instruction, beta.Instruction = alphaContext, betaContext
			}
			model := &scriptedModel{scripts: []script{
				response(call("one", "tool_search", `{"query":"select:alpha"}`), call("two", "tool_search", `{"query":"select:beta"}`), call("again", "tool_search", `{"query":"select:alpha"}`), finish()),
				response(text("Capabilities loaded"), finish()),
				response(text("Capabilities retained"), finish()),
			}}
			fixture := newFixture(t, model, alpha, beta)
			fixture.r.toolsForTurn = func(_ context.Context, invocation *store.AgentInvocation) ([]Tool, error) {
				invocation.Instructions = []store.AgentInstruction{
					{Text: alphaContext, AnyTools: []string{"alpha"}},
					{Text: betaContext, AnyTools: []string{"beta"}},
					{Text: unavailableContext, AnyTools: []string{"unavailable"}},
				}
				return nil, nil
			}
			fixture.start()
			fixture.waitRun(fixture.submit("Find both capabilities"), protocol.ConversationStatusCompleted)
			fixture.waitRun(fixture.submit("Keep using them"), protocol.ConversationStatusCompleted)
			if model.count() != 3 {
				t.Fatalf("discovery requests = %d", model.count())
			}
			// Discovery leaves the system prompt and declarations untouched;
			// the instructions arrive once as loaded-tool context.
			for index, request := range model.calls {
				system, history := requestText(request, true), requestText(request, false)
				if system != requestText(model.calls[0], true) || !slices.Equal(declarationBytes(t, request), declarationBytes(t, model.calls[0])) {
					t.Fatalf("request %d changed the cached prefix after discovery", index)
				}
				want := 1
				if index == 0 {
					want = 0
				}
				if strings.Contains(system, alphaContext) || strings.Count(history, alphaContext) != want || strings.Count(history, betaContext) != want || strings.Contains(history, unavailableContext) {
					t.Fatalf("request %d did not carry each admitted capability instruction exactly %d times", index, want)
				}
			}
		})
	}
}

func TestLocalizedDiscoveryLoadsNextRequest(t *testing.T) {
	deferred := objectTool("task_records", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		t.Error("discovery executed the tool")
		return ToolResult{}
	})
	deferred.Deferred = true
	deferred.SearchLabels = []string{"待办提醒"}
	model := &scriptedModel{scripts: []script{
		response(call("discover", "tool_search", `{"query":"待办提醒"}`), finish()),
		response(text("Final answer"), finish()),
	}}
	fixture := newFixture(t, model, deferred)
	fixture.start()
	fixture.waitRun(fixture.submit("Find the task capability"), protocol.ConversationStatusCompleted)
	if len(model.calls) != 2 {
		t.Fatalf("discovery made %d model requests", len(model.calls))
	}
	for index, request := range model.calls {
		if slices.ContainsFunc(request.Tools, func(tool fantasy.Tool) bool { return tool.GetName() == "task_records" }) {
			t.Fatalf("request %d declared the deferred tool", index)
		}
		described := strings.Contains(requestText(request, false), `<loaded-tool name="task_records">`)
		if described != (index == 1) {
			t.Fatalf("loaded-tool context on request %d = %t", index, described)
		}
	}
	if !slices.Contains(fixture.state().LoadedTools, "task_records") {
		t.Fatal("localized discovery did not retain its loaded tool")
	}
}

// TestDiscoveryKeepsTheCachedPrefix proves that loading a tool leaves the
// system prompt and the declared tools byte-identical across the turn and the
// next one, while the loaded tool runs through call_tool under its real name
// and its own approval.
func TestDiscoveryKeepsTheCachedPrefix(t *testing.T) {
	executed := 0
	deferred := objectTool("task_records", false, false, func(_ context.Context, call canonical.ToolRequest) ToolResult {
		executed++
		if call.Name != "task_records" || string(call.Arguments) != `{"title":"Review"}` {
			t.Errorf("dispatched call = %s %s", call.Name, call.Arguments)
		}
		return ToolResult{Value: map[string]any{"saved": true}}
	})
	deferred.Deferred, deferred.Approval = true, true
	deferred.Instruction = "Record tasks in the captured calendar."
	model := &scriptedModel{scripts: []script{
		response(call("discover", "tool_search", `{"query":"select:task_records"}`), finish()),
		response(call("save", callToolName, `{"name":"task_records","arguments":{"title":"Review"}}`), finish()),
		response(call("missing", callToolName, `{"name":"absent_tool","arguments":{}}`), finish()),
		response(text("Saved"), finish()),
		response(text("Still loaded"), finish()),
	}}
	fixture := newFixture(t, model, deferred)
	fixture.start()
	turn := fixture.submit("Record a review task")
	approval := fixture.waitingApproval()
	approved := false
	for _, record := range fixture.state().Records {
		if message := record.Message(); record.ID == approval.Entry && message != nil && approval.Index < uint64(len(message.Content)) {
			request, ok := message.Content[approval.Index].Data.(*canonical.ToolRequest)
			approved = ok && request.Name == "task_records" && request.ID == "save"
		}
	}
	if !approved {
		t.Fatalf("approval %+v does not name the dispatched tool's call", approval)
	}
	fixture.command("resolve_approval", &protocol.CommandResolveApprovalPayload{Session: fixture.session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}})
	fixture.waitRun(turn, protocol.ConversationStatusCompleted)
	fixture.waitRun(fixture.submit("Anything else?"), protocol.ConversationStatusCompleted)
	if executed != 1 || model.count() != 5 {
		t.Fatalf("executions = %d, requests = %d", executed, model.count())
	}
	first := model.calls[0]
	for index, request := range model.calls {
		if requestText(request, true) != requestText(first, true) || !slices.Equal(declarationBytes(t, request), declarationBytes(t, first)) {
			t.Fatalf("request %d changed the system prompt or tool declarations", index)
		}
	}
	if !slices.ContainsFunc(first.Tools, func(tool fantasy.Tool) bool { return tool.GetName() == callToolName }) {
		t.Fatal("call_tool is not declared from the first request")
	}
	if history := requestText(model.calls[4], false); strings.Count(history, `<loaded-tool name="task_records">`) != 1 || strings.Count(history, deferred.Instruction) != 1 {
		t.Fatalf("loaded-tool context was not carried exactly once:\n%s", history)
	}
	// The model sees its own call_tool call replayed; history keeps the tool.
	replayed := false
	for _, message := range model.calls[2].Prompt {
		for _, part := range message.Content {
			if value, ok := fantasy.AsMessagePart[fantasy.ToolCallPart](part); ok && value.ToolCallID == "save" {
				replayed = value.ToolName == callToolName && value.Input == `{"name":"task_records","arguments":{"title":"Review"}}`
			}
		}
	}
	if !replayed {
		t.Fatal("the dispatched call was not replayed as the model sent it")
	}
	var names []string
	for _, record := range fixture.state().Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				switch value := block.Data.(type) {
				case *canonical.ToolRequest:
					names = append(names, "request:"+value.Name)
				case *canonical.ToolResponse:
					names = append(names, "response:"+value.Name)
				}
			}
		}
	}
	if want := []string{"request:tool_search", "response:tool_search", "request:task_records", "response:task_records", "request:" + callToolName, "response:" + callToolName}; !slices.Equal(names, want) {
		t.Fatalf("recorded tool names = %v", names)
	}
	if !strings.Contains(model.calls[3].Prompt[len(model.calls[3].Prompt)-1].Content[0].(fantasy.ToolResultPart).Output.(fantasy.ToolResultOutputContentText).Text, "absent_tool") {
		t.Fatal("an unknown dispatched tool was not reported to the model")
	}
}

// requestText joins the text of a request's system messages or of the rest
// of its prompt.
func requestText(request fantasy.Call, system bool) string {
	var out strings.Builder
	for _, message := range request.Prompt {
		if (message.Role == fantasy.MessageRoleSystem) != system {
			continue
		}
		for _, part := range message.Content {
			if value, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
				out.WriteString(value.Text + "\n")
			}
		}
	}
	return out.String()
}

func declarationBytes(t *testing.T, request fantasy.Call) []byte {
	t.Helper()
	encoded, err := json.Marshal(request.Tools)
	if err != nil {
		t.Fatal(err)
	}
	return encoded
}
