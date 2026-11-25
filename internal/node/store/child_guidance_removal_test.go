package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// guidedChild delegates a child of the running parent turn, claims it and
// admits guidance addressed to it.
func guidedChild(ctx context.Context, db *sql.Conn, storage *Store, node protocol.NodeID, parentTurn protocol.TurnID, now time.Time) (AgentInvocation, protocol.TurnID, error) {
	call, err := delegationFixtureCall(ctx, db, parentTurn, "spawn_agent", map[string]any{"task": "Child task"}, now)
	if err != nil {
		return AgentInvocation{}, protocol.TurnID{}, err
	}
	child, _, err := DelegateAgent(ctx, db, node, parentTurn, call, nil, true, now)
	if err != nil {
		return child, protocol.TurnID{}, err
	}
	inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now)
	if err != nil || inv == nil {
		return child, protocol.TurnID{}, fmt.Errorf("child was not claimed: %v", err)
	}
	if err = queueFixtureInput(ctx, db, child.Session.ID, child.Run.Turn, now); err != nil {
		return child, protocol.TurnID{}, err
	}
	input := protocol.DefaultConversationInput()
	input.Text = "Direction the child never took"
	output, err := queueFixtureCommand(ctx, db, storage, node, "guide_turn", &protocol.CommandGuideTurnPayload{Recipient: child.Run.Turn, ExpectedRevision: inv.Run.Revision, Message: input}, now)
	if err != nil {
		return child, protocol.TurnID{}, err
	}
	return child, output.Data.(protocol.QueuedTurn).ID, nil
}

// childGuidanceFixture admits guidance for a running child, then ends the child
// failed or stopped, which pauses its queue, and completes the parent.
func childGuidanceFixture(t *testing.T, outcome string) (*Store, protocol.NodeID, protocol.Session, AgentInvocation, protocol.TurnID, time.Time) {
	t.Helper()
	storage, node, parent, parentTurn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var child AgentInvocation
	var guidance protocol.TurnID
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		if child, guidance, err = guidedChild(ctx, db, storage, node, parentTurn, now); err != nil {
			return err
		}
		switch outcome {
		case "failed":
			_, _, err = FinishTurn(ctx, db, child.Run.Turn, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "model failed"}, now)
		case "stopped":
			if _, err = queueFixtureCommand(ctx, db, storage, node, "stop_turn", &protocol.CommandStopTurnPayload{Turn: child.Run.Turn}, now); err != nil {
				return err
			}
			_, _, err = FinishTurn(ctx, db, child.Run.Turn, protocol.ConversationStatusCancelled, nil, now)
		}
		if err != nil {
			return err
		}
		if queue, err := ReadQueue(ctx, db, child.Session.ID); err != nil || !queue.Paused {
			return fmt.Errorf("the ended child did not hold its queue: %+v: %v", queue, err)
		}
		_, _, err = FinishTurn(ctx, db, parentTurn, protocol.ConversationStatusCompleted, nil, now)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return storage, node, parent, child, guidance, now
}

// restoreRetainedGuidance rewrites the guidance into the state earlier
// versions stored: queued and ready with an unaddressed pending row in the
// child's paused queue, and no events.
func restoreRetainedGuidance(ctx context.Context, db *sql.Conn, guidance protocol.TurnID) error {
	if _, err := db.ExecContext(ctx, "UPDATE agent_runs SET ready=1,body=json_set(json_remove(body,'$.finished_ms'),'$.status','queued') WHERE turn=?1", guidance.String()); err != nil {
		return err
	}
	_, err := db.ExecContext(ctx, "INSERT INTO agent_pending(turn,position,revision,request,caller) SELECT id,1,2,request,caller FROM turns WHERE id=?1", guidance.String())
	return err
}

