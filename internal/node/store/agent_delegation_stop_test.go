package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestStopQueuedChild(t *testing.T) {
	t.Parallel()
	record := func(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (*BackgroundRecord, error) {
		return readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "agent:"+turn.String())
	}
	t.Run("an unstarted child is finished by settlement", func(t *testing.T) {
		t.Parallel()
		storage, node, _, parent, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": "Queued child", "background": true}, now)
			if err != nil {
				return err
			}
			child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
			if err != nil {
				return err
			}
			stopped, err := stopAgentTurn(ctx, db, child.Run.Turn, now)
			if err != nil {
				return err
			}
			if stopped.Status != protocol.ConversationStatusCancelled || stopped.FinishedMs != nil {
				return fmt.Errorf("stopped queued child was finished in place: %+v", stopped)
			}
			if inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now); err != nil || inv != nil {
				return fmt.Errorf("stopped child was claimed: %v", err)
			}
			again, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"session": child.Session.ID, "task": "Reuse before settlement"}, now)
			if err != nil {
				return err
			}
			var busy *protocol.Fault
			if _, _, err = DelegateAgent(ctx, db, node, parent, again, nil, true, now); !errors.As(err, &busy) || busy.Code != protocol.ErrorCodeBusy {
				return fmt.Errorf("unsettled child accepted new work: %v", err)
			}
			events, err := SettleStoppedChildren(ctx, db, now)
			if err != nil {
				return err
			}
			if len(events) == 0 {
				return errors.New("settlement did not notify the child's owners")
			}
			run, err := ReadAgentRun(ctx, db, child.Run.Turn)
			if err != nil {
				return err
			}
			if run.Status != protocol.ConversationStatusCancelled || run.FinishedMs == nil || run.StartedMs != nil {
				return fmt.Errorf("settled child: %+v", run)
			}
			task, err := record(ctx, db, child.Run.Turn)
			if err != nil {
				return err
			}
			if task == nil || task.Task.State != protocol.ConversationBackgroundStatePending || task.Task.Outcome == nil || *task.Task.Outcome != protocol.ConversationBackgroundOutcomeCancelled {
				return fmt.Errorf("stopped child left its record running: %+v", task)
			}
			if events, err = SettleStoppedChildren(ctx, db, now); err != nil || len(events) != 0 {
				return fmt.Errorf("settlement repeated: %v %v", events, err)
			}
			// The cancelled result is not eligible for an automatic notice turn.
			var eligible bool
			if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_background WHERE "+backgroundEligible+")", task.Task.Session.String()).Scan(&eligible); err != nil || eligible {
				return fmt.Errorf("stopped child is eligible for a notice turn: %v", err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("queued guidance ends where it is stopped", func(t *testing.T) {
		t.Parallel()
		storage, node, _, parent, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": "Running child", "background": true}, now)
			if err != nil {
				return err
			}
			child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
			if err != nil {
				return err
			}
			if inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now); err != nil || inv == nil {
				return fmt.Errorf("child was not claimed: %v", err)
			}
			guide, err := delegationFixtureCall(ctx, db, parent, "guide_agent", map[string]any{"session": child.Session.ID, "message": "Queued guidance"}, now)
			if err != nil {
				return err
			}
			receipt, _, _, err := GuideChild(ctx, db, node, parent, guide, guide.Arguments, now)
			if err != nil {
				return err
			}
			stopped, err := stopAgentTurn(ctx, db, receipt.Run.Turn, now)
			if err != nil {
				return err
			}
			if stopped.Status != protocol.ConversationStatusCancelled || stopped.FinishedMs == nil {
				return fmt.Errorf("stopped guidance: %+v", stopped)
			}
			if events, err := SettleStoppedChildren(ctx, db, now); err != nil || len(events) != 0 {
				return fmt.Errorf("guidance was settled as a child task: %v %v", events, err)
			}
			task, err := record(ctx, db, child.Run.Turn)
			if err != nil {
				return err
			}
			if task == nil || task.Task.State != protocol.ConversationBackgroundStateRunning {
				return fmt.Errorf("stopping guidance closed the running child's record: %+v", task)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("an ordinary queued turn ends where it is stopped", func(t *testing.T) {
		t.Parallel()
		storage, node, session, _, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Queued input"}}})
			body, err := encode(request)
			if err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body); err != nil {
				return err
			}
			queued, err := admitAgentTurn(ctx, db, node, node, request, session, protocol.ConversationRunKindTask, true)
			if err != nil {
				return err
			}
			stopped, err := stopAgentTurn(ctx, db, queued.ID, now)
			if err != nil {
				return err
			}
			if stopped.Status != protocol.ConversationStatusCancelled || stopped.FinishedMs == nil {
				return fmt.Errorf("stopped queued turn: %+v", stopped)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
}

func TestConsumeStoppedChildren(t *testing.T) {
	t.Parallel()
	storage, node, _, parent, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		var children []AgentInvocation
		for _, task := range []string{"Cancelled child", "Finished child"} {
			call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": task, "background": true}, now)
			if err != nil {
				return err
			}
			child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
			if err != nil {
				return err
			}
			if inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now); err != nil || inv == nil {
				return fmt.Errorf("child was not claimed: %v", err)
			}
			if _, _, err = FinishTurn(ctx, db, child.Run.Turn, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "fixture"}, now); err != nil {
				return err
			}
			children = append(children, child)
		}
		// A stopping parent still owns the settlement of the children it cancelled.
		if _, err := stopAgentTurn(ctx, db, parent, now); err != nil {
			return err
		}
		events, err := ConsumeStoppedChildren(ctx, db, parent, []protocol.TurnID{children[0].Run.Turn})
		if err != nil {
			return err
		}
		if len(events) == 0 {
			return errors.New("consumption did not notify the parent")
		}
		for index, want := range []protocol.ConversationBackgroundState{protocol.ConversationBackgroundStateConsumed, protocol.ConversationBackgroundStatePending} {
			task, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "agent:"+children[index].Run.Turn.String())
			if err != nil {
				return err
			}
			if task == nil || task.Task.State != want {
				return fmt.Errorf("child %d record: %+v", index, task)
			}
		}
		var denied *protocol.Fault
		if _, err = ConsumeStoppedChildren(ctx, db, children[0].Run.Turn, []protocol.TurnID{children[1].Run.Turn}); !errors.As(err, &denied) || denied.Code != protocol.ErrorCodePermissionDenied {
			return fmt.Errorf("another session consumed the child: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
