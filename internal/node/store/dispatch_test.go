package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchContracts(test *testing.T) {
	test.Parallel()
	test.Run("metadata_edits_preserve_consumed_cursor", func(test *testing.T) {
		fixture := newDispatchFixture(test, "weekday")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			now := dispatchAt(test, "2026-10-16T08:00:00Z")
			item, err := fixture.weekly(ctx, db, now)
			if err != nil {
				return err
			}
			original := *item.NextMs
			if original <= now {
				return errors.New("weekly schedule did not start after the observation")
			}
			timing, _ := dispatchValue[protocol.DispatchTimingWeeklyPayload](item.Timing.Data)
			if !reflect.DeepEqual(timing.Weekdays, protocol.Bytes{1, 2, 3, 4, 5}) {
				return errors.New("weekly days were not sorted")
			}
			for _, enabled := range []bool{false, true} {
				item.Enabled, item.Payload = enabled, json.RawMessage(`"renamed"`)
				var err error
				item, err = saveDispatchSchedule(ctx, db, fixture.reference.Name, item, now)
				if err != nil {
					return err
				}
				if item.NextMs == nil || *item.NextMs != original {
					return errors.New("metadata edit changed the consumed cursor")
				}
			}
			page, err := listDispatchJobs(ctx, db, fixture.reference.Name, nil, 100)
			if err == nil && len(page.Jobs) != 0 {
				return errors.New("metadata edit enqueued a callback")
			}
			return err
		})
	})
	test.Run("downtime_consumes_latest_occurrence_once", func(test *testing.T) {
		fixture := newDispatchFixture(test, "weekday")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			item, err := fixture.weekly(ctx, db, 1)
			if err != nil {
				return err
			}
			first := dispatchAt(test, "2020-01-06T09:00:00Z")
			item.NextMs = &first
			if err := putDispatchSchedule(ctx, db, fixture.reference.Name, item); err != nil {
				return err
			}
			observed := dispatchAt(test, "2026-10-21T10:00:00Z")
			owners, err := dueDispatchSchedules(ctx, db, observed)
			if err != nil {
				return err
			}
			if !reflect.DeepEqual(owners, []string{fixture.reference.Name}) {
				return fmt.Errorf("due schedule owners: %v", owners)
			}
			owners, err = dueDispatchSchedules(ctx, db, observed)
			if err != nil {
				return err
			}
			if len(owners) != 0 {
				return errors.New("same observation repeated a calendar occurrence")
			}
			page, err := listDispatchJobs(ctx, db, fixture.reference.Name, nil, 100)
			if err != nil {
				return err
			}
			if len(page.Jobs) != 1 || page.Jobs[0].Event.ScheduledMs == nil ||
				*page.Jobs[0].Event.ScheduledMs != dispatchAt(test, "2026-10-21T09:00:00Z") {
				return fmt.Errorf("downtime delivery lost the latest occurrence: %+v", page)
			}
			stored, err := dispatchRequired[protocol.DispatchSchedule](ctx, db, "dispatch_schedules", fixture.reference.Name, "id", item.ID.String())
			if err == nil && (stored.NextMs == nil || *stored.NextMs != dispatchAt(test, "2026-10-22T09:00:00Z") || stored.Revision != item.Revision) {
				return errors.New("calendar consumption changed the revision or next occurrence")
			}
			return err
		})
	})
	test.Run("full_queue_preserves_due_cursor", func(test *testing.T) {
		fixture := newDispatchFixture(test, "weekday")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			item, err := fixture.weekly(ctx, db, 1)
			if err != nil {
				return err
			}
			first := dispatchAt(test, "2026-10-19T09:00:00Z")
			item.NextMs = &first
			if err := putDispatchSchedule(ctx, db, fixture.reference.Name, item); err != nil {
				return err
			}
			var firstJob protocol.JobID
			for index := 0; index < dispatchMaxPending; index++ {
				job, err := enqueueDispatch(ctx, db, &fixture.reference, fixture.handler, fixture.event(), 100)
				if err != nil {
					return err
				}
				if index == 0 {
					firstJob = job.ID
				}
			}
			owners, err := dueDispatchSchedules(ctx, db, first)
			if err != nil {
				return err
			}
			if len(owners) != 0 {
				return errors.New("full queue consumed its occurrence")
			}
			stored, err := dispatchRequired[protocol.DispatchSchedule](ctx, db, "dispatch_schedules", fixture.reference.Name, "id", item.ID.String())
			if err != nil {
				return err
			}
			expected, err := encode(item)
			if err != nil {
				return err
			}
			actual, err := encode(stored)
			if err != nil {
				return err
			}
			if !bytes.Equal(actual, expected) {
				return errors.New("full queue changed its due cursor")
			}
			var events int
			if err := db.QueryRowContext(ctx, "SELECT count(*) FROM dispatch_events").Scan(&events); err != nil {
				return err
			}
			if events != 0 {
				return errors.New("failed fanout committed an event")
			}
			if _, err := cancelDispatchJob(ctx, db, fixture.reference.Name, firstJob, first); err != nil {
				return err
			}
			owners, err = dueDispatchSchedules(ctx, db, first+1)
			if err != nil {
				return err
			}
			if !reflect.DeepEqual(owners, []string{fixture.reference.Name}) {
				return errors.New("freeing queue capacity did not release the occurrence")
			}
			stored, err = dispatchRequired[protocol.DispatchSchedule](ctx, db, "dispatch_schedules", fixture.reference.Name, "id", item.ID.String())
			if err == nil && (stored.NextMs == nil || *stored.NextMs != dispatchAt(test, "2026-10-20T09:00:00Z")) {
				return errors.New("released occurrence has an incorrect next cursor")
			}
			return err
		})
	})
	test.Run("schedule_pause_preserves_consumed_cursor", func(test *testing.T) {
		fixture := newDispatchFixture(test, "once")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			item := protocol.DispatchSchedule{ID: protocol.NewScheduleID(), Topic: "once", Payload: json.RawMessage("null"),
				Timing: protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: 1000}}}
			var err error
			item, err = saveDispatchSchedule(ctx, db, fixture.reference.Name, item, 0)
			if err != nil {
				return err
			}
			owners, err := dueDispatchSchedules(ctx, db, 2000)
			if err != nil {
				return err
			}
			if len(owners) != 0 {
				return errors.New("paused one-shot was delivered")
			}
			item.Enabled = true
			item, err = saveDispatchSchedule(ctx, db, fixture.reference.Name, item, 0)
			if err != nil {
				return err
			}
			owners, err = dueDispatchSchedules(ctx, db, 2000)
			if err != nil {
				return err
			}
			if len(owners) == 0 {
				return errors.New("enabled one-shot was not delivered")
			}
			item, err = dispatchRequired[protocol.DispatchSchedule](ctx, db, "dispatch_schedules", fixture.reference.Name, "id", item.ID.String())
			if err != nil {
				return err
			}
			if item.NextMs != nil {
				return errors.New("completed one-shot retained a due cursor")
			}
			for _, enabled := range []bool{false, true} {
				item.Enabled = enabled
				item, err = saveDispatchSchedule(ctx, db, fixture.reference.Name, item, 0)
				if err != nil {
					return err
				}
			}
			owners, err = dueDispatchSchedules(ctx, db, 3000)
			if err != nil {
				return err
			}
			if len(owners) != 0 {
				return errors.New("re-enabled one-shot repeated its effect")
			}
			page, err := listDispatchJobs(ctx, db, fixture.reference.Name, nil, 100)
			if err == nil && len(page.Jobs) != 1 {
				return fmt.Errorf("one-shot callback count: %d", len(page.Jobs))
			}
			return err
		})
	})
	test.Run("interval_coalesces_missed_ticks", func(test *testing.T) {
		fixture := newDispatchFixture(test, "every")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			item := protocol.DispatchSchedule{ID: protocol.NewScheduleID(), Enabled: true, Topic: "every", Payload: json.RawMessage("null"),
				Timing: protocol.DispatchTiming{Kind: "every", Data: protocol.DispatchTimingEveryPayload{AnchorMs: 1000, IntervalMs: 1000}}}
			var err error
			item, err = saveDispatchSchedule(ctx, db, fixture.reference.Name, item, 0)
			if err != nil {
				return err
			}
			for count := 0; count < 2; count++ {
				if _, err := dueDispatchSchedules(ctx, db, 5500); err != nil {
					return err
				}
			}
			page, err := listDispatchJobs(ctx, db, fixture.reference.Name, nil, 100)
			if err != nil {
				return err
			}
			if len(page.Jobs) != 1 || page.Jobs[0].Event.ScheduledMs == nil || *page.Jobs[0].Event.ScheduledMs != 5000 {
				return errors.New("missed interval ticks were replayed instead of coalesced")
			}
			stored, err := dispatchRequired[protocol.DispatchSchedule](ctx, db, "dispatch_schedules", fixture.reference.Name, "id", item.ID.String())
			if err == nil && (stored.NextMs == nil || *stored.NextMs != 6000) {
				return errors.New("interval cursor drifted from its anchor")
			}
			return err
		})
	})
	test.Run("retained_version_requires_claimed_request", func(test *testing.T) {
		fixture := newDispatchFixture(test, "retained")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			old := fixture.reference
			current, err := dispatchCurrent(ctx, db, old.Name)
			if err != nil {
				return err
			}
			current.Summary.Digest = "dispatch-fixture-v2"
			body, err := encode(current)
			if err != nil {
				return err
			}
			if _, err := db.ExecContext(ctx, "UPDATE plugins SET body=?1 WHERE name=?2", body, old.Name); err != nil {
				return err
			}
			handler := fixture.handler
			handler.Callback.Command = &protocol.Command{Kind: "call_plugin", Data: protocol.CommandCallPluginPayload{Handler: "run", Input: json.RawMessage("null")}}
			handler.Callback.Bindings = map[string]string{"/data/input": "/payload"}
			job, err := enqueueDispatch(ctx, db, &old, handler, fixture.event(), 1)
			if err != nil {
				return err
			}
			work, _, err := claimDispatchJobs(ctx, db, fixture.node, 2)
			if err != nil {
				return err
			}
			if len(work) != 1 {
				return errors.New("retained callback was not claimed")
			}
			original := work[0].Request
			info, err := DispatchPackage(ctx, db, fixture.node, fixture.node, original)
			if err != nil {
				return err
			}
			if info == nil || dispatchReference(info.Summary) != old {
				return errors.New("claimed callback lost its frozen package authority")
			}
			info, err = DispatchPackage(ctx, db, fixture.node, protocol.NodeID{92}, original)
			if err != nil {
				return err
			}
			if info != nil {
				return errors.New("remote caller borrowed a claimed request")
			}
			for _, forgeScope := range []bool{false, true} {
				forged := original
				plugin := *original.Plugin
				forged.Plugin = &plugin
				if forgeScope {
					worktree := protocol.NewWorktreeID()
					forged.Plugin.Worktree = &worktree
				} else {
					forged.Command = protocol.Command{Kind: "call_plugin", Data: protocol.CommandCallPluginPayload{Handler: "run", Input: json.RawMessage(`{"key":"forged"}`)}}
				}
				_, err := DispatchPackage(ctx, db, fixture.node, fixture.node, forged)
				var failure *protocol.Fault
				if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodePermissionDenied {
					return fmt.Errorf("forged claimed request was accepted: %v", err)
				}
			}
			current.Summary.Enabled = false
			body, err = encode(current)
			if err != nil {
				return err
			}
			if _, err := db.ExecContext(ctx, "UPDATE plugins SET body=?1 WHERE name=?2", body, old.Name); err != nil {
				return err
			}
			_, err = DispatchPackage(ctx, db, fixture.node, fixture.node, original)
			var failure *protocol.Fault
			if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeNotConfigured {
				return fmt.Errorf("disabled package retained callback authority: %v", err)
			}
			if _, err := FinishDispatch(ctx, db, job.ID, nil, 3); err != nil {
				return err
			}
			info, err = DispatchPackage(ctx, db, fixture.node, fixture.node, original)
			if err == nil && info != nil {
				return errors.New("finished callback retained claimed authority")
			}
			return err
		})
	})
	test.Run("recovery_does_not_replay_uncertain_effects", func(test *testing.T) {
		fixture := newDispatchFixture(test, "recover")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			for index := 0; index < 3; index++ {
				handler := fixture.handler
				if index == 2 {
					handler.Callback.Completion = protocol.DispatchCompletionTurn
				}
				if _, err := enqueueDispatch(ctx, db, &fixture.reference, handler, fixture.event(), 1); err != nil {
					return err
				}
			}
			work, _, err := claimDispatchJobs(ctx, db, fixture.node, 2)
			if err != nil {
				return err
			}
			if len(work) != 3 {
				return errors.New("recovery fixture was not claimed")
			}
			output := protocol.Output{Kind: "notifications_dismissed"}
			result := protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}
			requestBody, err := encode(work[1].Request)
			if err != nil {
				return err
			}
			resultBody, err := encode(result)
			if err != nil {
				return err
			}
			if _, err := db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",
				fixture.node[:], work[1].Request.ID.String(), requestBody, string(resultBody)); err != nil {
				return err
			}
			turn, err := fixture.admitTurn(ctx, db, work[2].Request)
			if err != nil {
				return err
			}
			batch, _, err := DispatchTick(ctx, db, fixture.node, 3, true)
			if err != nil {
				return err
			}
			if len(batch.Work) != 1 || batch.Work[0].Job != work[2].Job || batch.Work[0].Request.ID != work[2].Request.ID || batch.Pending {
				return errors.New("recovery replayed a previously claimed callback")
			}
			for index, status := range []protocol.DispatchStatus{protocol.DispatchStatusUnknown, protocol.DispatchStatusCompleted, protocol.DispatchStatusRunning} {
				job, err := dispatchRequired[protocol.DispatchJob](ctx, db, "dispatch_jobs", fixture.reference.Name, "id", work[index].Job.String())
				if err != nil {
					return err
				}
				if job.Status != status || (job.FinishedMs == nil) != (status == protocol.DispatchStatusRunning) {
					return fmt.Errorf("recovery status: %+v, expected %s", job, status)
				}
			}
			actualTurn, err := ReadDispatchTurn(ctx, db, fixture.node, fixture.reference.Name, work[2].Job)
			if err != nil {
				return err
			}
			if actualTurn != turn.ID {
				return errors.New("recovery lost the exact admitted turn")
			}
			run, err := ReadAgentRun(ctx, db, turn.ID)
			if err != nil {
				return err
			}
			run.Status = protocol.ConversationStatusCompleted
			if err := putAgentRun(ctx, db, run); err != nil {
				return err
			}
			batch, _, err = DispatchTick(ctx, db, fixture.node, 4, true)
			if err == nil && len(batch.Work) != 0 {
				return errors.New("recovery observed an already completed turn")
			}
			return err
		})
	})
}

