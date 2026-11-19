package engine

import (
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
	model := &scriptedModel{scripts: []script{
		response(call("earlier", operations.LegacyUpdatePlanTool, earlier), call("current", operations.UpdatePlanTool, current), finish()),
		response(text("Ready"), finish()),
	}}
	f := newFixture(t, model)
	config := f.session.Config
	config.Mode = protocol.WorkModePlan
	f.session = f.command("set_session_config", &protocol.CommandSetSessionConfigPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision, Config: config}).Data.(protocol.Session)
	f.start()
	f.waitRun(f.submit("Prepare the checks"), protocol.ConversationStatusCompleted)
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
