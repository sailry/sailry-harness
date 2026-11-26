package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (f *engineFixture) waitingApproval() protocol.ConversationApproval {
	f.t.Helper()
	for {
		var id string
		err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(f.ctx, "SELECT id FROM agent_approvals LIMIT 1").Scan(&id)
		})
		if err == nil {
			key, err := protocol.ParseApprovalID(id)
			if err != nil {
				f.t.Fatal(err)
			}
			var value protocol.ConversationApproval
			err = f.db.Node.Read(f.ctx, func(db *sql.Conn) error { var err error; value, err = store.ReadApproval(f.ctx, db, key); return err })
			if err != nil {
				f.t.Fatal(err)
			}
			return value
		}
		if !errors.Is(err, sql.ErrNoRows) {
			f.t.Fatal(err)
		}
		select {
		case <-f.changed:
		case <-f.ctx.Done():
			f.t.Fatal("approval was not committed")
		}
	}
}
func (f *engineFixture) waitingQuestion() protocol.ConversationQuestion {
	f.t.Helper()
	for {
		var id string
		err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(f.ctx, "SELECT id FROM agent_questions LIMIT 1").Scan(&id)
		})
		if err == nil {
			key, err := protocol.ParseQuestionID(id)
			if err != nil {
				f.t.Fatal(err)
			}
			var value protocol.ConversationQuestion
			err = f.db.Node.Read(f.ctx, func(db *sql.Conn) error { var err error; value, err = store.ReadQuestion(f.ctx, db, key); return err })
			if err != nil {
				f.t.Fatal(err)
			}
			return value
		}
		if !errors.Is(err, sql.ErrNoRows) {
			f.t.Fatal(err)
		}
		select {
		case <-f.changed:
		case <-f.ctx.Done():
			f.t.Fatal("question was not committed")
		}
	}
}
func TestApprovalDeadline(t *testing.T) {
	t.Parallel()
	// A tool asks for every call, or a read-only tool asks for the one call
	// that also writes; the plain call beside it runs without asking.
	for _, test := range []struct {
		name      string
		configure func(*Tool)
		calls     script
		ran       int32
	}{
		{"approval tool", func(tool *Tool) { tool.Approval = true }, response(call("write", "write", `{}`), finish()), 0},
		{"confirmed call of a read-only tool", func(tool *Tool) {
			tool.ReadOnly, tool.Concurrent = true, true
			tool.Confirm = func(call canonical.ToolRequest) bool { return string(call.Arguments) == `{"save":true}` }
		}, response(call("plain", "write", `{}`), call("write", "write", `{"save":true}`), finish()), 1},
	} {
		t.Run(test.name, func(t *testing.T) {
			var executions atomic.Int32
			tool := objectTool("write", false, false, func(context.Context, canonical.ToolRequest) ToolResult {
				executions.Add(1)
				return ToolResult{Value: map[string]bool{"ok": true}}
			})
			test.configure(&tool)
			m := &scriptedModel{scripts: []script{test.calls, response(text("Denied"), finish())}}
			f := newFixture(t, m, tool)
			f.start()
			turn := f.submit("Task")
			approval := f.waitingApproval()
			timer := f.r.clock.(*manualClock)
			select {
			case wait := <-timer.waits:
				if wait.duration != 300*time.Second {
					t.Fatalf("deadline=%s", wait.duration)
				}
				close(wait.advance)
			case <-f.ctx.Done():
				t.Fatal("no owner deadline")
			}
			f.waitRun(turn, protocol.ConversationStatusCompleted)
			if executions.Load() != test.ran {
				t.Fatalf("%d calls ran, want %d", executions.Load(), test.ran)
			}
			var after protocol.ConversationApproval
			err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
				var err error
				after, err = store.ReadApproval(f.ctx, db, approval.ID)
				return err
			})
			if err != nil {
				t.Fatal(err)
			}
			if after.State != protocol.ConversationApprovalStateCancelled {
				t.Fatalf("deadline state=%s", after.State)
			}
		})
	}
}
func TestApprovalSettlesBeforeCall(t *testing.T) {
	t.Parallel()
	var executions atomic.Int32
	var f *engineFixture
	tool := objectTool("write", false, false, func(ctx context.Context, request canonical.ToolRequest) ToolResult {
		var state string
		var claimed bool
		err := f.db.Node.Read(ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(ctx, "SELECT json_extract(body,'$.state'),claimed FROM agent_approvals WHERE call=?1", request.ID).Scan(&state, &claimed)
		})
		if err != nil || state != "approved" || !claimed {
			return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "tool ran before decision commit"}}
		}
		executions.Add(1)
		return ToolResult{Value: map[string]bool{"ok": true}}
	})
	tool.Approval = true
	m := &scriptedModel{scripts: []script{response(call("write", "write", `{}`), finish()), response(text("Done"), finish())}}
	f = newFixture(t, m, tool)
	f.start()
	turn := f.submit("Task")
	approval := f.waitingApproval()
	command := protocol.Command{Kind: "resolve_approval", Data: &protocol.CommandResolveApprovalPayload{Session: f.session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}}
	unlock := f.r.LockDecision(command)
	select {
	case wait := <-f.r.clock.(*manualClock).waits:
		close(wait.advance)
	case <-f.ctx.Done():
		unlock()
		t.Fatal("no deadline")
	}
	if executions.Load() != 0 {
		unlock()
		t.Fatal("uncommitted claim executed")
	}
	request := protocol.NewRequest(f.node, command)
	err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, _, err := store.ExecuteAgentCommand(f.ctx, db, f.node, f.r.profile, f.node, request, f.r.clock.Now())
		return err
	})
	unlock()
	if err != nil {
		t.Fatal(err)
	}
	f.r.Committed()
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if executions.Load() != 1 {
		t.Fatalf("executions=%d", executions.Load())
	}
}

