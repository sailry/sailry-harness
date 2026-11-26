package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRetryChildTurnRecoversFailedSubagentAndDeliversToParent(t *testing.T) {
	t.Parallel()
	scripts := []script{
		// 1. Parent spawns a foreground non-overlapping child.
		response(fantasy.StreamPart{
			Type:          fantasy.StreamPartTypeToolCall,
			ID:            "call-spawn",
			ToolCallName:  "spawn_agent",
			ToolCallInput: `{"task":"Inspect widget","title":"Widget audit","overlap":false}`,
		}, finish()),
		// 2. Child fails with a permanent error so its turn ends failed immediately.
		response(fantasy.StreamPart{
			Type:  fantasy.StreamPartTypeError,
			Error: &model.Failure{Kind: model.InvalidRequest, Message: "temporary upstream rejection"},
		}),
		// 3. Parent finishes its turn after observing the failed child.
		response(text("Child failed; awaiting recovery."), finish()),
		// 4. Retried child runs cleanly using persisted inherited tools and completes.
		response(text("Recovered child report: all clear."), finish()),
		// 5. Parent receives automatic follow-up notice when retried child completes.
		response(text("Integrated recovered child result."), finish()),
	}
	m := &scriptedModel{scripts: scripts}
	f := newFixture(t, m)

	enableChildRecoveryDelegation(t, f)

	f.start()

	parentTurn := f.submit("Audit widget")
	f.waitRun(parentTurn, protocol.ConversationStatusCompleted)

	// Locate the failed child session & turn.
	var childSession protocol.SessionID
	var failedChildTurn protocol.TurnID
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var s, turn string
		if err := db.QueryRowContext(f.ctx, "SELECT t.session, a.turn FROM agent_delegations d JOIN turns t ON t.id=d.turn JOIN agent_runs a ON a.turn=d.turn WHERE d.parent_turn=?1 ORDER BY t.rowid DESC LIMIT 1", parentTurn.String()).Scan(&s, &turn); err != nil {
			return err
		}
		var err error
		childSession, err = protocol.ParseSessionID(s)
		if err != nil {
			return err
		}
		failedChildTurn, err = protocol.ParseTurnID(turn)
		return err
	}); err != nil {
		t.Fatalf("finding child turn: %v", err)
	}

	runCmd := func(kind string, payload any) (protocol.Output, error) {
		req := protocol.NewRequest(f.node, protocol.Command{Kind: kind, Data: payload})
		raw, err := json.Marshal(req)
		if err != nil {
			return protocol.Output{}, err
		}
		var out protocol.Output
		err = f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
			if _, err := admission.Admit(f.ctx, db, f.node, req, raw); err != nil {
				return err
			}
			var events []protocol.Event
			var e error
			out, events, e = store.ExecuteAgentCommand(f.ctx, db, f.node, f.r.profile, f.node, req, f.r.clock.Now())
			if e != nil {
				return e
			}
			_, e = store.RecordEvents(f.ctx, db, f.node, events)
			return e
		})
		if err == nil {
			f.r.Wake()
		}
		return out, err
	}

	// Verify retry_child_turn refuses a non-delegated main turn.
	if _, err := runCmd("retry_child_turn", &protocol.CommandRetryChildTurnPayload{Turn: parentTurn}); err == nil {
		t.Fatal("retry_child_turn accepted a non-delegated main turn")
	}

	// Controller retries the failed child turn!
	out, err := runCmd("retry_child_turn", &protocol.CommandRetryChildTurnPayload{Turn: failedChildTurn})
	if err != nil {
		t.Fatalf("retry_child_turn failed: %v", err)
	}
	raw, err := json.Marshal(out.Data)
	if err != nil {
		t.Fatal(err)
	}
	var queued protocol.QueuedTurn
	if err := json.Unmarshal(raw, &queued); err != nil {
		t.Fatal(err)
	}
	if queued.Session != childSession {
		t.Fatalf("retried turn session = %s, want %s", queued.Session, childSession)
	}

	// Wait for the retried child turn to complete cleanly.
	retriedRun := f.waitRun(queued.ID, protocol.ConversationStatusCompleted)
	if retriedRun.Status != protocol.ConversationStatusCompleted {
		t.Fatalf("retried child run status = %s, want completed (err=%+v)", retriedRun.Status, retriedRun.Error)
	}

	// Verify retrying a now-completed child turn is refused.
	if _, err := runCmd("retry_child_turn", &protocol.CommandRetryChildTurnPayload{Turn: queued.ID}); err == nil {
		t.Fatal("retry_child_turn accepted a completed child turn")
	}

	// Verify the completed retried child delivered a follow-up notice back to the parent session.
	deadline := time.Now().Add(5 * time.Second)
	var delivered bool
	for time.Now().Before(deadline) {
		var count int
		_ = f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(f.ctx, "SELECT count(*) FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.notice')=1 AND json_extract(a.body,'$.status')='completed'", f.session.ID.String()).Scan(&count)
		})
		if count > 0 {
			delivered = true
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if !delivered {
		t.Fatal("retried child completion was not delivered back to the parent session")
	}
}