type dispatchFixture struct {
	handle    *Handle
	node      protocol.NodeID
	reference protocol.PluginReference
	handler   protocol.DispatchHandler
}

func newDispatchFixture(test *testing.T, topic string) dispatchFixture {
	test.Helper()
	_, node, storage := schemaFixture(test)
	fixture := dispatchFixture{handle: storage.Node, node: node}
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest = "dispatch-source", "dispatch-fixture-v1"
	info.Summary.Revision, info.Summary.Enabled = 1, true
	fixture.reference = dispatchReference(info.Summary)
	fixture.handler = protocol.DispatchHandler{
		Name: topic, Enabled: true, Source: protocol.DispatchSource{Package: fixture.reference.Name, Topic: topic}, Queue: topic,
		Callback: protocol.DispatchCallback{
			Completion: protocol.DispatchCompletionCommand, Bindings: map[string]string{},
			Command: &protocol.Command{Kind: "read_plugin_value", Data: protocol.CommandReadPluginValuePayload{Key: "fixture"}},
		},
	}
	fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
		body, err := encode(info)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO plugins(name,revision,installed,body) VALUES(?1,1,1,?2)", fixture.reference.Name, body); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO plugin_packages(name,digest,body) VALUES(?1,?2,?3)", fixture.reference.Name, fixture.reference.Digest, body); err != nil {
			return err
		}
		fixture.handler, err = saveDispatchHandler(ctx, db, fixture.reference, fixture.handler)
		return err
	})
	return fixture
}

