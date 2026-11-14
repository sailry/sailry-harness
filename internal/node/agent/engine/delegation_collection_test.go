package engine

import (
	"database/sql"
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestExplicitCollectionDoesNotRedeliverForegroundResult(t *testing.T) {
	const answer = "Unique completed child response"
	f, parent := delegationFixture(t, &scriptedModel{scripts: []script{response(text(answer), finish())}})
	spawn := saveDelegationCall(t, f, parent, "foreground", false)
	result := f.r.spawnChild(parent, spawn, spawn.Arguments)
	if result.Failure != nil {
		t.Fatal(result.Failure)
	}
	f.r.workers.Wait()
	child := parent.foreground[0]
	record := canonical.NewRecord(parent.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, responseBlock(0, spawn, result)), f.r.clock.Now())
	record.Provenance.Author = "tool"
	if err := f.r.save(parent.ctx, parent.inv.Session.ID, record); err != nil {
		t.Fatal(err)
	}
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := store.ReleaseChildren(f.ctx, db, parent.inv.Run.Turn, []protocol.TurnID{child.turn})
		return err
	}); err != nil {
		t.Fatal(err)
	}
	child.released = true
	f.start()
	f.waitRun(child.turn, protocol.ConversationStatusCompleted)
	// Guidance lets the model explicitly collect the child before automatic
	// delivery resumes at the following model boundary.
	parent.foregroundSteered = true
	args, _ := json.Marshal(map[string]any{"session": child.session})
	wait := canonical.ToolRequest{ID: "collect", Name: "wait_agent", Arguments: args}
	record = canonical.NewRecord(parent.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &wait}), f.r.clock.Now())
	if err := f.r.save(parent.ctx, parent.inv.Session.ID, record); err != nil {
		t.Fatal(err)
	}
	parent.timeline = newTimeline(f.r.clock)
	if err := f.r.captureTools(parent); err != nil {
		t.Fatal(err)
	}
	if err := f.r.runTools(parent, f.state(), []canonical.ToolRequest{wait}); err != nil {
		t.Fatal(err)
	}
	parent.foregroundSteered = false
	if delivered, err := f.r.foregroundStep(parent); err != nil || delivered {
		t.Fatalf("collected foreground result was delivered again: %t, %v", delivered, err)
	}
	f.r.cancelForeground(parent)
	body, err := json.Marshal(f.state().Records)
	if err != nil || strings.Count(string(body), answer) != 1 || strings.Contains(string(body), "was cancelled before it finished") {
		t.Fatalf("collected result was lost, duplicated or cancelled: %s, %v", body, err)
	}
}