func TestParentStopCancelsRetriedChildBeforeFurtherTools(t *testing.T) {
	t.Parallel()
	parentStarted := make(chan struct{})
	firstStarted, firstCancelled, releaseFirst := make(chan struct{}), make(chan struct{}), make(chan struct{})
	unexpected := make(chan struct{}, 1)
	first := objectTool("fixture_first", true, false, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
		close(firstStarted)
		<-ctx.Done()
		close(firstCancelled)
		select {
		case <-releaseFirst:
		case <-t.Context().Done():
		}
		return ToolResult{Value: "Finished already-admitted tool"}
	})
	later := objectTool("fixture_later", true, false, func(context.Context, canonical.ToolRequest) ToolResult {
		unexpected <- struct{}{}
		return ToolResult{Value: "Unexpected tool execution"}
	})
	main := &scriptedModel{scripts: []script{
		response(call("spawn", "spawn_agent", `{"task":"Inspect widget","overlap":false}`), finish()),
		response(text("Original parent turn ended"), finish()),
		func(ctx context.Context, _ fantasy.Call, _ func(fantasy.StreamPart) bool) {
			close(parentStarted)
			<-ctx.Done()
		},
	}}
	child := &scriptedModel{scripts: []script{
		response(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.InvalidRequest, Message: "fixture rejection"}}),
		response(call("first", "fixture_first", `{}`), call("later", "fixture_later", `{}`), finish()),
	}}
	f := backgroundFailureFixture(t, main, child, first, later)
	enableChildRecoveryDelegation(t, f)
	f.start()
	original := f.submit("Delegate the original task")
	f.waitRun(original, protocol.ConversationStatusCompleted)
	failed := onlyBackgroundTestChild(t, f)
	f.waitRun(failed, protocol.ConversationStatusFailed)

	parent := f.submit("Run a later parent turn")
	awaitBackgroundTest(t, f.ctx, parentStarted)
	retry := f.command("retry_child_turn", &protocol.CommandRetryChildTurnPayload{Turn: failed}).Data.(protocol.QueuedTurn)
	awaitBackgroundTest(t, f.ctx, firstStarted)
	f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: parent})
	if _, active := f.r.OperationContext(retry.ID); active {
		t.Fatal("parent Stop did not cancel the claimed retry's operation context")
	}
	awaitBackgroundTest(t, f.ctx, firstCancelled)
	// The parent has to wait for work that the retry already admitted.
	f.waitRun(parent, protocol.ConversationStatusStopping)
	close(releaseFirst)
	f.waitRun(parent, protocol.ConversationStatusCancelled)
	run := f.waitRun(retry.ID, protocol.ConversationStatusCancelled)
	if run.FinishedMs == nil || child.count() != 2 {
		t.Fatalf("retried child did not settle or made another model call: %+v, calls=%d", run, child.count())
	}
	select {
	case <-unexpected:
		t.Fatal("retried child started a later tool after its parent stopped")
	default:
	}
	assertBackgroundFailureState(t, f, retry.ID, protocol.ConversationBackgroundStateConsumed, nil)
}

func enableChildRecoveryDelegation(t *testing.T, f *engineFixture) {
	t.Helper()
	cfg := f.session.Config
	cfg.Mode = protocol.WorkModeFlagship
	f.command("set_session_config", &protocol.CommandSetSessionConfigPayload{
		Session:          f.session.ID,
		ExpectedRevision: f.session.Revision,
		Config:           cfg,
	})
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		f.session, err = store.ReadSession(f.ctx, db, f.session.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
}
