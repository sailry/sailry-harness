package engine

import (
	"context"
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
}

func TestDiscoveryMergesCapabilityInstructionsAcrossTurns(t *testing.T) {
	for _, hosted := range []bool{false, true} {
		name := "declared instructions"
		if hosted {
			name = "instructions already supplied by the host"
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
			for index, request := range model.calls {
				var system strings.Builder
				for _, message := range request.Prompt {
					if message.Role != fantasy.MessageRoleSystem {
						continue
					}
					for _, part := range message.Content {
						if value, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
							system.WriteString(value.Text)
						}
					}
				}
				want := 1
				if index == 0 && !hosted {
					want = 0
				}
				if strings.Count(system.String(), alphaContext) != want || strings.Count(system.String(), betaContext) != want || strings.Contains(system.String(), unavailableContext) {
					t.Fatalf("request %d did not preserve each admitted capability instruction exactly %d times", index, want)
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
		offered := slices.ContainsFunc(request.Tools, func(tool fantasy.Tool) bool { return tool.GetName() == "task_records" })
		if offered != (index == 1) {
			t.Fatalf("tool visibility on request %d = %t", index, offered)
		}
	}
	if !slices.Contains(fixture.state().LoadedTools, "task_records") {
		t.Fatal("localized discovery did not retain its loaded tool")
	}
}
