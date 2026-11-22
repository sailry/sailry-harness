package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNativePlanPersistsOrderedProgressInPlanMode(t *testing.T) {
	const earlier = `{"title":null,"steps":[{"description":"Read inputs","state":"in_progress"}]}`
	const current = `{"title":"Plan checks","steps":[{"description":"Read inputs","state":"completed"},{"description":"Run checks","state":"pending"}]}`
	checking, release := make(chan struct{}), make(chan struct{})
	check := objectTool("check_fixture", true, true, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
		close(checking)
		select {
		case <-release:
		case <-ctx.Done():
		}
		return ToolResult{Value: true}
	})
	model := &scriptedModel{scripts: []script{
		response(call("earlier", operations.LegacyUpdatePlanTool, earlier), call("current", operations.UpdatePlanTool, current), call("check", "check_fixture", `{}`), finish()),
		response(text("Ready"), finish()),
	}}
	f := newFixture(t, model, check)
	config := f.session.Config
	config.Mode = protocol.WorkModePlan
	f.session = f.command("set_session_config", &protocol.CommandSetSessionConfigPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision, Config: config}).Data.(protocol.Session)
	f.start()
	turn := f.submit("Prepare the checks")
	select {
	case <-checking:
	case <-f.ctx.Done():
		t.Fatal("check did not start")
	}
	state := f.state()
	plan := store.LatestAgentPlan(state.Records)
	if plan == nil || plan.Title == nil || *plan.Title != "Plan checks" || len(plan.Steps) != 2 || plan.Steps[1].Description != "Run checks" {
		t.Fatalf("saved plan = %+v", plan)
	}
	var history protocol.ConversationHistory
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		history, err = store.ReadConversation(f.ctx, db, f.session.ID, nil, 10)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	var projected []string
	for _, entry := range history.Page.Entries {
		for _, part := range entry.Parts {
			result, ok := part.Data.(*protocol.ConversationPartToolResultPayload)
			if !ok || result.ID == nil || result.Summary == nil || !operations.IsUpdatePlanTool(result.Name) {
				continue
			}
			if result.Summary.Progress != nil {
				projected = append(projected, *result.ID)
			}
		}
	}
	if !slices.Equal(projected, []string{"earlier", "current"}) {
		t.Fatalf("projected plan updates = %v", projected)
	}
	close(release)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
}

func TestChildRestoresCompactedPlanOnlyWithItsPlanTool(t *testing.T) {
	const plan = `{"title":"Child checks","steps":[{"description":"Verify the result","state":"in_progress"}]}`
	for _, selected := range []bool{false, true} {
		name := "unselected"
		if selected {
			name = "selected"
		}
		t.Run(name, func(t *testing.T) {
			m := &scriptedModel{scripts: []script{response(text("Acknowledged"), finish())}}
			f, x, binding, _ := compactionFixture(t, m)
			if err := f.r.captureTools(x); err != nil {
				t.Fatal(err)
			}
			x.parentTools = captureInheritedTools(x.tools)
			delegation := &protocol.Delegation{}
			x.inv.Session.Delegation = &delegation
			tools := []protocol.PluginConversationTool{}
			if selected {
				tools = append(tools, protocol.PluginConversationTool{Kind: "builtin", Data: &protocol.PluginConversationToolBuiltinPayload{Name: operations.UpdatePlanTool}})
			}
			x.childRole = &protocol.RoleProfile{Tools: &tools}
			if err := f.r.captureTools(x); err != nil {
				t.Fatal(err)
			}
			now, turn := f.r.clock.Now(), x.inv.Run.Turn
			call := canonical.ToolRequest{ID: "plan", Name: operations.UpdatePlanTool, Arguments: json.RawMessage(plan)}
			request := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &call}), now)
			request.Provenance.Author = "assistant"
			receipt := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, responseBlock(0, call, updatePlanTool().Execute(x.ctx, call))), now)
			receipt.Provenance.Author = "tool"
			checkpoint := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: receipt.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Earlier task work")), Start: now, End: now}, now)
			if err := f.r.save(x.ctx, f.session.ID, request, receipt, checkpoint); err != nil {
				t.Fatal(err)
			}
			x.timeline = newTimeline(f.r.clock)
			if err := f.r.infer(x, binding, m, f.state()); err != nil {
				t.Fatal(err)
			}
			prompt := promptText(m.calls[0])
			if !strings.Contains(prompt, "Earlier task work") || strings.Contains(prompt, `<context-section name="plan">`) != selected || strings.Contains(prompt, plan) != selected {
				t.Fatalf("compacted child plan restoration did not match the captured role: %s", prompt)
			}
		})
	}
}

func TestNextTurnRestoresOnlyRewoundPlan(t *testing.T) {
	const open = `{"title":"Audit","steps":[{"description":"Verify the inputs","state":"pending"}]}`
	const closed = `{"title":"Audit","steps":[{"description":"Verify the inputs","state":"completed"}]}`
	const resume = "Resume from the retained turn"
	m := &scriptedModel{scripts: []script{
		response(call("open", operations.UpdatePlanTool, open), finish()), response(text("Inspection pending"), finish()),
		response(call("close", operations.UpdatePlanTool, closed), finish()), response(text("Inspection complete"), finish()),
		response(text("Ready to verify"), finish()),
	}}
	f := newFixture(t, m)
	f.start()
	first := f.submit("Plan the audit")
	f.waitRun(first, protocol.ConversationStatusCompleted)
	second := f.submit("Complete the audit")
	f.waitRun(second, protocol.ConversationStatusCompleted)
	var revision uint64
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		history, err := store.ReadConversation(f.ctx, db, f.session.ID, nil, 1)
		revision = history.Page.Revision
		return err
	}); err != nil {
		t.Fatal(err)
	}
	f.command("rewind_conversation", &protocol.CommandRewindConversationPayload{Session: f.session.ID, Through: &first, ExpectedHead: second, ExpectedRevision: revision})
	f.waitRun(f.submit(resume), protocol.ConversationStatusCompleted)
	request := promptText(m.calls[4])
	_, tail, found := strings.Cut(request, resume)
	if !found || !strings.Contains(tail, `<context-section name="plan">`) || !strings.Contains(tail, open) || strings.Contains(request, "Inspection complete") {
		t.Fatal("the new turn did not restore the unfinished plan from the retained membership")
	}
}

func TestNativePlanRejectsTextOverHistoryBudget(t *testing.T) {
	tool := updatePlanTool()
	const start = `{"title":null,"steps":[{"description":"`
	const end = `","state":"pending"}]}`
	text := strings.Repeat("\u00e9", 32*1024)
	if result := tool.Execute(t.Context(), canonical.ToolRequest{Arguments: json.RawMessage(start + text + end)}); result.Failure != nil {
		t.Fatalf("exactly 64 KiB of plan text was refused: %+v", result.Failure)
	}
	if result := tool.Execute(t.Context(), canonical.ToolRequest{Arguments: json.RawMessage(start + text + "x" + end)}); result.Failure == nil || result.Failure.Code != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("plan text exceeding 64 KiB was not rejected: %+v", result.Failure)
	}
}
