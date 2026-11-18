package store

import (
	"database/sql"
	"errors"
	"fmt"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestBackgroundCallbackSettlesFrozenBatch(t *testing.T) {
	t.Parallel()
	storage, node, session, parent, now := agentHistoryFixture(t, protocol.WorkModeFlagship)
	ctx := t.Context()
	var frozen []string
	var notice, failureTurn protocol.TurnID
	var untouched string
	var queueRevision uint64
	const reason = "Fixture original child outcome is unknown"
	const late = "late-completion"
	pending := func(db *sql.Conn) (bool, error) {
		var exists bool
		err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending WHERE turn=?1)", notice.String()).Scan(&exists)
		return exists, err
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := queueFixtureInput(ctx, db, session.ID, parent, now); err != nil {
			return err
		}
		input := protocol.DefaultCommandRunCommandPayload()
		input.Turn, input.Command, input.Background, input.FollowUp = parent, "Fixture command", true, true
		request := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
		if _, err := RegisterCommand(ctx, db, request, now); err != nil {
			return err
		}
		snapshot := protocol.ProcessSnapshot{
			Info:   protocol.ProcessInfo{ID: request.ID, Session: session.ID, Turn: parent, Status: protocol.ProcessStatus{Kind: "finished", Data: protocol.ProcessOutcome{Kind: "timed_out"}}},
			Stdout: protocol.ProcessCapture{Text: "command stdout"}, Stderr: protocol.ProcessCapture{Text: "command stderr"},
		}
		if _, err := FinishCommand(ctx, db, process.Finished{Snapshot: snapshot, ElapsedMs: new(uint64(12))}, now); err != nil {
			return err
		}
		frozen = append(frozen, "command:"+request.ID.String())
		child := func(background bool, reuse *protocol.SessionID, failure *protocol.Fault) (AgentInvocation, error) {
			args := map[string]any{"task": "Fixture child", "background": background}
			if reuse != nil {
				args["session"] = reuse
			}
			call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", args, now)
			if err != nil {
				return AgentInvocation{}, err
			}
			inv, _, err := DelegateAgent(ctx, db, node, parent, call, nil, background, now)
			if err != nil {
				return inv, err
			}
			if !background {
				if _, err = ReleaseChildren(ctx, db, parent, []protocol.TurnID{inv.Run.Turn}); err != nil {
					return inv, err
				}
			}
			claimed, _, err := ClaimChild(ctx, db, inv.Run.Turn, now)
			if err != nil || claimed == nil {
				return inv, fmt.Errorf("child did not start: %v", err)
			}
			status := protocol.ConversationStatusCompleted
			if failure != nil {
				status = protocol.ConversationStatusFailed
			} else {
				reply := canonical.NewRecord(inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "child response")), now)
				reply.Provenance.Author = "assistant"
				if _, _, err = AppendCanonical(ctx, db, inv.Session.ID, reply); err != nil {
					return inv, err
				}
			}
			_, _, err = FinishTurn(ctx, db, inv.Run.Turn, status, failure, now)
			return inv, err
		}
		failed, err := child(true, nil, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: reason})
		if err != nil {
			return err
		}
		failureTurn = failed.Run.Turn
		frozen = append(frozen, "agent:"+failureTurn.String())
		// The exact failure must survive a later success in the same child session.
		succeeded, err := child(true, &failed.Session.ID, nil)
		if err != nil {
			return err
		}
		legacy := "agent:" + succeeded.Run.Turn.String()
		if _, err = db.ExecContext(ctx, "UPDATE agent_background SET body=json_remove(body,'$.follow_up') WHERE id=?1", legacy); err != nil {
			return err
		}
		frozen = append(frozen, legacy)
		foreground, err := child(false, nil, nil)
		if err != nil {
			return err
		}
		untouched = "agent:" + foreground.Run.Turn.String()
		if _, _, err = FinishTurn(ctx, db, parent, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		claimed, _, err := ClaimNext(ctx, db, now)
		if err != nil || claimed == nil || !claimed.Run.Notice || len(claimed.Run.Notices) != len(frozen) {
			return fmt.Errorf("mixed results were not reserved as one notice: %v", err)
		}
		notice = claimed.Run.Turn
		for i, id := range frozen {
			if claimed.Run.Notices[i].ID != id {
				return fmt.Errorf("reservation %d is %q, want %q", i, claimed.Run.Notices[i].ID, id)
			}
		}
		exists, err := pending(db)
		if err != nil || !exists {
			return fmt.Errorf("notice lost its queue item before delivery: %v", err)
		}
		queue, err := ReadQueue(ctx, db, session.ID)
		if err != nil {
			return err
		}
		queueRevision = queue.Revision
		if _, err = backgroundFixtureRecord(ctx, db, session, parent, late, true, now); err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, `CREATE TRIGGER refuse_background BEFORE INSERT ON agent_events WHEN json_extract(NEW.body,'$.kind')='background'
			BEGIN SELECT RAISE(ABORT,'injected background append failure'); END`)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, _, err := DeliverBackground(ctx, db, notice, true, now)
		return err
	}); err == nil || !strings.Contains(err.Error(), "injected background append failure") {
		t.Fatalf("expected injected append failure, got %v", err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		exists, err := pending(db)
		if err != nil || !exists {
			return fmt.Errorf("failed append changed the notice queue: %v", err)
		}
		queue, err := ReadQueue(ctx, db, session.ID)
		if err != nil || queue.Revision != queueRevision {
			return fmt.Errorf("failed append changed the notice queue revision: %v", err)
		}
		check := func(id string, state protocol.ConversationBackgroundState, delivery *protocol.TurnID) error {
			record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", id)
			if err != nil {
				return err
			}
			if record == nil || record.Task.State != state || (record.Task.Delivery == nil) != (delivery == nil) || delivery != nil && *record.Task.Delivery != *delivery {
				return fmt.Errorf("evidence %s has wrong state or delivery: %+v", id, record)
			}
			return nil
		}
		for _, id := range frozen {
			if err = check(id, protocol.ConversationBackgroundStatePending, &notice); err != nil {
				return err
			}
		}
		if _, err = db.ExecContext(ctx, "DROP TRIGGER refuse_background"); err != nil {
			return err
		}
		entry, events, err := DeliverBackground(ctx, db, notice, true, now)
		if err != nil || entry == nil || entry.Author != "system" || len(entry.Parts) != 0 || len(events) == 0 {
			return fmt.Errorf("notice did not append a hidden callback: %v", err)
		}
		stored, err := readBody[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE session=?1 AND id=?2", session.ID.String(), entry.ID)
		if err != nil {
			return err
		}
		if stored == nil || stored.Kind != "background" || stored.Message() != nil || stored.Metadata.Input != nil || stored.Metadata.Runtime == nil || !stored.Metadata.Runtime.AgentVisible || stored.Metadata.Runtime.UserVisible {
			return errors.New("callback was not stored as agent-visible background evidence")
		}
		results := stored.Data.(*canonical.Background).Results
		var identities []string
		for _, result := range results {
			identities = append(identities, result.Notice.ID)
		}
		if !slices.Equal(identities, frozen) {
			return fmt.Errorf("notice settled %v, want %v", identities, frozen)
		}
		command, failed, succeeded := results[0], results[1], results[2]
		if command.Notice.Outcome != protocol.ConversationBackgroundOutcomeTimedOut || command.Stdout == nil || *command.Stdout != "command stdout" || command.Stderr == nil || *command.Stderr != "command stderr" || command.ElapsedMs == nil || *command.ElapsedMs != 12 {
			return errors.New("callback lost command timeout evidence")
		}
		child, ok := processStatusData[protocol.ConversationBackgroundKindAgentPayload](failed.Notice.Kind.Data)
		if !ok || child.Turn != failureTurn || failed.Notice.Outcome != protocol.ConversationBackgroundOutcomeFailed || failed.ErrorCode == nil || *failed.ErrorCode != protocol.ErrorCodeOutcomeUnknown || failed.Notice.Summary == nil || *failed.Notice.Summary != reason || succeeded.Response == nil || *succeeded.Response != "child response" {
			return errors.New("callback lost the exact child failure or success")
		}
		for _, id := range frozen {
			if err = check(id, protocol.ConversationBackgroundStateDelivered, &notice); err != nil {
				return err
			}
		}
		if exists, err = pending(db); err != nil || exists {
			return fmt.Errorf("callback did not settle its queue item: %v", err)
		}
		if repeated, events, err := DeliverBackground(ctx, db, notice, true, now); err != nil || repeated != nil || len(events) != 0 {
			return fmt.Errorf("initial delivery was not idempotent: %v", err)
		}
		if err = check(late, protocol.ConversationBackgroundStatePending, nil); err != nil {
			return err
		}
		if next, _, err := DeliverBackground(ctx, db, notice, false, now); err != nil || next == nil {
			return fmt.Errorf("normal model boundary did not pick up the later result: %v", err)
		}
		if err = check(late, protocol.ConversationBackgroundStateDelivered, &notice); err != nil {
			return err
		}
		if err = check(untouched, protocol.ConversationBackgroundStatePending, nil); err != nil {
			return err
		}
		history, err := ReadConversation(ctx, db, session.ID, nil, 20)
		if err != nil {
			return err
		}
		for _, entry := range history.Page.Entries {
			if entry.Turn == notice {
				return errors.New("automatic callback appeared in user history")
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestBackgroundCallbackInitialInputKeepsManualOnlyCommands(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		input := protocol.DefaultCommandRunCommandPayload()
		input.Turn, input.Command, input.Background, input.FollowUp = turn, "Fixture recovered command", true, true
		request := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
		if _, err := RegisterCommand(ctx, db, request, now); err != nil {
			return err
		}
		if _, err := RecoverCommands(ctx, db, now); err != nil {
			return err
		}
		if _, err := backgroundFixtureRecord(ctx, db, session, turn, "no-follow-up", false, now); err != nil {
			return err
		}
		stopped, err := backgroundFixtureRecord(ctx, db, session, turn, "explicitly-stopped", true, now)
		if err != nil {
			return err
		}
		stopped.Task.Outcome = new(protocol.ConversationBackgroundOutcomeStopped)
		if err = saveBackgroundRecord(ctx, db, &stopped); err != nil {
			return err
		}
		if entry, events, err := DeliverBackground(ctx, db, turn, false, now); err != nil || entry != nil || len(events) != 0 {
			return fmt.Errorf("running turn took manual-only evidence: %v", err)
		}
		if err = queueFixtureInput(ctx, db, session.ID, turn, now); err != nil {
			return err
		}
		entry, _, err := DeliverInputBackground(ctx, db, turn, now)
		if err != nil || entry == nil {
			return fmt.Errorf("explicit input lost its waiting command evidence: %v", err)
		}
		stored, err := readBody[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE session=?1 AND id=?2", session.ID.String(), entry.ID)
		if err != nil {
			return err
		}
		results := stored.Data.(*canonical.Background).Results
		if len(results) != 3 || results[0].Notice.ID != "command:"+request.ID.String() || results[0].Notice.Outcome != protocol.ConversationBackgroundOutcomeInterrupted || results[1].Notice.ID != "no-follow-up" || results[2].Notice.ID != stopped.Task.ID || results[2].Notice.Outcome != protocol.ConversationBackgroundOutcomeStopped {
			return fmt.Errorf("explicit input changed the waiting command outcomes: %+v", results)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