func TestDeniedToolReleasesPreparedCall(t *testing.T) {
	t.Parallel()
	var pending atomic.Int32
	tool := objectTool("write", false, false, func(context.Context, canonical.ToolRequest) ToolResult {
		t.Error("denied tool executed")
		return ToolResult{Value: map[string]bool{"ok": true}}
	})
	tool.Approval = true
	tool.Prepare = func(context.Context, canonical.ToolRequest) error { pending.Add(1); return nil }
	tool.ReleasePrepared = func(canonical.ToolRequest) { pending.Add(-1) }
	m := &scriptedModel{scripts: []script{response(call("write", "write", `{}`), finish()), response(text("Done"), finish())}}
	f := newFixture(t, m, tool)
	f.start()
	turn := f.submit("Task")
	approval := f.waitingApproval()
	if got := pending.Load(); got != 1 {
		t.Fatalf("pending preparations before approval: %d", got)
	}
	f.command("resolve_approval", &protocol.CommandResolveApprovalPayload{Session: f.session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "deny"}})
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if got := pending.Load(); got != 0 {
		t.Fatalf("denied tool retained %d preparations", got)
	}
}

func TestReadBeforeAnApprovalRunsFirst(t *testing.T) {
	t.Parallel()
	readDone := make(chan struct{})
	read := objectTool("read", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		close(readDone)
		return ToolResult{Value: "Read"}
	})
	write := objectTool("write", false, false, func(context.Context, canonical.ToolRequest) ToolResult {
		t.Error("denied tool executed")
		return ToolResult{}
	})
	write.Approval = true
	m := &scriptedModel{scripts: []script{
		response(call("read", "read", `{}`), call("write", "write", `{}`), finish()),
		response(text("Done"), finish()),
	}}
	f := newFixture(t, m, read, write)
	f.start()
	turn := f.submit("Read, then write")
	approval := f.waitingApproval()
	select {
	case <-readDone:
	default:
		t.Fatal("the later approval blocked an earlier read")
	}
	f.command("resolve_approval", &protocol.CommandResolveApprovalPayload{Session: f.session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "deny"}})
	f.waitRun(turn, protocol.ConversationStatusCompleted)
}

