package engine

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSubagentListingTool(t *testing.T) {
	t.Parallel()
	f, x := delegationFixture(t)
	if err := f.r.captureTools(x); err != nil {
		t.Fatal(err)
	}
	tool := findTool(x.tools, "list_subagents")
	if tool == nil || !tool.ReadOnly || !tool.Concurrent || !tool.Deferred || tool.Approval || tool.ExecutePrepared != nil {
		t.Fatal("listing must remain a deferred read-only native tool in Standard mode")
	}
	for _, query := range []string{"子智能体", "子智能體", "子代理", "查看", "续接", "恢复", "中断后查看子智能体状态", "select:list_subagents"} {
		match := builtinMatches(x.tools, query, 1)
		if len(match) != 1 || match[0].Definition.Name != "list_subagents" {
			t.Fatalf("query %q did not find the subagent list", query)
		}
	}
	for _, invalid := range []string{`{"session":"external"}`, `{"limit":0}`, `{"limit":51}`, `{"before":100}`, `{"before":"0"}`} {
		var args any
		if err := json.Unmarshal([]byte(invalid), &args); err != nil {
			t.Fatal(err)
		}
		if err := tool.schema.Validate(args); err == nil {
			t.Errorf("schema accepted %s", invalid)
		}
	}
	call := canonical.ToolRequest{ID: "list", Name: "list_subagents", Arguments: json.RawMessage(`{}`)}
	result := tool.Execute(x.ctx, call)
	if page, ok := result.Value.(store.SubagentPage); !ok || result.Failure != nil || page.Subagents == nil || len(page.Subagents) != 0 || page.NextBefore != nil || result.Collect != nil {
		t.Fatalf("empty list did not return a read-only page: %+v", result)
	}
	record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &call}), f.r.clock.Now())
	f.r.captureCorePresentation(&record)
	presentation := record.Metadata.Presentation
	if presentation == nil || presentation.Tools[call.Name] != protocol.ToolPresentationSummary || presentation.Displays[call.Name].Locales["zh-CN"] != "查看子智能体" || len(presentation.Delegations) != 0 {
		t.Fatalf("subagent list presentation was missing or treated as spawning: %+v", presentation)
	}

	x.inv.Turn.Config.Assistant = &protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "assistant"}}
	x.inv.Assistant = &protocol.PluginConversationDeclaration{Tools: []protocol.PluginConversationTool{
		{Kind: "builtin", Data: &protocol.PluginConversationToolBuiltinPayload{Name: "list_subagents"}},
	}}
	if err := f.r.captureTools(x); err != nil {
		t.Fatal(err)
	}
	if preferred := findTool(x.tools, "list_subagents"); preferred == nil || preferred.Deferred {
		t.Fatal("a captured assistant could not prioritize the native list")
	}
}