// assertGuidanceSettled checks that the guidance can never be admitted, even
// if the child's queue resumes.
func assertGuidanceSettled(ctx context.Context, db *sql.Conn, child AgentInvocation, guidance protocol.TurnID, now time.Time) error {
	run, err := ReadAgentRun(ctx, db, guidance)
	if err != nil {
		return err
	}
	if run.Status != protocol.ConversationStatusCancelled || run.FinishedMs == nil {
		return fmt.Errorf("guidance did not end cancelled: %+v", run)
	}
	var lingering bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_pending WHERE turn=?1)", guidance.String()).Scan(&lingering); err != nil || lingering {
		return fmt.Errorf("guidance stayed in a queue: %v", err)
	}
	if pending, err := PendingGuidance(ctx, db, child.Run.Turn); err != nil || pending {
		return fmt.Errorf("guidance stayed addressed to the child: %v", err)
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_queues SET paused=0,pause_reason=NULL WHERE session=?1", child.Session.ID.String()); err != nil {
		return err
	}
	if next, _, err := ClaimNext(ctx, db, now); err != nil || next != nil {
		return fmt.Errorf("a resumed queue claimed %+v: %v", next, err)
	}
	if next, _, err := ClaimChild(ctx, db, guidance, now); err != nil || next != nil {
		return fmt.Errorf("the child scheduler claimed %+v: %v", next, err)
	}
	return nil
}

func TestEndedChildSettlesGuidanceItNeverJoined(t *testing.T) {
	t.Parallel()
	for _, outcome := range []string{"failed", "stopped"} {
		t.Run(outcome, func(t *testing.T) {
			t.Parallel()
			storage, _, _, child, guidance, now := childGuidanceFixture(t, outcome)
			ctx := t.Context()
			if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				if queue, err := ReadQueue(ctx, db, child.Session.ID); err != nil || len(queue.Items) != 0 {
					return fmt.Errorf("child queue kept items %+v: %v", queue.Items, err)
				}
				return assertGuidanceSettled(ctx, db, child, guidance, now)
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}

// The child's last activity reports the queue its end leaves, without the
// guidance that ending cancelled.
func TestEndedChildReportsItsSettledQueue(t *testing.T) {
	t.Parallel()
	storage, node, _, parentTurn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		child, _, err := guidedChild(ctx, db, storage, node, parentTurn, now)
		if err != nil {
			return err
		}
		_, events, err := FinishTurn(ctx, db, child.Run.Turn, protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "model failed"}, now)
		if err != nil {
			return err
		}
		var reported *protocol.ActivitySummary
		for _, event := range events {
			if value, ok := event.Data.(protocol.EventConversationChangedPayload); ok && value.Session == child.Session.ID {
				reported = value.Activity
			}
		}
		if reported == nil || reported.Queued != 0 {
			return fmt.Errorf("the ended child reported activity %+v", reported)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

// A completed child hands its next guidance to that guidance's own turn,
// which only the in-memory owner claims. A restart before the claim leaves
// no owner, so recovery settles it.
func TestRecoverySettlesGuidanceHandedOnByACompletedChild(t *testing.T) {
	t.Parallel()
	storage, node, _, parentTurn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		child, guidance, err := guidedChild(ctx, db, storage, node, parentTurn, now)
		if err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, child.Run.Turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		next, _, err := NextChildGuidance(ctx, db, child.Run.Turn)
		if err != nil || next == nil || next.Turn != guidance {
			return fmt.Errorf("the completed child handed on %+v: %v", next, err)
		}
		if _, err = RecoverRuns(ctx, db, now); err != nil {
			return err
		}
		return assertGuidanceSettled(ctx, db, child, guidance, now)
	}); err != nil {
		t.Fatal(err)
	}
}

func TestRemovalDiscardsRetainedChildGuidance(t *testing.T) {
	t.Parallel()
	for _, outcome := range []string{"failed", "stopped"} {
		for _, heal := range []string{"removal", "recovery"} {
			t.Run(outcome+" "+heal, func(t *testing.T) {
				t.Parallel()
				storage, node, parent, child, guidance, now := childGuidanceFixture(t, outcome)
				ctx := t.Context()
				var other protocol.Session
				if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
					if err := restoreRetainedGuidance(ctx, db, guidance); err != nil {
						return err
					}
					out, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &parent.Config})
					if err != nil {
						return err
					}
					other = out.Data.(protocol.Session)
					var busy bool
					if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))", child.Session.ID.String()).Scan(&busy); err != nil || !busy {
						return fmt.Errorf("fixture is not the stored stuck state: %v", err)
					}
					if heal == "recovery" {
						if _, err = RecoverRuns(ctx, db, now); err != nil {
							return err
						}
						if err = assertGuidanceSettled(ctx, db, child, guidance, now); err != nil {
							return err
						}
					}
					current, err := ReadSession(ctx, db, parent.ID)
					if err != nil {
						return err
					}
					request := protocol.NewRequest(node, protocol.Command{Kind: "remove_sessions", Data: &protocol.CommandRemoveSessionsPayload{Expected: []protocol.SessionRevision{
						{Session: parent.ID, Revision: current.Revision}, {Session: other.ID, Revision: other.Revision},
					}}})
					output, _, err := ExecuteCommand(ctx, db, node, node, storage.Profile, request, PreparedPaths{})
					if err != nil {
						return fmt.Errorf("removal failed: %w", err)
					}
					if ids, ok := output.Data.([]protocol.SessionID); !ok || len(ids) != 3 {
						return fmt.Errorf("removed %v, want the parent, its child and the other session", output.Data)
					}
					return assertGuidanceSettled(ctx, db, child, guidance, now)
				}); err != nil {
					t.Fatal(err)
				}
			})
		}
	}
}

