package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestQueuePauseReason covers the reason a paused queue reports to clients.
func TestQueuePauseReason(t *testing.T) {
	t.Parallel()
	for _, row := range []struct {
		name string
		want protocol.ConversationQueuePause
	}{
		{"stop", protocol.ConversationQueuePauseStopped},
		{"user pause", protocol.ConversationQueuePauseUser},
		{"input not recorded", protocol.ConversationQueuePauseUnrecorded},
		{"lost owner", protocol.ConversationQueuePauseInterrupted},
	} {
		t.Run(row.name, func(t *testing.T) {
			t.Parallel()
			storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := context.Background()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				if row.name != "input not recorded" {
					if err := queueFixtureInput(ctx, db, session.ID, turn, now); err != nil {
						return err
					}
				}
				switch row.name {
				case "stop":
					if _, err := queueFixtureCommand(ctx, db, storage, node, "stop_turn", &protocol.CommandStopTurnPayload{Turn: turn}, now); err != nil {
						return err
					}
					if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCancelled, nil, now); err != nil {
						return err
					}
				case "user pause":
					if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					queue, err := ReadQueue(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if queue.Paused || queue.PauseReason != nil {
						return errors.New("running queue reports a pause")
					}
					if _, err = queueFixtureCommand(ctx, db, storage, node, "set_queue_paused", &protocol.CommandSetQueuePausedPayload{Session: session.ID, ExpectedRevision: queue.Revision, Paused: true}, now); err != nil {
						return err
					}
				case "input not recorded":
					if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "provider is unavailable"}, now); err != nil {
						return err
					}
				default:
					if _, err := RecoverRuns(ctx, db, now); err != nil {
						return err
					}
				}
				queue, err := ReadQueue(ctx, db, session.ID)
				if err != nil {
					return err
				}
				if !queue.Paused || queue.PauseReason == nil || *queue.PauseReason != row.want {
					return fmt.Errorf("paused %v for %v, want %s", queue.Paused, queue.PauseReason, row.want)
				}
				// Resuming clears the reason with the pause.
				if _, err = queueFixtureCommand(ctx, db, storage, node, "set_queue_paused", &protocol.CommandSetQueuePausedPayload{Session: session.ID, ExpectedRevision: queue.Revision, Paused: false}, now); err != nil {
					return err
				}
				if queue, err = ReadQueue(ctx, db, session.ID); err != nil {
					return err
				}
				if queue.Paused || queue.PauseReason != nil {
					return errors.New("resumed queue kept its pause reason")
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

// A turn that resumes the queue leaves no reason in its row for a later pause
// to show.
func TestResumedQueueRow(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		if err := pauseQueue(ctx, db, session.ID, protocol.ConversationQueuePauseStopped); err != nil {
			return err
		}
		current, err := ReadSession(ctx, db, session.ID)
		if err != nil {
			return err
		}
		if _, err = delegationFixtureParent(ctx, db, node, current, now); err != nil {
			return err
		}
		var paused bool
		var reason sql.NullString
		if err = db.QueryRowContext(ctx, "SELECT paused,pause_reason FROM agent_queues WHERE session=?1", session.ID.String()).Scan(&paused, &reason); err != nil {
			return err
		}
		if paused || reason.Valid {
			return fmt.Errorf("resumed queue row: paused %v, reason %q", paused, reason.String)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

// TestStalledDelivery covers stall evidence under the delivery rules: one
// notice per watched command, counted in the automatic chain, and none for
// a session the user stopped.
func TestStalledDelivery(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	input := protocol.DefaultCommandRunCommandPayload()
	input.Turn, input.Command, input.Background, input.FollowUp = turn, "fixture server", true, true
	command := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
	later := protocol.NewRequest(node, protocol.Command{Kind: "run_command", Data: &input})
	id := "stall:" + command.ID.String()
	stall := func(db *sql.Conn, after time.Duration) (bool, error) {
		stalled := process.Stalled{Snapshot: protocol.ProcessSnapshot{Info: protocol.ProcessInfo{ID: command.ID, Session: session.ID, Turn: turn, Status: protocol.ProcessStatus{Kind: "running"}}}, ElapsedMs: uint64(after.Milliseconds())}
		_, written, err := StallCommand(ctx, db, stalled, now.Add(after))
		return written, err
	}
	notice := func(db *sql.Conn) (*AgentInvocation, error) {
		next, _, err := ClaimNext(ctx, db, now)
		if err != nil || next == nil {
			return nil, err
		}
		if !next.Run.Notice || len(next.Run.Notices) != 1 || next.Run.Notices[0].ID != id || next.Run.Notices[0].Outcome != protocol.ConversationBackgroundOutcomeStalled {
			return nil, fmt.Errorf("claim was not the stall's notice turn: %+v", next.Run)
		}
		return next, nil
	}
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := queueFixtureInput(ctx, db, session.ID, turn, now); err != nil {
			return err
		}
		if _, err := RegisterCommand(ctx, db, command, now); err != nil {
			return err
		}
		if _, err := RegisterCommand(ctx, db, later, now); err != nil {
			return err
		}
		if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		if written, err := stall(db, 15*time.Minute); err != nil || !written {
			return fmt.Errorf("silent command recorded no stall: %v", err)
		}
		first, err := notice(db)
		if err != nil || first == nil {
			return fmt.Errorf("stall started no notice turn: %v", err)
		}
		// Evidence reserved for a notice turn is not rewritten under it.
		if written, err := stall(db, 20*time.Minute); err != nil || written {
			return fmt.Errorf("reserved stall evidence was rewritten: %v", err)
		}
		if entry, _, err := DeliverBackground(ctx, db, first.Run.Turn, true, now); err != nil || entry == nil {
			return fmt.Errorf("notice turn did not take its reservation: %v", err)
		}
		if _, _, err = FinishTurn(ctx, db, first.Run.Turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		// One stall period wakes the agent once.
		if again, err := notice(db); err != nil || again != nil {
			return fmt.Errorf("delivered stall started another notice turn: %v", err)
		}
		if written, err := stall(db, 30*time.Minute); err != nil || written {
			return fmt.Errorf("another silent period repeated a delivered stall: %v", err)
		}
		command, id = later, "stall:"+later.ID.String()
		if written, err := stall(db, 30*time.Minute); err != nil || !written {
			return fmt.Errorf("another command recorded no first stall: %v", err)
		}
		second, err := notice(db)
		if err != nil || second == nil {
			return fmt.Errorf("next stall period started no notice turn: %v", err)
		}
		// Stall notice turns count toward the automatic chain limit.
		if chain, err := ReadAutomaticChain(ctx, db, session.ID); err != nil || chain.Count != 2 {
			return fmt.Errorf("automatic chain counts %d stall turns: %v", chain.Count, err)
		}
		// The user stops the notice turn: later stalls wait for the user.
		if _, err = queueFixtureCommand(ctx, db, storage, node, "stop_turn", &protocol.CommandStopTurnPayload{Turn: second.Run.Turn}, now); err != nil {
			return err
		}
		if entry, events, err := DeliverBackground(ctx, db, second.Run.Turn, true, now); !errors.Is(err, context.Canceled) || entry != nil || len(events) != 0 {
			return fmt.Errorf("notice appended after Stop: %v", err)
		}
		if _, _, err = FinishTurn(ctx, db, second.Run.Turn, protocol.ConversationStatusCancelled, nil, now); err != nil {
			return err
		}
		// The held command is checked again later instead of ending observation.
		if written, err := stall(db, 45*time.Minute); !errors.Is(err, ErrCommandHeld) || written {
			return fmt.Errorf("stall after the stop replaced held evidence: %v", err)
		}
		record, err := readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", id)
		if err != nil {
			return err
		}
		if record.Task.State != protocol.ConversationBackgroundStatePending || !record.Task.Suppressed || record.Task.Delivery != nil || record.Task.Hold == nil || *record.Task.Hold != protocol.ConversationBackgroundHoldStopped {
			return fmt.Errorf("stopped stall evidence: %+v", record.Task)
		}
		queue, err := ReadQueue(ctx, db, session.ID)
		if err != nil {
			return err
		}
		if !queue.Paused || queue.PauseReason == nil || *queue.PauseReason != protocol.ConversationQueuePauseStopped {
			return errors.New("stopped session's queue is not paused as stopped")
		}
		// Even a resumed queue starts nothing for a held result.
		if err = setQueuePaused(ctx, db, session.ID, false); err != nil {
			return err
		}
		if next, err := notice(db); err != nil || next != nil {
			return fmt.Errorf("stall woke a session the user stopped: %v", err)
		}
		// The user's next message takes the held evidence and clears the hold.
		current, err := ReadSession(ctx, db, session.ID)
		if err != nil {
			return err
		}
		user, err := delegationFixtureParent(ctx, db, node, current, now)
		if err != nil {
			return err
		}
		if err = queueFixtureInput(ctx, db, session.ID, user, now); err != nil {
			return err
		}
		if entry, _, err := DeliverInputBackground(ctx, db, user, now); err != nil || entry == nil {
			return fmt.Errorf("user turn did not take the held stall: %v", err)
		}
		if record, err = readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", id); err != nil {
			return err
		}
		if record.Task.State != protocol.ConversationBackgroundStateDelivered || record.Task.Hold != nil || record.Task.Delivery == nil || *record.Task.Delivery != user {
			return fmt.Errorf("taken stall kept its hold: %+v", record.Task)
		}
		// A later model failure cannot un-deliver committed evidence.
		revision := record.Task.Revision
		if _, _, err = FinishTurn(ctx, db, user, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "model is unavailable"}, now); err != nil {
			return err
		}
		if record, err = readBody[BackgroundRecord](ctx, db, "SELECT body FROM agent_background WHERE id=?1", id); err != nil {
			return err
		}
		if record.Task.Revision != revision || record.Task.State != protocol.ConversationBackgroundStateDelivered || record.Task.Delivery == nil || *record.Task.Delivery != user || record.Task.Hold != nil {
			return fmt.Errorf("failed turn altered delivered stall: %+v", record.Task)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
