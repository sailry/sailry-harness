package engine

import (
	"context"
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestReactiveCompactionRediscoversUnloadedTool(t *testing.T) {
	const target = "read_session"
	const plan = `{"title":"Lookup","steps":[{"description":"Verify the lookup","state":"in_progress"}]}`
	candidates := func(request fantasy.Call) []string {
		const section = "<context-section name=\"deferred_tools\">\n"
		prompt := promptText(request)
		start := strings.LastIndex(prompt, section)
		if start < 0 {
			return nil
		}
		body, _, closed := strings.Cut(prompt[start+len(section):], "\n</context-section>")
		names, ok := strings.CutPrefix(body, "Deferred tools\n")
		if !closed || !ok {
			return nil
		}
		line, _, _ := strings.Cut(names, "\n")
		return strings.Split(line, ", ")
	}
	search := func(id, name string) script {
		return response(call(id, "tool_search", `{"query":"select:`+name+`"}`), finish())
	}
	rediscover := func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
		offered, deferred := offeredNames(request), candidates(request)
		if !slices.Contains(offered, target) && !slices.Contains(offered, "update_plan") && slices.Contains(offered, "tool_search") &&
			slices.Contains(deferred, target) && slices.Contains(deferred, "update_plan") &&
			strings.Contains(promptText(request), "using select:update_plan") && strings.Contains(promptText(request), plan) {
			search("reload", target+",update_plan")(ctx, request, yield)
			return
		}
		t.Errorf("request after compaction did not restore the plan and deferred tools: declarations = %v, candidates = %v", offered, deferred)
		response(text("Deferred tool unavailable"), finish())(ctx, request, yield)
	}
	overflow := fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.ContextOverflow, Message: "context full"}}
	m := &scriptedModel{scripts: []script{
		search("initial-load", target+",update_plan"),
		response(call("plan", "update_plan", plan), finish()),
		response(text("Lookup set up"), finish()),
		response(overflow),
		response(text(`{"user_intent":["Resume the lookup"]}`), finish()),
		rediscover,
		response(text("Lookup ready"), finish()),
	}}
	progress := packageTool("progress", "update_plan", "", false, func(_ context.Context, call canonical.ToolRequest) ToolResult {
		return ToolResult{Value: map[string]json.RawMessage{"progress": call.Arguments}}
	})
	f := newFixture(t, m, progress)
	f.start()
	f.waitRun(f.submit("Set up the lookup"), protocol.ConversationStatusCompleted)
	if count, loaded := m.count(), f.state().LoadedTools; count != 3 || !slices.Contains(loaded, target) || !slices.Contains(loaded, progressToolName) {
		t.Fatalf("before the overflow turn: requests = %d, loaded tools = %v", count, loaded)
	}

	turn := f.submit("Continue the lookup")
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if count := m.count(); count != len(m.scripts) {
		t.Fatalf("requests = %d, want %d including compaction and rediscovery", count, len(m.scripts))
	}
	if offered, deferred := offeredNames(m.calls[3]), candidates(m.calls[3]); !slices.Contains(offered, target) || slices.Contains(deferred, target) {
		t.Errorf("request before overflow: want %q declared but not deferred; declarations = %v, candidates = %v", target, offered, deferred)
	}
	if offered := offeredNames(m.calls[4]); len(offered) != 0 {
		t.Errorf("summary request offered tools: %v", offered)
	}
	if offered := offeredNames(m.calls[6]); !slices.Contains(offered, target) || !slices.Contains(offered, "update_plan") {
		t.Errorf("request after rediscovery lacks the requested tools: %v", offered)
	}

	state := f.state()
	compactions := 0
	for _, record := range state.Records {
		if record.Kind != "compaction" {
			continue
		}
		compactions++
		delta := record.Metadata.Context
		if record.Turn != turn || delta == nil || delta.LoadedTools == nil || slices.Contains(*delta.LoadedTools, target) || slices.Contains(*delta.LoadedTools, progressToolName) {
			t.Errorf("compaction did not unload the discovered tools in the overflow turn: %+v", delta)
		}
	}
	if compactions != 1 || !slices.Contains(state.LoadedTools, target) {
		t.Errorf("compactions = %d, loaded tools after rediscovery = %v", compactions, state.LoadedTools)
	}
}
