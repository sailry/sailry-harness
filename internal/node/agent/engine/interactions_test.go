package engine

import (
	"context"
	"database/sql"
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
	var executions atomic.Int32
	tool := objectTool("write", false, false, func(context.Context, canonical.ToolRequest) ToolResult {
		executions.Add(1)
		return ToolResult{Value: map[string]bool{"ok": true}}
	})
	tool.Approval = true
	m := &scriptedModel{scripts: []script{response(call("write", "write", `{}`), finish()), response(text("Denied"), finish())}}
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
	if executions.Load() != 0 {
		t.Fatal("timeout authorized execution")
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
	if after.State == protocol.ConversationApprovalStateApproved || after.State == protocol.ConversationApprovalStatePending {
		t.Fatalf("deadline state=%s", after.State)
	}
}
func TestApprovalSettlesBeforeCall(t *testing.T) {
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
func TestQuestionSurvivesRestart(t *testing.T) {
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
