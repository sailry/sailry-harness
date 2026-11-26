package engine

import (
	"context"
	"database/sql"
	"errors"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (f *engineFixture) pendingWriteApproval(call string) protocol.ApprovalID {
	f.t.Helper()
	for {
		var id string
		err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(f.ctx, "SELECT id FROM agent_approvals WHERE call=?1 AND json_extract(body,'$.state')='pending'", call).Scan(&id)
		})
		if err == nil {
			key, err := protocol.ParseApprovalID(id)
			if err != nil {
				f.t.Fatal(err)
			}
			return key
		}
		if !errors.Is(err, sql.ErrNoRows) {
			f.t.Fatal(err)
		}
		select {
		case <-f.changed:
		case <-f.ctx.Done():
			f.t.Fatalf("waiting for approval of %s", call)
		}
	}
}

func (f *engineFixture) decideWrite(call, decision string) {
	f.t.Helper()
	f.command("resolve_approval", &protocol.CommandResolveApprovalPayload{
		Session: f.session.ID, Approval: f.pendingWriteApproval(call), Decision: protocol.ConversationDecision{Kind: decision},
	})
}

func writeBatchIDs(records []canonical.Record) []string {
	var ids []string
	for _, record := range records {
		if msg := record.Message(); msg != nil {
			for _, block := range msg.Content {
				if result, ok := block.Data.(*canonical.ToolResponse); ok {
					ids = append(ids, result.ID)
				}
			}
		}
	}
	return ids
}

func TestDisjointWriteBatchOverlapsAndOrdersDependencies(t *testing.T) {
	t.Parallel()
	paths := map[string][]string{
		"first": {"dir/a"}, "second": {"elsewhere", "other/file"},
		"parent": {"dir", "destination"}, "again": {"destination"},
	}
	started := make(chan string, 2)
	releaseFirst, releaseSecond, secondDone := make(chan struct{}), make(chan struct{}), make(chan struct{})
	write := objectTool("write_files", false, false, func(ctx context.Context, req canonical.ToolRequest) ToolResult {
		var release <-chan struct{}
		switch req.ID {
		case "first":
			release = releaseFirst
		case "second":
			release = releaseSecond
			defer close(secondDone)
		}
		if release != nil {
			started <- req.ID
			select {
			case <-release:
			case <-ctx.Done():
				return toolFault(ctx.Err())
			}
		}
		return ToolResult{Value: req.ID}
	})
	write.Approval = true
	write.WritePaths = func(req canonical.ToolRequest) []string { return paths[req.ID] }
	var receipts []string
	write.ObserveHistory = func(records []canonical.Record) { receipts = writeBatchIDs(records) }
	write.Prepare = func(_ context.Context, req canonical.ToolRequest) error {
		var want []string
		switch req.ID {
		case "parent":
			want = []string{"first", "second"}
		case "again":
			want = []string{"first", "second", "parent"}
		}
		if !slices.Equal(receipts, want) {
			t.Errorf("preparing %s saw receipts %v, want %v", req.ID, receipts, want)
		}
		return nil
	}
	m := &scriptedModel{scripts: []script{
		response(call("first", "write_files", "{}"), call("second", "write_files", "{}"), call("parent", "write_files", "{}"), call("again", "write_files", "{}"), finish()),
		response(text("Done"), finish()),
	}}
	f := newFixture(t, m, write)
	f.start()
	turn := f.submit("Edit the files")
	f.decideWrite("first", "approve")
	f.decideWrite("second", "approve")
	var running []string
	for range 2 {
		select {
		case id := <-started:
			running = append(running, id)
		case <-f.ctx.Done():
			t.Fatalf("disjoint writes did not overlap: %v", running)
		}
	}
	slices.Sort(running)
	if !slices.Equal(running, []string{"first", "second"}) {
		t.Fatalf("unexpected concurrent writes: %v", running)
	}
	close(releaseSecond)
	select {
	case <-secondDone:
	case <-f.ctx.Done():
		t.Fatal("second write did not finish")
	}
	close(releaseFirst)
	f.decideWrite("parent", "approve")
	f.decideWrite("again", "approve")
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	state := f.state()
	if ids := writeBatchIDs(state.Records); !slices.Equal(ids, []string{"first", "second", "parent", "again"}) {
		t.Fatalf("persisted receipt order: %v", ids)
	}
	for id, result := range pressureStoredResults(state.Records) {
		if result.Status != canonical.ToolSuccess || string(result.Result.Value) != `"`+id+`"` {
			t.Errorf("write %s returned %s: %s", id, result.Status, result.Result.Value)
		}
	}
}

