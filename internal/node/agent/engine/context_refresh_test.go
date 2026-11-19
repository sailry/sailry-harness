package engine

import (
	"context"
	"database/sql"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestToollessModelClearsPluginCapabilityContext(t *testing.T) {
	const (
		enabledInstruction  = "Active goal: call update_goal after verification."
		disabledInstruction = "New active goal state: call update_goal."
		disabledPart        = "New owned context for a tool-capable request."
		capabilityPolicy    = "Use read_goal when inspecting progress."
		generalInstruction  = "Keep the reply brief."
		laterInput          = "Continue with the text-only model"
	)
	model := &scriptedModel{scripts: []script{response(text("First reply"), finish()), response(text("Second reply"), finish())}}
	f := newFixture(t, model)
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(f.ctx, `UPDATE providers SET body=json_insert(body,'$.provider.models[#]',
			json_set(json_extract(body,'$.provider.models[0]'),'$.id','fixture-text','$.tools',json('false')))`)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	f.r.toolsForTurn = func(_ context.Context, invocation *store.AgentInvocation) ([]Tool, error) {
		read := packageTool("goals", "read_goal", "", true, nil)
		read.Instruction = capabilityPolicy
		update := packageTool("goals", "update_goal", "", false, nil)
		update.TurnInstruction = prompt.Part{Key: "plugin:goals", Text: enabledInstruction}
		if invocation.Turn.Config.Model == "fixture-text" {
			update.TurnInstruction.Text = disabledInstruction
			update.ContextParts = []string{disabledPart}
		}
		invocation.Instructions = []store.AgentInstruction{{Text: generalInstruction}}
		return []Tool{read, update}, nil
	}
	f.start()
	f.waitRun(f.submit("Start the goal"), protocol.ConversationStatusCompleted)
	config := f.session.Config
	config.Model = "fixture-text"
	f.session = f.command("set_session_config", &protocol.CommandSetSessionConfigPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision, Config: config}).Data.(protocol.Session)
	f.waitRun(f.submit(laterInput), protocol.ConversationStatusCompleted)

	first, second := model.calls[0], model.calls[1]
	if !strings.Contains(promptText(first), enabledInstruction) || !strings.Contains(promptText(first), capabilityPolicy) ||
		slices.Contains(offeredNames(first), "update_goal") {
		t.Fatal("tool-capable parent lost the initial policy or deferred goal instruction")
	}
	requestText := promptText(second)
	if len(second.Tools) != 0 || strings.Contains(requestText, capabilityPolicy) || strings.Contains(requestText, disabledInstruction) ||
		strings.Contains(requestText, disabledPart) || !strings.Contains(requestText, generalInstruction) {
		t.Fatalf("text-only model received tool capabilities or lost general instructions: %q", requestText)
	}
	_, tail, found := strings.Cut(requestText, laterInput)
	if !found || !strings.Contains(tail, "<context-section name=\"plugin:goals\">\nCleared;") || strings.Contains(tail, enabledInstruction) {
		t.Fatal("text-only model did not clear the previous goal tool instruction")
	}
}

func TestCollaborateOffersNativeProgressTool(t *testing.T) {
	model := &scriptedModel{scripts: []script{response(text("Completed"), finish())}}
	f := newFixture(t, model)
	config := f.session.Config
	config.Mode = protocol.WorkModeFlagship
	input := protocol.DefaultConversationStart()
	input.Config, input.Execution, input.Message.Text = &config, protocol.ConversationExecutionModel, "Coordinate the reports"
	turn := f.command("start_session", &input).Data.(protocol.QueuedTurn)
	f.start()
	f.waitRun(turn.ID, protocol.ConversationStatusCompleted)
	request := model.calls[0]
	if !slices.Contains(offeredNames(request), operations.UpdatePlanTool) || !strings.Contains(promptText(request), "Use update_plan to record a plan") {
		t.Fatal("Collaboration required discovery for its native progress tool")
	}
}

func TestGuidanceRefreshesPlanAndToolDiscoveryWithoutCompaction(t *testing.T) {
	const originalPlan = `{"title":"Coordinate two fixes","steps":[{"description":"Fix the first report","state":"completed"},{"description":"Verify the second report","state":"in_progress"}]}`
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
		response(call("plan", operations.UpdatePlanTool, originalPlan), finish()),
		response(call("hold-one", "hold_fixture", `{}`), finish()),
		response(call("discover", "tool_search", `{"query":"select:optional_probe"}`), call("hold-two", "hold_fixture", `{}`), finish()),
		response(call("hold-three", "hold_fixture", `{}`), finish()),
		response(text("Verified"), finish()),
	}}
	fixture := newFixture(t, model, cold, remaining, hold)
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
	if model.count() != len(model.scripts) {
		t.Fatalf("model requests = %d", model.count())
	}
	const planSection = `<context-section name="plan">`
	first := promptText(model.calls[0])
	if strings.Contains(first, planSection) || !slices.Contains(offeredNames(model.calls[0]), operations.UpdatePlanTool) {
		t.Fatal("fresh request did not offer the native plan tool without inventing plan state")
	}
	next := promptText(model.calls[1])
	if !strings.Contains(next, planSection) || !strings.Contains(next, originalPlan) {
		t.Fatal("the native tool result did not appear in the current plan context")
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
		if !ok || !strings.Contains(after, planSection) || !strings.Contains(after, originalPlan) ||
			!slices.Contains(offeredNames(request), operations.UpdatePlanTool) {
			t.Errorf("request %d lacks the native progress tool or verified plan after guidance", step.index)
		}
		_, catalog, ok := strings.Cut(after, `<context-section name="deferred_tools">`)
		catalog, _, _ = strings.Cut(catalog, "</context-section>")
		if !ok || !strings.Contains(catalog, "remaining_probe") || strings.Contains(catalog, "update_plan") || strings.Contains(catalog, "optional_probe") == step.loaded {
			t.Errorf("request %d has stale deferred tools: %q", step.index, catalog)
		}
		if enabled := slices.ContainsFunc(request.Tools, func(tool fantasy.Tool) bool { return tool.GetName() == "optional_probe" }); enabled != step.loaded {
			t.Errorf("request %d has wrong discovered-tool declaration: %v", step.index, enabled)
		}
		if step.index == 4 && strings.Count(after, planSection) != 1 {
			t.Error("unchanged plan was injected again without new input")
		}
	}
}
