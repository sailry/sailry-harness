package store

import (
	"context"
	"database/sql"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCommandEvidenceRecovery(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	input := protocol.DefaultCommandRunCommandPayload()
	input.Turn, input.Command, input.Background, input.FollowUp = turn, "fixture command", true, true
	request := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error { _, err := RegisterCommand(ctx, db, request, now); return err }); err != nil {
		t.Fatal(err)
	}
	output := strings.Repeat("x", 2048) + "中"
	completion := process.Finished{Snapshot: protocol.ProcessSnapshot{Info: protocol.ProcessInfo{ID: request.ID, Session: session.ID, Turn: turn, Status: protocol.ProcessStatus{Kind: "finished", Data: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}}, Stdout: protocol.ProcessCapture{Text: output}}}
	elapsed := uint64(12)
	completion.ElapsedMs = &elapsed
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := FinishCommand(ctx, db, completion, now.Add(time.Second))
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := FinishCommand(ctx, db, completion, now.Add(2*time.Second)); err != nil {
			return err
		}
		record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "command:"+request.ID.String())
		if err != nil {
			return err
		}
		if record == nil || !record.FollowUp || record.Task.Revision != 2 || record.Task.State != protocol.ConversationBackgroundStatePending || record.Task.Outcome == nil || *record.Task.Outcome != protocol.ConversationBackgroundOutcomeCompleted || record.ElapsedMs == nil || *record.ElapsedMs != 12 || record.Stdout == nil || !strings.HasSuffix(*record.Stdout, "中") || len(*record.Stdout) > 2048 {
			return errors.New("durable command completion changed")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	interrupted := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := RegisterPromotedCommand(ctx, db, interrupted, now); err != nil {
			return err
		}
		if _, err := RecoverCommands(ctx, db, now.Add(3*time.Second)); err != nil {
			return err
		}
		record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "command:"+interrupted.ID.String())
		if err != nil {
			return err
		}
		if record == nil || record.Task.Outcome == nil || *record.Task.Outcome != protocol.ConversationBackgroundOutcomeInterrupted || !record.Task.Recovered || !record.Task.Suppressed || !record.FollowUp {
			return errors.New("recovery lost interruption evidence")
		}
		tasks, err := ReadBackgroundTasks(ctx, db, session.ID)
		if err != nil {
			return err
		}
		if len(tasks) != 2 {
			return errors.New("background evidence was not retained")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestRemovedSessionDismissesCommandEvidence(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	input := protocol.DefaultCommandRunCommandPayload()
	// A follow-up would queue a delivery turn, and a busy session cannot be removed.
	input.Turn, input.Command, input.Background = turn, "fixture server", true
	running := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
	pending := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
	exited := process.Finished{Snapshot: protocol.ProcessSnapshot{Info: protocol.ProcessInfo{ID: pending.ID, Session: session.ID, Turn: turn, Status: protocol.ProcessStatus{Kind: "finished", Data: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}}}}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		for _, request := range []protocol.Request{running, pending} {
			if _, err := RegisterCommand(ctx, db, request, now); err != nil {
				return err
			}
		}
		if _, err := FinishCommand(ctx, db, exited, now); err != nil {
			return err
		}
		if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		current, err := ReadSession(ctx, db, session.ID)
		if err != nil {
			return err
		}
		output, events, err := removeSession(ctx, db, session.ID, current.Revision)
		if err != nil {
			return err
		}
		if output.Kind != "sessions_removed" || len(events) != 1 || events[0].Kind != "sessions_removed" {
			return errors.New("removal lost its event")
		}
		for _, request := range []protocol.Request{running, pending} {
			record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "command:"+request.ID.String())
			if err != nil {
				return err
			}
			if record == nil || record.Task.State != protocol.ConversationBackgroundStateDismissed || record.Task.Delivery != nil {
				return errors.New("removed session kept live command evidence")
			}
		}
		// The stopped process reports after the removal; the dismissed row stays closed.
		stopped := process.Finished{Snapshot: protocol.ProcessSnapshot{Info: protocol.ProcessInfo{ID: running.ID, Session: session.ID, Turn: turn, Status: protocol.ProcessStatus{Kind: "finished", Data: protocol.ProcessOutcome{Kind: "cancelled"}}}}}
		if changes, err := FinishCommand(ctx, db, stopped, now.Add(time.Second)); err != nil || len(changes) != 0 {
			return errors.New("late completion reopened dismissed evidence")
		}
		record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", "command:"+running.ID.String())
		if err != nil {
			return err
		}
		if record.Task.State != protocol.ConversationBackgroundStateDismissed || record.Task.Outcome != nil {
			return errors.New("late completion changed dismissed evidence")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