func TestDisjointWriteBatchSteeringDrainsApprovedWrite(t *testing.T) {
	t.Parallel()
	started, release := make(chan struct{}), make(chan struct{})
	write := objectTool("write_files", false, false, func(ctx context.Context, req canonical.ToolRequest) ToolResult {
		if req.ID != "approved" {
			t.Errorf("unapproved write %s executed", req.ID)
			return ToolResult{Value: req.ID}
		}
		close(started)
		select {
		case <-release:
			return ToolResult{Value: req.ID}
		case <-ctx.Done():
			return toolFault(ctx.Err())
		}
	})
	write.Approval = true
	write.WritePaths = func(req canonical.ToolRequest) []string { return []string{req.ID} }
	m := &scriptedModel{scripts: []script{
		response(call("approved", "write_files", "{}"), call("denied", "write_files", "{}"), call("withdrawn", "write_files", "{}"), finish()),
		response(text("Updated"), finish()),
	}}
	f := newFixture(t, m, write)
	f.start()
	turn := f.submit("Edit independent files")
	f.decideWrite("approved", "approve")
	select {
	case <-started:
	case <-f.ctx.Done():
		t.Fatal("approved write did not start")
	}
	f.decideWrite("denied", "deny")
	f.pendingWriteApproval("withdrawn")
	f.steerQueued(turn, f.queueForSteering("Stop editing"))
	close(release)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	results := pressureStoredResults(f.state().Records)
	if result := results["approved"]; result == nil || result.Status != canonical.ToolSuccess || string(result.Result.Value) != `"approved"` {
		t.Fatalf("approved write lost its result: %+v", result)
	}
	for _, id := range []string{"denied", "withdrawn"} {
		if result := results[id]; result == nil || result.Status != canonical.ToolErrorStatus {
			t.Fatalf("%s was not refused: %+v", id, result)
		}
	}
}

func TestDisjointWriteBatchStopKeepsCompletedWrite(t *testing.T) {
	t.Parallel()
	done := make(chan struct{})
	write := objectTool("write_files", false, false, func(_ context.Context, req canonical.ToolRequest) ToolResult {
		if req.ID != "first" {
			t.Errorf("unapproved write %s executed", req.ID)
		}
		close(done)
		return ToolResult{Value: req.ID}
	})
	write.Approval = true
	write.WritePaths = func(req canonical.ToolRequest) []string { return []string{req.ID} }
	m := &scriptedModel{scripts: []script{
		response(call("first", "write_files", "{}"), call("second", "write_files", "{}"), finish()),
	}}
	f := newFixture(t, m, write)
	f.start()
	turn := f.submit("Make independent edits")
	f.decideWrite("first", "approve")
	f.pendingWriteApproval("second")
	select {
	case <-done:
	case <-f.ctx.Done():
		t.Fatal("the first approved write did not execute")
	}
	f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: turn})
	f.waitRun(turn, protocol.ConversationStatusCancelled)
	results := pressureStoredResults(f.state().Records)
	if got := results["first"]; got == nil || got.Status != canonical.ToolSuccess || string(got.Result.Value) != `"first"` {
		t.Fatalf("completed write lost its receipt: %+v", got)
	}
	if got := results["second"]; got == nil || got.Status != canonical.ToolErrorStatus {
		t.Fatalf("unexpected receipt for the unapproved write: %+v", got)
	}
}