func (fixture dispatchFixture) write(test *testing.T, operation func(context.Context, *sql.Conn) error) {
	test.Helper()
	if err := fixture.handle.Write(test.Context(), func(db *sql.Conn) error { return operation(test.Context(), db) }); err != nil {
		test.Fatal(err)
	}
}

func (fixture dispatchFixture) weekly(ctx context.Context, db *sql.Conn, timestamp int64) (protocol.DispatchSchedule, error) {
	return saveDispatchSchedule(ctx, db, fixture.reference.Name, protocol.DispatchSchedule{
		ID: protocol.NewScheduleID(), Enabled: true, Topic: "weekday", Payload: json.RawMessage("null"),
		Timing: protocol.DispatchTiming{Kind: "weekly", Data: protocol.DispatchTimingWeeklyPayload{Timezone: "UTC", Weekdays: protocol.Bytes{5, 1, 2, 3, 4}, Hour: 9}},
	}, timestamp)
}

func (fixture dispatchFixture) event() protocol.DispatchEvent {
	return protocol.DispatchEvent{
		ID: protocol.NewEventID(), Source: fixture.handler.Source, Payload: json.RawMessage(`{"key":"bound"}`), TimestampMs: 100,
	}
}

func (fixture dispatchFixture) admitTurn(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.QueuedTurn, error) {
	turn := protocol.DefaultQueuedTurn()
	turn.ID, turn.Session, turn.Request = protocol.NewTurnID(), protocol.NewSessionID(), request.ID
	turn.Kind, turn.Revision = protocol.ConversationRunKindTask, 1
	turn.Config.Provider = protocol.NewProviderID()
	turn.Config.Mode, turn.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	worktree := protocol.NewWorktreeID()
	output := protocol.Output{Kind: "queued_turn", Data: turn}
	values := []any{request, protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}, turn.Config, turn.Roles, turn.Plugins}
	bodies := make([][]byte, len(values))
	for index, value := range values {
		body, err := encode(value)
		if err != nil {
			return turn, err
		}
		bodies[index] = body
	}
	var receipt protocol.Result[protocol.Output, protocol.Fault]
	if err := json.Unmarshal(bodies[1], &receipt); err != nil {
		return turn, fmt.Errorf("fixture admission receipt does not round trip: %w", err)
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)", fixture.node[:], request.ID.String(), bodies[0], string(bodies[1])); err != nil {
		return turn, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO worktrees(id,path,main) VALUES(?1,?2,1)", worktree.String(), "/dispatch-fixture"); err != nil {
		return turn, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)", turn.Session.String(), worktree.String()); err != nil {
		return turn, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,1,?2,?3,?4)", turn.Session.String(), worktree.String(), bodies[2], bodies[3]); err != nil {
		return turn, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO turns(id,kind,plugins,roles,session,revision,request,caller) VALUES(?1,'task',?2,?3,?4,1,?5,?6)",
		turn.ID.String(), bodies[4], bodies[3], turn.Session.String(), request.ID.String(), fixture.node[:]); err != nil {
		return turn, err
	}
	run := protocol.DefaultConversationRun()
	run.Turn, run.Session, run.Worktree = turn.ID, turn.Session, worktree
	run.Kind, run.Status, run.Sequence, run.Revision = turn.Kind, protocol.ConversationStatusRunning, 1, 1
	body, err := encode(run)
	if err != nil {
		return turn, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO agent_runs(turn,ready,body,hooks) VALUES(?1,1,?2,X'5B5D')", turn.ID.String(), body)
	return turn, err
}

func dispatchAt(test *testing.T, value string) int64 {
	test.Helper()
	instant, err := time.Parse(time.RFC3339, value)
	if err != nil {
		test.Fatal(err)
	}
	return instant.UnixMilli()
}
