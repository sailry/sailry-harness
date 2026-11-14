package engine

import (
	"context"
	"slices"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
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
		offered := slices.ContainsFunc(request.Tools, func(tool fantasy.Tool) bool { return tool.GetName() == "task_records" })
		if offered != (index == 1) {
			t.Fatalf("tool visibility on request %d = %t", index, offered)
		}
	}
	if !slices.Contains(fixture.state().LoadedTools, "task_records") {
		t.Fatal("localized discovery did not retain its loaded tool")
	}
}