func TestSessionRemovalStopsRunningTurns(t *testing.T) {
	t.Parallel()
	storage, node, running, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var finished protocol.Session
	var command protocol.Command
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		out, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &running.Config})
		if err != nil {
			return err
		}
		finished = out.Data.(protocol.Session)
		current, err := ReadSession(ctx, db, running.ID)
		if err != nil {
			return err
		}
		command = protocol.Command{Kind: "remove_sessions", Data: &protocol.CommandRemoveSessionsPayload{Expected: []protocol.SessionRevision{
			{Session: running.ID, Revision: current.Revision}, {Session: finished.ID, Revision: finished.Revision},
		}}}
		request := protocol.NewRequest(node, command)
		if _, _, err = ExecuteCommand(ctx, db, node, node, storage.Profile, request, PreparedPaths{}); !providerFixtureFault(err, protocol.ErrorCodeBusy) || !errors.Is(err, ErrSessionRemovalActive) {
			return fmt.Errorf("removal of a running session = %v, want busy", err)
		}
		stale := protocol.Command{Kind: "remove_sessions", Data: &protocol.CommandRemoveSessionsPayload{Expected: []protocol.SessionRevision{
			{Session: running.ID, Revision: current.Revision + 1}, {Session: finished.ID, Revision: finished.Revision},
		}}}
		if stopped, _, err := StopSessionRemoval(ctx, db, stale, now); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) || len(stopped) != 0 {
			return fmt.Errorf("a stale removal stopped %v: %v", stopped, err)
		}
		if run, err := ReadAgentRun(ctx, db, turn); err != nil || run.Status != protocol.ConversationStatusRunning {
			return fmt.Errorf("a stale removal changed the turn: %+v: %v", run, err)
		}
		stopped, events, err := StopSessionRemoval(ctx, db, command, now)
		if err != nil {
			return err
		}
		if len(stopped) != 1 || stopped[0].Turn != turn || stopped[0].Status != protocol.ConversationStatusStopping || len(events) == 0 {
			return fmt.Errorf("stop for removal = %+v events=%d", stopped, len(events))
		}
		// A stopping turn is not stopped again, and still holds the removal.
		if again, _, err := StopSessionRemoval(ctx, db, command, now); err != nil || len(again) != 0 {
			return fmt.Errorf("second stop = %+v: %v", again, err)
		}
		if active, err := ActiveSessionRemoval(ctx, db, command); err != nil || len(active) != 1 || active[0].Status != protocol.ConversationStatusStopping {
			return fmt.Errorf("a stopping turn did not hold the removal: %+v: %v", active, err)
		}
		if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		if run, err := ReadAgentRun(ctx, db, turn); err != nil || run.Status != protocol.ConversationStatusCancelled {
			return fmt.Errorf("the stopped turn ended %+v: %v", run, err)
		}
		if active, err := ActiveSessionRemoval(ctx, db, command); err != nil || len(active) != 0 {
			return fmt.Errorf("a settled turn still held the removal: %v", err)
		}
		output, _, err := ExecuteCommand(ctx, db, node, node, storage.Profile, protocol.NewRequest(node, command), PreparedPaths{})
		if err != nil {
			return err
		}
		if ids, ok := output.Data.([]protocol.SessionID); !ok || len(ids) != 2 {
			return fmt.Errorf("removed %v", output.Data)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
