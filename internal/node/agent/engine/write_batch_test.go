package engine

import (
	"context"
	"database/sql"
	"errors"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
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

func TestDisjointWriteBatchWaitsOnlyForTheSameFile(t *testing.T) {
	t.Parallel()
	// The second call targets the same file as the first. The third shares
	// only a path prefix and can run while the first is still executing.
	paths := map[string]string{"first": "assets", "again": "assets", "child": "assets/icon"}
	releaseFirst, releaseChild := make(chan struct{}), make(chan struct{})
	started := make(chan string, len(paths))
	write := objectTool("write_files", false, false, func(ctx context.Context, req canonical.ToolRequest) ToolResult {
		started <- req.ID
		var release <-chan struct{}
		switch req.ID {
		case "first":
			release = releaseFirst
		case "child":
			release = releaseChild
		}
		if release != nil {
			select {
			case <-release:
			case <-ctx.Done():
				return toolFault(ctx.Err())
			}
		}
		return ToolResult{Value: req.ID}
	})
	write.Approval = true
	write.FilePaths = func(req canonical.ToolRequest) []string { return []string{paths[req.ID]} }
	var receipts []string
	write.ObserveHistory = func(records []canonical.Record) { receipts = writeBatchIDs(records) }
	write.Prepare = func(_ context.Context, req canonical.ToolRequest) error {
		if req.ID == "again" && !slices.Contains(receipts, "first") {
			t.Errorf("same-file preparation did not observe the preceding write: %v", receipts)
		}
		return nil
	}
	m := &scriptedModel{scripts: []script{
		response(call("first", "write_files", "{}"), call("again", "write_files", "{}"), call("child", "write_files", "{}"), finish()),
		response(text("Done"), finish()),
	}}
	f := newFixture(t, m, write)
	f.start()
	turn := f.submit("Edit the files")
	waitStarted := func(want string) {
		t.Helper()
		select {
		case id := <-started:
			if id != want {
				t.Fatalf("started %q, want %q", id, want)
			}
		case <-f.ctx.Done():
			t.Fatalf("waiting for %s to start", want)
		}
	}
	f.decideWrite("first", "approve")
	waitStarted("first")
	f.decideWrite("child", "approve")
	waitStarted("child")
	// The slow independent operation cannot delay the second write once
	// the first write to that file is complete.
	close(releaseFirst)
	f.decideWrite("again", "approve")
	waitStarted("again")
	close(releaseChild)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	state := f.state()
	if ids := writeBatchIDs(state.Records); !slices.Equal(ids, []string{"first", "again", "child"}) {
		t.Fatalf("persisted receipt order: %v", ids)
	}
	for id, result := range pressureStoredResults(state.Records) {
		if result.Status != canonical.ToolSuccess || string(result.Result.Value) != `"`+id+`"` {
			t.Errorf("write %s returned %s: %s", id, result.Status, result.Result.Value)
		}
	}
}

func TestMixedCodingBatchReadsWritesAndRunsCommandTogether(t *testing.T) {
	t.Parallel()
	readRelease, releaseOthers := make(chan struct{}), make(chan struct{})
	started := make(chan string, 4)
	makeTool := func(name string, op protocol.ToolOperation, readOnly bool, release <-chan struct{}) Tool {
		tool := objectTool(name, readOnly, readOnly, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
			started <- call.ID
			if release != nil {
				select {
				case <-release:
				case <-ctx.Done():
					return toolFault(ctx.Err())
				}
			}
			return ToolResult{Value: call.ID}
		})
		tool.Binding = &store.ApprovalBinding{Tool: name, Operation: &op}
		tool.Approval = !readOnly
		return tool
	}
	read := makeTool("read_file", protocol.ToolOperationReadFileLines, true, readRelease)
	edit := makeTool("edit_file", protocol.ToolOperationEditFile, false, nil)
	write := makeTool("write_file", protocol.ToolOperationWriteFile, false, releaseOthers)
	shell := makeTool("run_command", protocol.ToolOperationRunCommand, false, releaseOthers)
	read.FilePaths = func(canonical.ToolRequest) []string { return []string{"a.go"} }
	edit.FilePaths = read.FilePaths
	write.FilePaths = func(canonical.ToolRequest) []string { return []string{"b.go"} }

	var readObserved bool
	edit.ObserveHistory = func(records []canonical.Record) {
		result := pressureStoredResults(records)["read"]
		readObserved = result != nil && result.Status == canonical.ToolSuccess
	}
	edit.Prepare = func(context.Context, canonical.ToolRequest) error {
		if !readObserved {
			return errors.New("the required file read has not completed")
		}
		return nil
	}
	m := &scriptedModel{scripts: []script{
		response(call("read", "read_file", `{"path":"a.go"}`), call("edit", "edit_file", `{"path":"a.go","old_text":"old","new_text":"new"}`),
			call("write", "write_file", `{"path":"b.go","text":"new"}`), call("shell", "run_command", `{"command":"git status"}`), finish()),
		response(text("Done"), finish()),
	}}
	f := newFixture(t, m, read, edit, write, shell)
	f.start()
	turn := f.submit("Inspect the workspace and update the files")
	f.decideWrite("write", "approve")
	f.decideWrite("shell", "approve")
	var active []string
	for range 3 {
		select {
		case id := <-started:
			active = append(active, id)
		case <-f.ctx.Done():
			t.Fatalf("mixed batch stalled with %v active", active)
		}
	}
	slices.Sort(active)
	if !slices.Equal(active, []string{"read", "shell", "write"}) {
		t.Fatalf("unexpected concurrent calls: %v", active)
	}
	close(readRelease)
	f.decideWrite("edit", "approve")
	select {
	case id := <-started:
		if id != "edit" {
			t.Fatalf("expected same-file edit, got %s", id)
		}
	case <-f.ctx.Done():
		t.Fatal("edit did not start after its file read returned")
	}
	close(releaseOthers)
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if ids := writeBatchIDs(f.state().Records); !slices.Equal(ids, []string{"read", "edit", "write", "shell"}) {
		t.Fatalf("mixed batch lost request order: %v", ids)
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
	write.FilePaths = func(req canonical.ToolRequest) []string { return []string{req.ID} }
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
	write.FilePaths = func(req canonical.ToolRequest) []string { return []string{req.ID} }
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
