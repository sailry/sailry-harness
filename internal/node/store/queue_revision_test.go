package store

import (
	"database/sql"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestQueueRevisionAfterImmediateStop(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name       string
		recorded   bool
		nextBefore bool
		unknown    bool
	}{
		{name: "unrecorded input"},
		{name: "recorded input", recorded: true},
		{name: "next submission before settlement", nextBefore: true},
		{name: "recorded input with next submission", recorded: true, nextBefore: true},
		{name: "uncertain stop outcome", unknown: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				if test.recorded {
					if err := queueFixtureInput(ctx, db, session.ID, turn, now); err != nil {
						return err
					}
				}
				if _, err := queueFixtureCommand(ctx, db, storage, node, "stop_turn", &protocol.CommandStopTurnPayload{Turn: turn}, now); err != nil {
					return err
				}
				submit := func() (protocol.TurnID, error) {
					output, err := queueFixtureCommand(ctx, db, storage, node, "submit_turn", &protocol.CommandSubmitTurnPayload{
						Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Next input"},
					}, now)
					if err != nil {
						return protocol.TurnID{}, err
					}
					return output.Data.(protocol.QueuedTurn).ID, nil
				}
				var next protocol.TurnID
				if test.nextBefore {
					var err error
					if next, err = submit(); err != nil {
						return err
					}
				}
				before, item, err := queueFixtureItem(ctx, db, session.ID, turn)
				if err != nil || item != nil {
					return fmt.Errorf("stopping input was visible in the queue: %+v, %v", item, err)
				}
				var failure *protocol.Fault
				if test.unknown {
					failure = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "tool completion could not be confirmed"}
				}
				run, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCancelled, failure, now)
				if err != nil || run.Status != protocol.ConversationStatusCancelled || !reflect.DeepEqual(run.Error, failure) {
					return fmt.Errorf("stop lost its outcome: %+v, %v", run, err)
				}
				after, retained, err := queueFixtureItem(ctx, db, session.ID, turn)
				if err != nil {
					return err
				}
				if after.Revision <= before.Revision || after.Paused != before.Paused || !reflect.DeepEqual(after.PauseReason, before.PauseReason) {
					return fmt.Errorf("settlement did not version the queue without undoing a later resume: before %+v, after %+v", before, after)
				}
				if (retained == nil) != test.recorded || retained != nil && retained.Ready {
					return fmt.Errorf("stopped input retention changed: %+v", retained)
				}
				if retained != nil {
					message, err := readQueuedMessage(ctx, db, turn)
					if err != nil || message.Message.Text != "Fixture input" {
						return fmt.Errorf("stopped input was lost: %+v, %v", message, err)
					}
				}
				_, err = queueFixtureCommand(ctx, db, storage, node, "set_queue_paused", &protocol.CommandSetQueuePausedPayload{
					Session: session.ID, ExpectedRevision: before.Revision, Paused: !after.Paused,
				}, now)
				if refusal, ok := errors.AsType[*protocol.Fault](err); !ok || refusal.Code != protocol.ErrorCodeRevisionConflict || refusal.Message != queueRevisionChanged.Message {
					return fmt.Errorf("stale queue mutation was not rejected: %v", err)
				}
				unchanged, err := ReadQueue(ctx, db, session.ID)
				if err != nil || !reflect.DeepEqual(unchanged, after) {
					return fmt.Errorf("rejected queue mutation changed state: %+v, %v", unchanged, err)
				}
				if !test.nextBefore {
					if next, err = submit(); err != nil {
						return err
					}
				}
				invocation, _, err := ClaimNext(ctx, db, now)
				if err != nil || invocation == nil || invocation.Run.Turn != next || invocation.Message.Text != "Next input" {
					return fmt.Errorf("next submission replayed the stopped input or failed to start: %+v, %v", invocation, err)
				}
				if err = queueFixtureInput(ctx, db, session.ID, next, now); err != nil {
					return err
				}
				if _, _, err = FinishTurn(ctx, db, next, protocol.ConversationStatusCompleted, nil, now); err != nil {
					return err
				}
				if invocation, _, err = ClaimNext(ctx, db, now); err != nil || invocation != nil {
					return fmt.Errorf("retained input replayed without an explicit send: %+v, %v", invocation, err)
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestQueueRevisionFailureRollsBackSettlement(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var before protocol.ConversationQueue
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := queueFixtureCommand(ctx, db, storage, node, "stop_turn", &protocol.CommandStopTurnPayload{Turn: turn}, now); err != nil {
			return err
		}
		var err error
		if before, err = ReadQueue(ctx, db, session.ID); err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "CREATE TRIGGER reject_queue_revision BEFORE UPDATE OF revision ON agent_queues BEGIN SELECT RAISE(ABORT, 'fixture queue revision failure'); END")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCancelled, nil, now)
		return err
	})
	if err == nil || !strings.Contains(err.Error(), "fixture queue revision failure") {
		t.Fatalf("settlement swallowed the queue write failure: %v", err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		run, err := ReadAgentRun(ctx, db, turn)
		if err != nil || run.Status != protocol.ConversationStatusStopping || run.FinishedMs != nil {
			return fmt.Errorf("failed queue write committed settlement: %+v, %v", run, err)
		}
		after, err := ReadQueue(ctx, db, session.ID)
		if err != nil || !reflect.DeepEqual(after, before) {
			return fmt.Errorf("failed settlement changed the queue: %+v, %v", after, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
