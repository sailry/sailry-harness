package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestChildAtBudgetHandsGuidanceToSuccessor(t *testing.T) {
	t.Parallel()
	const guidance = "Check the newly supplied constraint"
	requested := make(chan fantasy.Call, 1)
	toolStarted := make(chan struct{})
	language := &scriptedModel{scripts: []script{
		response(call("check", "check_fixture", `{}`), finish()),
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			requested <- request
			response(text("Checked the constraint"), finish())(ctx, request, yield)
		},
	}}
	f, parent := delegationFixture(t, language)
	f.r.tools = []Tool{objectTool("check_fixture", true, true, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
		close(toolStarted)
		<-ctx.Done()
		return toolFault(ctx.Err())
	})}
	parent.inv.Turn.Roles.Profiles = []protocol.RoleProfile{{ID: protocol.NewRoleID(), Key: "bounded", Name: "Bounded check", MaxTurns: new(uint32(1))}}
	roster, err := json.Marshal(parent.inv.Turn.Roles)
	if err != nil {
		t.Fatal(err)
	}
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(f.ctx, "UPDATE turns SET roles=?2 WHERE id=?1", parent.inv.Run.Turn.String(), roster)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := f.r.captureTools(parent); err != nil {
		t.Fatal(err)
	}
	spawn := canonical.ToolRequest{ID: "spawn bounded", Name: "spawn_agent", Arguments: json.RawMessage(`{"task":"Run the check", "role":"bounded", "background":true}`)}
	record := canonical.NewRecord(parent.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &spawn}), f.r.clock.Now())
	record.Provenance.Author = "assistant"
	if err := f.r.save(parent.ctx, parent.inv.Session.ID, record); err != nil {
		t.Fatal(err)
	}
	result := f.r.spawnChild(parent, spawn, spawn.Arguments)
	if result.Failure != nil {
		t.Fatal(result.Failure)
	}
	turn := result.Value.(map[string]any)["turn"].(protocol.TurnID)
	f.start()
	select {
	case <-toolStarted:
	case <-f.ctx.Done():
		t.Fatal("child did not reach the tool boundary on its only step")
	}
	running := f.waitRun(turn, protocol.ConversationStatusRunning)
	queued := f.command("guide_turn", &protocol.CommandGuideTurnPayload{
		Recipient: turn, ExpectedRevision: running.Revision, Message: protocol.ConversationInput{Text: guidance},
	}).Data.(protocol.QueuedTurn)
	first := f.waitRun(turn, protocol.ConversationStatusCompleted)
	next := f.waitRun(queued.ID, protocol.ConversationStatusCompleted)
	if next.Joined != nil || next.StartedMs == nil || first.FinishedMs == nil || *next.StartedMs <= *first.FinishedMs || next.Session != first.Session {
		t.Fatalf("guidance did not run in its own successor: recipient=%+v guidance=%+v", first, next)
	}
	select {
	case request := <-requested:
		if got := strings.Count(promptText(request), guidance); got != 1 {
			t.Fatalf("successor request contained guidance %d times", got)
		}
	default:
		t.Fatal("guidance was marked complete without a model request")
	}
	if language.count() != 2 || f.readQuery("SELECT count(*) FROM agent_events WHERE turn='"+turn.String()+"' AND json_extract(body,'$.metadata.runtime.notes.turn_limit')=1") != 1 {
		t.Fatal("initial child should stop at its budget without an extra attempt")
	}
}
