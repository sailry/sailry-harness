package store

import (
	"context"
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestAutomaticRunNotices covers who a finished run's outcome notice is for:
// the user who started the turn or scheduled it, and anyone whose automatic
// chain broke. A step of a chain that succeeded raises none.
func TestAutomaticRunNotices(t *testing.T) {
	t.Parallel()
	failure := &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "model is unavailable"}
	lost := &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "execution was interrupted"}
	notice := func(run *protocol.ConversationRun) { run.Notice = true }
	continuation := func(run *protocol.ConversationRun) {
		run.Continuation = &protocol.ConversationContinuation{Package: protocol.PluginReference{Name: "goals"}}
	}
	asked := func(run *protocol.ConversationRun) {
		run.Continuation = &protocol.ConversationContinuation{Package: protocol.PluginReference{Name: "goals"}, User: true}
	}
	// schedule stores the dispatch job whose callback admitted the turn.
	schedule := func(ctx context.Context, db *sql.Conn, node protocol.NodeID, turn protocol.TurnID) error {
		admitted := protocol.DefaultQueuedTurn()
		admitted.ID, admitted.Kind, admitted.Revision = turn, protocol.ConversationRunKindTask, 1
		admitted.Config.Mode, admitted.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
		output := protocol.Output{Kind: "queued_turn", Data: admitted}
		result, err := encode(protocol.Result[protocol.Output, protocol.Fault]{Ok: &output})
		if err != nil {
			return err
		}
		request := protocol.NewRequestID()
		if _, err := db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,X'6E756C6C','completed',?3)", node[:], request.String(), string(result)); err != nil {
			return err
		}
		job := protocol.DefaultDispatchJob()
		job.ID, job.Handler, job.Queue, job.Request, job.Status = protocol.NewJobID(), "nightly", "default", request, protocol.DispatchStatusRunning
		body, err := encode(job)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO dispatch_jobs(id,package,queue,status,body,callback,reference) VALUES(?1,'scheduled-tasks','default','running',?2,X'6E756C6C',X'6E756C6C')", job.ID.String(), body)
		return err
	}
	for _, scenario := range []struct {
		name      string
		mark      func(*protocol.ConversationRun)
		scheduled bool
		status    protocol.ConversationStatus
		failure   *protocol.Fault
		noticed   bool
	}{
		{"user_turn_completing", nil, false, protocol.ConversationStatusCompleted, nil, true},
		{"user_turn_failing", nil, false, protocol.ConversationStatusFailed, failure, true},
		{"scheduled_turn_completing", nil, true, protocol.ConversationStatusCompleted, nil, true},
		{"scheduled_continuation_completing", continuation, true, protocol.ConversationStatusCompleted, nil, true},
		{"continuation_the_user_asked_for_completing", asked, false, protocol.ConversationStatusCompleted, nil, true},
		{"notice_turn_completing", notice, false, protocol.ConversationStatusCompleted, nil, false},
		{"notice_turn_failing", notice, false, protocol.ConversationStatusFailed, failure, true},
		{"notice_turn_stopped", notice, false, protocol.ConversationStatusCancelled, nil, false},
		{"continuation_mid_chain_completing", continuation, false, protocol.ConversationStatusCompleted, nil, false},
		{"last_automatic_turn_failing", continuation, false, protocol.ConversationStatusFailed, failure, true},
		{"last_automatic_turn_interrupted", continuation, false, protocol.ConversationStatusInterrupted, lost, true},
		{"continuation_stopped", continuation, false, protocol.ConversationStatusCancelled, nil, false},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			t.Parallel()
			storage, node, _, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				run, err := ReadAgentRun(ctx, db, turn)
				if err != nil {
					return err
				}
				if scenario.mark != nil {
					scenario.mark(&run)
					if err := putAgentRun(ctx, db, run); err != nil {
						return err
					}
				}
				if scenario.scheduled {
					if err := schedule(ctx, db, node, turn); err != nil {
						return err
					}
				}
				_, events, err := FinishTurn(ctx, db, turn, scenario.status, scenario.failure, now)
				if err != nil {
					return err
				}
				raised := 0
				for _, event := range events {
					if stored, ok := event.Data.(protocol.NotificationNotice); ok && event.Kind == "notification_changed" && stored.Content.Details != nil && stored.Content.Details.Kind == "task" {
						raised++
					}
				}
				stored, err := ReadNotifications(ctx, db)
				if err != nil {
					return err
				}
				kept := 0
				for _, current := range stored {
					if current.Content.Details != nil && current.Content.Details.Kind == "task" {
						kept++
					}
				}
				want := 0
				if scenario.noticed {
					want = 1
				}
				if raised != want || kept != want {
					return fmt.Errorf("raised %d and kept %d outcome notices, want %d", raised, kept, want)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
	t.Run("the_rule", func(t *testing.T) {
		t.Parallel()
		for _, status := range []protocol.ConversationStatus{protocol.ConversationStatusCompleted, protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted} {
			run := protocol.ConversationRun{Status: status, Notice: true}
			if runNoticed(run, true, true) {
				t.Fatalf("a delegated %s run was noticed", status)
			}
		}
		for _, status := range []protocol.ConversationStatus{protocol.ConversationStatusQueued, protocol.ConversationStatusRunning, protocol.ConversationStatusStopping, protocol.ConversationStatusCancelled} {
			if runNoticed(protocol.ConversationRun{Status: status}, false, true) {
				t.Fatalf("a %s run was noticed", status)
			}
		}
	})
}
