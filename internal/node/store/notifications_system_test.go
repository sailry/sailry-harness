package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Each scenario opens its own synced store beside the package's other parallel tests.
func TestRunOutcomeNotices(t *testing.T) {
	t.Parallel()
	taskNotice := func(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (*protocol.NotificationNotice, *protocol.NotificationDetailsTask, error) {
		notices, err := ReadNotifications(ctx, db)
		if err != nil {
			return nil, nil, err
		}
		var found *protocol.NotificationNotice
		var details *protocol.NotificationDetailsTask
		for index, notice := range notices {
			if notice.Content.Details == nil || notice.Content.Details.Kind != "task" {
				continue
			}
			task, valid := dispatchValue[protocol.NotificationDetailsTask](notice.Content.Details.Data)
			if !valid || task.Turn != turn {
				continue
			}
			if found != nil {
				return nil, nil, errors.New("one run raised more than one outcome notice")
			}
			found, details = &notices[index], &task
		}
		return found, details, nil
	}
	changed := func(events []protocol.Event) int {
		count := 0
		for _, event := range events {
			if event.Kind == "notification_changed" {
				count++
			}
		}
		return count
	}
	for _, scenario := range []struct {
		name    string
		status  protocol.ConversationStatus
		failure *protocol.Fault
		kind    protocol.NotificationKind
		noticed bool
	}{
		{"completed_run_raises_a_success_notice", protocol.ConversationStatusCompleted, nil, protocol.NotificationKindSuccess, true},
		{"failed_run_raises_an_error_notice_with_its_code", protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "model is unavailable"}, protocol.NotificationKindError, true},
		{"interrupted_run_raises_an_error_notice", protocol.ConversationStatusInterrupted, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "execution was interrupted"}, protocol.NotificationKindError, true},
		{"cancelled_run_raises_none", protocol.ConversationStatusCancelled, nil, "", false},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			t.Parallel()
			storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				run, events, err := FinishTurn(ctx, db, turn, scenario.status, scenario.failure, now)
				if err != nil {
					return err
				}
				notice, task, err := taskNotice(ctx, db, turn)
				if err != nil {
					return err
				}
				if !scenario.noticed {
					if notice != nil || changed(events) != 0 {
						return fmt.Errorf("%s run raised a notice", run.Status)
					}
					return nil
				}
				if notice == nil || changed(events) != 1 {
					return fmt.Errorf("%s run raised no notice: %d events", run.Status, changed(events))
				}
				content := notice.Content
				if content.Kind != scenario.kind || content.Session == nil || *content.Session != session.ID || notice.Package != "" || notice.Read ||
					content.Title == "" || notice.TimestampMs != now.UnixMilli() {
					return fmt.Errorf("outcome notice lost its run: %+v", notice)
				}
				if task.Status != run.Status || task.FinishedMs == nil || *task.FinishedMs != now.UnixMilli() || task.StartedMs == nil ||
					(task.Error == nil) != (scenario.failure == nil) || task.Error != nil && *task.Error != scenario.failure.Code {
					return fmt.Errorf("outcome details: %+v", task)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
	t.Run("restart_recovery_reports_the_interrupted_run", func(t *testing.T) {
		t.Parallel()
		storage, _, _, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := t.Context()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			events, err := RecoverRuns(ctx, db, now)
			if err != nil {
				return err
			}
			notice, task, err := taskNotice(ctx, db, turn)
			if err != nil {
				return err
			}
			if notice == nil || changed(events) != 1 || task.Status != protocol.ConversationStatusInterrupted || task.Error == nil || *task.Error != protocol.ErrorCodeOutcomeUnknown {
				return fmt.Errorf("recovered run was not reported: %+v", notice)
			}
			if again, err := RecoverRuns(ctx, db, now); err != nil || changed(again) != 0 {
				return fmt.Errorf("second recovery repeated the notice: %v", err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("delegated_run_raises_none", func(t *testing.T) {
		t.Parallel()
		storage, _, session, turn, _ := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := t.Context()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			run, err := ReadAgentRun(ctx, db, turn)
			if err != nil {
				return err
			}
			run.Status, run.FinishedMs = protocol.ConversationStatusCompleted, run.StartedMs
			for _, delegated := range []bool{false, true} {
				if delegated {
					body, err := encode(protocol.Delegation{Session: protocol.NewSessionID(), Turn: protocol.NewTurnID()})
					if err != nil {
						return err
					}
					if _, err := db.ExecContext(ctx, "INSERT INTO agent_delegations(turn,parent_turn,entry,part,body) VALUES(?1,?2,'entry',0,?3)", turn.String(), turn.String(), body); err != nil {
						return err
					}
				}
				events, err := runNotice(ctx, db, run)
				if err != nil {
					return err
				}
				if (len(events) == 0) != delegated {
					return fmt.Errorf("delegated=%v run raised %d notices in session %s", delegated, len(events), session.ID)
				}
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("explained_failure_is_not_reported_twice", func(t *testing.T) {
		t.Parallel()
		storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := t.Context()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			run, err := ReadAgentRun(ctx, db, turn)
			if err != nil {
				return err
			}
			ms := now.UnixMilli()
			incident := protocol.NewRequestID()
			report := protocol.DefaultNotificationSystemSystemDiagnostic()
			report.Incident, report.Source, report.Phase = incident, protocol.NotificationSystemSystemSourcePluginCallback, protocol.NotificationSystemDiagnosticPhaseCallback
			report.Reason, report.Code = protocol.NotificationSystemDiagnosticReasonFailure, protocol.ErrorCodeInternal
			report.Session, report.Turn, report.Version, report.Platform = &session.ID, &turn, "0.1.0", "test-any"
			run.Status, run.FinishedMs = protocol.ConversationStatusFailed, &ms
			run.Error = &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "callback failed", Details: &protocol.FaultDetails{Kind: "system_diagnostic", Data: &report}}
			if events, err := runNotice(ctx, db, run); err != nil || len(events) != 1 {
				return fmt.Errorf("unreported incident raised %d notices: %v", len(events), err)
			}
			if _, err := RecordPluginStatusDiagnostic(ctx, db, report, ms); err != nil {
				return err
			}
			if events, err := runNotice(ctx, db, run); err != nil || len(events) != 0 {
				return fmt.Errorf("a failure its diagnostic reported raised %d notices: %v", len(events), err)
			}
			run.Status, run.Error = protocol.ConversationStatusCompleted, nil
			if events, err := runNotice(ctx, db, run); err != nil || len(events) != 1 {
				return fmt.Errorf("a later success was suppressed: %v", err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("compaction_failure_raises_a_diagnostic_and_explains_its_stop", func(t *testing.T) {
		t.Parallel()
		storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := t.Context()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			append := func(author string, continued bool) ([]protocol.Event, protocol.ConversationEntry, error) {
				record := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Compaction failed")), now)
				record.Provenance.Author = author
				record.Metadata.Compaction = &canonical.Maintenance{Failure: &protocol.ConversationCompactionFailure{
					Source: protocol.ConversationCompactionSourceAutomatic, Stage: protocol.ConversationCompactionStageRequest,
					Reason: protocol.ConversationCompactionReasonRequestFailed, Continued: continued,
				}}
				entry, events, err := AppendCanonical(ctx, db, session.ID, record)
				return events, entry, err
			}
			if events, _, err := append("assistant", false); err != nil || changed(events) != 0 {
				return fmt.Errorf("a record the system did not write raised a diagnostic: %v", err)
			}
			events, entry, err := append("system", true)
			if err != nil {
				return err
			}
			if changed(events) != 1 {
				return errors.New("compaction failure raised no diagnostic")
			}
			notices, err := ReadNotifications(ctx, db)
			if err != nil {
				return err
			}
			diagnostic, valid := dispatchValue[protocol.NotificationDetailsDiagnostic](notices[0].Content.Details.Data)
			if !valid || notices[0].Content.Kind != protocol.NotificationKindError || notices[0].Content.Session == nil || *notices[0].Content.Session != session.ID ||
				diagnostic.Event != entry.ID || diagnostic.Sequence != entry.Sequence || diagnostic.Turn != turn || diagnostic.Cause.Kind != "compaction" || diagnostic.Version == "" || diagnostic.Platform == "" {
				return fmt.Errorf("compaction diagnostic: %+v", notices[0])
			}
			run, err := ReadAgentRun(ctx, db, turn)
			if err != nil {
				return err
			}
			run.Status, run.Error = protocol.ConversationStatusFailed, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "compaction failed"}
			if events, err := runNotice(ctx, db, run); err != nil || len(events) != 1 {
				return fmt.Errorf("a continued compaction failure hid the later outcome: %v", err)
			}
			if _, _, err := append("system", false); err != nil {
				return err
			}
			if events, err := runNotice(ctx, db, run); err != nil || len(events) != 0 {
				return fmt.Errorf("the compaction failure that stopped the turn was reported twice: %v", err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
}
