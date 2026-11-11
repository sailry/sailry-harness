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

// A scheduled run has no connected controller to derive its attention from.
func TestDispatchNotices(t *testing.T) {
	t.Parallel()
	notices := func(ctx context.Context, db *sql.Conn, kind string) ([]protocol.NotificationNotice, error) {
		all, err := ReadNotifications(ctx, db)
		if err != nil {
			return nil, err
		}
		found := []protocol.NotificationNotice{}
		for _, notice := range all {
			if notice.Content.Details != nil && notice.Content.Details.Kind == kind {
				found = append(found, notice)
			}
		}
		return found, nil
	}
	// job stores a running turn-completion job whose callback admitted turn.
	job := func(ctx context.Context, db *sql.Conn, node protocol.NodeID, turn protocol.TurnID) error {
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
		running := protocol.DefaultDispatchJob()
		running.ID, running.Handler, running.Queue, running.Request, running.Status = protocol.NewJobID(), "nightly", "default", request, protocol.DispatchStatusRunning
		body, err := encode(running)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO dispatch_jobs(id,package,queue,status,body,callback,reference) VALUES(?1,'scheduled-tasks','default','running',?2,X'6E756C6C',X'6E756C6C')", running.ID.String(), body)
		return err
	}
	ask := func(ctx context.Context, db *sql.Conn, turn protocol.TurnID, approval bool) error {
		if approval {
			_, err := db.ExecContext(ctx, "INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,?1,?3,X'6E756C6C','',0)", protocol.NewApprovalID().String(), turn.String(), `{"state":"pending"}`)
			return err
		}
		_, err := db.ExecContext(ctx, "INSERT INTO agent_questions(id,turn,call,body) VALUES(?1,?2,?1,?3)", protocol.NewQuestionID().String(), turn.String(), `{"state":{"kind":"pending"}}`)
		return err
	}
	answer := func(ctx context.Context, db *sql.Conn, turn protocol.TurnID) error {
		if _, err := db.ExecContext(ctx, "DELETE FROM agent_approvals WHERE turn=?1", turn.String()); err != nil {
			return err
		}
		_, err := db.ExecContext(ctx, "DELETE FROM agent_questions WHERE turn=?1", turn.String())
		return err
	}
	for _, scenario := range []struct {
		name     string
		approval bool
		kind     protocol.NotificationKind
	}{
		{"scheduled_turn_waiting_for_approval_raises_a_durable_warning", true, protocol.NotificationKindWarning},
		{"scheduled_turn_waiting_for_input_raises_a_durable_notice", false, protocol.NotificationKindInfo},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			storage, node, session, turn, _ := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				if err := job(ctx, db, node, turn); err != nil {
					return err
				}
				if err := ask(ctx, db, turn, scenario.approval); err != nil {
					return err
				}
				events, err := activityEvent(ctx, db, session.ID)
				if err != nil {
					return err
				}
				if len(events) != 2 || events[0].Kind != "notification_changed" || events[1].Kind != "conversation_changed" {
					return fmt.Errorf("waiting transition did not publish its notice first: %+v", events)
				}
				list, err := notices(ctx, db, "task")
				if err != nil {
					return err
				}
				if len(list) != 1 {
					return fmt.Errorf("waiting notices: %+v", list)
				}
				task, valid := dispatchValue[protocol.NotificationDetailsTask](list[0].Content.Details.Data)
				if !valid || task.Turn != turn || task.Status != protocol.ConversationStatusRunning || list[0].Content.Kind != scenario.kind ||
					list[0].Content.Session == nil || *list[0].Content.Session != session.ID || list[0].Read {
					return fmt.Errorf("waiting notice: %+v", list[0])
				}
				// Activity that changes nothing about the wait tells nobody again.
				events, err = activityEvent(ctx, db, session.ID)
				if err != nil {
					return err
				}
				if len(events) != 1 {
					return fmt.Errorf("unchanged wait was reported again: %+v", events)
				}
				// An answer settles the notice; the next wait of the turn is new.
				if err := answer(ctx, db, turn); err != nil {
					return err
				}
				events, err = activityEvent(ctx, db, session.ID)
				if err != nil {
					return err
				}
				list, err = notices(ctx, db, "task")
				if err != nil {
					return err
				}
				if len(events) != 2 || len(list) != 1 || !list[0].Read {
					return fmt.Errorf("answered wait kept its notice unread: %+v", list)
				}
				if err := ask(ctx, db, turn, scenario.approval); err != nil {
					return err
				}
				if _, err := activityEvent(ctx, db, session.ID); err != nil {
					return err
				}
				list, err = notices(ctx, db, "task")
				if err != nil {
					return err
				}
				if len(list) != 2 || list[0].Read == list[1].Read {
					return fmt.Errorf("second wait was not reported once: %+v", list)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
	t.Run("turn_a_controller_started_raises_none", func(t *testing.T) {
		storage, _, session, turn, _ := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := t.Context()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			if err := ask(ctx, db, turn, true); err != nil {
				return err
			}
			events, err := activityEvent(ctx, db, session.ID)
			if err != nil {
				return err
			}
			list, err := notices(ctx, db, "task")
			if err == nil && (len(events) != 1 || len(list) != 0) {
				return fmt.Errorf("interactive wait raised a durable notice: %+v", list)
			}
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("finished_run_settles_its_waiting_notice", func(t *testing.T) {
		storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := t.Context()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			if err := job(ctx, db, node, turn); err != nil {
				return err
			}
			if err := ask(ctx, db, turn, true); err != nil {
				return err
			}
			if _, err := activityEvent(ctx, db, session.ID); err != nil {
				return err
			}
			if err := answer(ctx, db, turn); err != nil {
				return err
			}
			if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
				return err
			}
			list, err := notices(ctx, db, "task")
			if err != nil {
				return err
			}
			unread := 0
			for _, notice := range list {
				task, _ := dispatchValue[protocol.NotificationDetailsTask](notice.Content.Details.Data)
				if !notice.Read {
					unread++
					if task.Status != protocol.ConversationStatusCompleted {
						return fmt.Errorf("finished run left a waiting notice unread: %+v", notice)
					}
				}
			}
			if len(list) != 2 || unread != 1 {
				return fmt.Errorf("finished scheduled run notices: %+v", list)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	for _, scenario := range []struct {
		name    string
		result  error
		kind    protocol.NotificationKind
		noticed bool
	}{
		{"failed_callback_raises_one_error_notice", &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "denied"}, protocol.NotificationKindError, true},
		{"unknown_callback_outcome_raises_one_warning", &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "unknown"}, protocol.NotificationKindWarning, true},
		{"cancelled_callback_raises_none", &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "cancelled"}, "", false},
		{"completed_callback_raises_none", nil, "", false},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			fixture := newDispatchFixture(t, "notice")
			fixture.write(t, func(ctx context.Context, db *sql.Conn) error {
				queued, err := enqueueDispatch(ctx, db, &fixture.reference, fixture.handler, fixture.event(), 1)
				if err != nil {
					return err
				}
				if _, _, _, err := claimDispatchJobs(ctx, db, fixture.node, 2); err != nil {
					return err
				}
				events, err := FinishDispatch(ctx, db, queued.ID, scenario.result, 3)
				if err != nil {
					return err
				}
				list, err := notices(ctx, db, "system")
				if err != nil {
					return err
				}
				if !scenario.noticed {
					if len(list) != 0 || len(events) != 1 {
						return fmt.Errorf("callback raised a notice: %+v", list)
					}
					return nil
				}
				if len(list) != 1 || len(events) != 2 || events[1].Kind != "notification_changed" {
					return fmt.Errorf("callback notices: %+v, events %+v", list, events)
				}
				report, valid := dispatchValue[protocol.NotificationSystemSystemDiagnostic](list[0].Content.Details.Data)
				var failure *protocol.Fault
				errors.As(scenario.result, &failure)
				if !valid || list[0].Content.Kind != scenario.kind || report.Incident != queued.Request || report.Code != failure.Code ||
					report.Source != protocol.NotificationSystemSystemSourcePluginCallback || report.Plugin == nil || report.Plugin.Name != fixture.reference.Name ||
					report.Operation == nil || *report.Operation != fixture.handler.Name || list[0].TimestampMs != 3 {
					return fmt.Errorf("callback notice: %+v", list[0])
				}
				return nil
			})
		})
	}
	t.Run("callback_that_admitted_a_turn_reports_through_its_run", func(t *testing.T) {
		fixture := newDispatchFixture(t, "admitted")
		fixture.write(t, func(ctx context.Context, db *sql.Conn) error {
			handler := fixture.handler
			handler.Callback.Completion = protocol.DispatchCompletionTurn
			queued, err := enqueueDispatch(ctx, db, &fixture.reference, handler, fixture.event(), 1)
			if err != nil {
				return err
			}
			work, _, _, err := claimDispatchJobs(ctx, db, fixture.node, 2)
			if err != nil {
				return err
			}
			if _, err := fixture.admitTurn(ctx, db, work[0].Request); err != nil {
				return err
			}
			if _, err := FinishDispatch(ctx, db, queued.ID, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "model is unavailable"}, 3); err != nil {
				return err
			}
			list, err := notices(ctx, db, "system")
			if err == nil && len(list) != 0 {
				return fmt.Errorf("failed turn was reported by its job as well: %+v", list)
			}
			return err
		})
	})
	t.Run("callback_refused_at_claim_raises_one_error_notice", func(t *testing.T) {
		fixture := newDispatchFixture(t, "refused")
		fixture.write(t, func(ctx context.Context, db *sql.Conn) error {
			queued, err := enqueueDispatch(ctx, db, &fixture.reference, fixture.handler, fixture.event(), 1)
			if err != nil {
				return err
			}
			if _, err := db.ExecContext(ctx, "DELETE FROM plugin_packages WHERE name=?1", fixture.reference.Name); err != nil {
				return err
			}
			batch, events, err := DispatchTick(ctx, db, fixture.node, 2, false)
			if err != nil {
				return err
			}
			list, err := notices(ctx, db, "system")
			if err != nil {
				return err
			}
			published := 0
			for _, event := range events {
				if event.Kind == "notification_changed" {
					published++
				}
			}
			if len(batch.Work) != 0 || len(list) != 1 || published != 1 {
				return fmt.Errorf("refused callback notices: %+v, events %+v", list, events)
			}
			report, valid := dispatchValue[protocol.NotificationSystemSystemDiagnostic](list[0].Content.Details.Data)
			if !valid || report.Incident != queued.Request || report.Code != protocol.ErrorCodeNotConfigured {
				return fmt.Errorf("refused callback notice: %+v", list[0])
			}
			return nil
		})
	})
}