func TestQuestionSurvivesRestart(t *testing.T) {
	t.Parallel()
	m := &scriptedModel{scripts: []script{response(call("ask", "ask_user", `{"prompt":"Choose","input":{"kind":"choice","options":["First (Recommended)","Second"],"multiple":false}}`), finish())}}
	f := newFixture(t, m)
	f.start()
	turn := f.submit("Task")
	question := f.waitingQuestion()
	f.r.Stop()
	if err := f.r.Recover(f.ctx); err != nil {
		t.Fatal(err)
	}
	var after protocol.ConversationQuestion
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		after, err = store.ReadQuestion(f.ctx, db, question.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if after.ID != question.ID || after.State.Kind != "interrupted" {
		t.Fatalf("recovered question=%+v", after)
	}
	f.waitRun(turn, protocol.ConversationStatusInterrupted)
	if m.count() != 1 {
		t.Fatal("question request replayed")
	}
}

func TestSequentialApprovalToolsObservePriorBatchResults(t *testing.T) {
	t.Parallel()
	// Reproduce two sequential approval-gated edits to the same path (plus a
	// preceding read) in a single model batch. Each edit's ObserveHistory and
	// Prepare must observe the revision committed by the prior call in the batch.
	currentRev := "rev-0"
	var observedRevs []string
	var applied int

	read := objectTool("read_doc", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		return ToolResult{Value: map[string]any{"kind": "file_content", "data": map[string]any{"path": "doc.txt", "revision": currentRev}}}
	})
	var seenRev string
	edit := objectTool("edit_doc", false, false, func(_ context.Context, call canonical.ToolRequest) ToolResult {
		applied++
		currentRev = "rev-" + string(rune('0'+applied))
		return ToolResult{Value: map[string]any{"kind": "file_written", "data": map[string]any{"path": "doc.txt", "revision": currentRev}}}
	})
	edit.Approval = true
	edit.ObserveHistory = func(records []canonical.Record) {
		seenRev = ""
		for _, rec := range records {
			msg := rec.Message()
			if msg == nil || rec.Provenance.Author != "tool" {
				continue
			}
			for _, block := range msg.Content {
				resp, ok := block.Data.(*canonical.ToolResponse)
				if !ok || resp.Status != canonical.ToolSuccess {
					continue
				}
				var payload struct {
					Data struct {
						Revision string `json:"revision"`
					} `json:"data"`
				}
				if json.Unmarshal(resp.Result.Value, &payload) == nil && payload.Data.Revision != "" {
					seenRev = payload.Data.Revision
				}
			}
		}
	}
	edit.Prepare = func(context.Context, canonical.ToolRequest) error {
		observedRevs = append(observedRevs, seenRev)
		if seenRev != currentRev {
			return &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "stale revision " + seenRev + " != " + currentRev}
		}
		return nil
	}
	m := &scriptedModel{scripts: []script{
		response(
			call("r1", "read_doc", `{}`),
			call("e1", "edit_doc", `{"step":1}`),
			call("e2", "edit_doc", `{"step":2}`),
			finish(),
		),
		response(text("Both edits succeeded"), finish()),
	}}
	f := newFixture(t, m, read, edit)
	f.start()
	turn := f.submit("Read and edit twice in one batch")
	for range 2 {
		var approvalID protocol.ApprovalID
		for {
			var raw string
			err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
				return db.QueryRowContext(f.ctx, "SELECT id FROM agent_approvals WHERE json_extract(body, '$.state')='pending' LIMIT 1").Scan(&raw)
			})
			if err == nil {
				approvalID, err = protocol.ParseApprovalID(raw)
				if err != nil {
					t.Fatal(err)
				}
				break
			}
			select {
			case <-f.changed:
			case <-f.ctx.Done():
				t.Fatal("waiting for sequential approval")
			}
		}
		f.command("resolve_approval", &protocol.CommandResolveApprovalPayload{
			Session: f.session.ID, Approval: approvalID, Decision: protocol.ConversationDecision{Kind: "approve"},
		})
	}
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if applied != 2 {
		t.Fatalf("applied edits = %d, want 2", applied)
	}
	if len(observedRevs) != 2 || observedRevs[0] != "rev-0" || observedRevs[1] != "rev-1" {
		t.Fatalf("observed revisions during Prepare = %v, want [rev-0 rev-1]", observedRevs)
	}
}
