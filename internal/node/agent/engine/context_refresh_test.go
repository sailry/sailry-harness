package engine

import (
	"context"
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestGuidanceRefreshesPlanAndToolDiscoveryWithoutCompaction(t *testing.T) {
	const originalPlan = `{"title":"Coordinate two fixes","steps":[{"description":"Fix the first report","state":"completed"},{"description":"Verify the second report","state":"in_progress"}]}`
	planTool := operations.Alias("progress", "update_plan")
	progress := objectTool(planTool, true, false, func(_ context.Context, request canonical.ToolRequest) ToolResult {
		return ToolResult{Value: map[string]json.RawMessage{"progress": request.Arguments}}
	})
	cold := objectTool("optional_probe", true, true, nil)
	remaining := objectTool("remaining_probe", true, true, nil)
	cold.Deferred, remaining.Deferred = true, true

	type gate struct{ entered, release chan struct{} }
	gates := make(chan gate)
	hold := objectTool("hold_fixture", true, false, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
		g := gate{make(chan struct{}), make(chan struct{})}
		select {
		case gates <- g:
			close(g.entered)
		case <-ctx.Done():
			return ToolResult{Value: nil}
		}
		select {
		case <-g.release:
		case <-ctx.Done():
		}
		return ToolResult{Value: map[string]bool{"done": true}}
	})
	model := &scriptedModel{scripts: []script{
		response(call("plan", planTool, originalPlan), finish()),
		response(call("hold-one", "hold_fixture", `{}`), finish()),
		response(call("discover", "tool_search", `{"query":"select:optional_probe"}`), call("hold-two", "hold_fixture", `{}`), finish()),
		response(call("hold-three", "hold_fixture", `{}`), finish()),
		response(text("Verified"), finish()),
	}}
	fixture := newFixture(t, model, progress, cold, remaining, hold)
	fixture.start()
	turn := fixture.submit("Coordinate both reports")
	for _, guidance := range []string{"Also check the logs", "Keep the earlier result"} {
		var active gate
		select {
		case active = <-gates:
		case <-fixture.ctx.Done():
			t.Fatal("fixture did not reach a guidance boundary")
		}
		<-active.entered
		input := protocol.DefaultConversationInput()
		input.Text = guidance
		fixture.command("guide_turn", &protocol.CommandGuideTurnPayload{Recipient: turn, ExpectedRevision: fixture.session.Revision, Message: input})
		close(active.release)
	}
	select {
	case active := <-gates:
		close(active.release)
	case <-fixture.ctx.Done():
		t.Fatal("fixture did not reach the step without new input")
	}
	fixture.waitRun(turn, protocol.ConversationStatusCompleted)
	if model.count() != 5 {
		t.Fatalf("model requests = %d", model.count())
	}
	for _, record := range fixture.state().Records {
		if record.Kind == "compaction" {
			t.Fatal("fixture must exercise guidance without compaction")
		}
	}
	for _, step := range []struct {
		index    int
		guidance string
		loaded   bool
	}{
		{2, "Also check the logs", false},
		{3, "Keep the earlier result", true},
		{4, "Keep the earlier result", true},
	} {
		request := model.calls[step.index]
		var prompt strings.Builder
		for _, message := range request.Prompt {
			for _, part := range message.Content {
				if value, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
					prompt.WriteString(value.Text)
					prompt.WriteByte('\n')
				}
			}
		}
		_, after, ok := strings.Cut(prompt.String(), step.guidance)
		if !ok || !strings.Contains(after, `<context-section name="plan">`) || !strings.Contains(after, originalPlan) {
			t.Errorf("request %d lacks the verified plan after its guidance", step.index)
		}
		_, catalog, ok := strings.Cut(after, `<context-section name="deferred_tools">`)
		catalog, _, _ = strings.Cut(catalog, "</context-section>")
		if !ok || !strings.Contains(catalog, "remaining_probe") || strings.Contains(catalog, "optional_probe") == step.loaded {
			t.Errorf("request %d has stale deferred tools: %q", step.index, catalog)
		}
		if enabled := slices.ContainsFunc(request.Tools, func(tool fantasy.Tool) bool { return tool.GetName() == "optional_probe" }); enabled != step.loaded {
			t.Errorf("request %d has wrong discovered-tool declaration: %v", step.index, enabled)
		}
		if step.index == 4 && strings.Count(after, `<context-section name="plan">`) != 1 {
			t.Error("unchanged plan was injected again without new input")
		}
	}
}
