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

// Over 1 s only beside the package's other parallel tests: each opens its own synced store.
func TestDispatchContracts(test *testing.T) {
	test.Parallel()
	test.Run("settlement preserves an already committed outcome", func(test *testing.T) {
		fixture := newDispatchFixture(test, "settlement")
		failure := &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture callback failure"}
		var ended protocol.DispatchJob
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			job, err := enqueueDispatch(ctx, db, &fixture.reference, fixture.handler, fixture.event(), 100, false)
			if err != nil {
				return err
			}
			work, _, _, err := claimDispatchJobs(ctx, db, fixture.node, 200)
			if err != nil {
				return err
			}
			if len(work) != 1 || work[0].Job != job.ID {
				return errors.New("fixture did not claim the callback")
			}
			if _, err := FinishDispatch(ctx, db, job.ID, failure, 300); err != nil {
				return err
			}
			ended, err = dispatchRequired[protocol.DispatchJob](ctx, db, "dispatch_jobs", fixture.reference.Name, "id", job.ID.String())
			return err
		})
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			events, err := FinishDispatch(ctx, db, ended.ID, failure, 400)
			if err != nil {
				return err
			}
			current, err := dispatchRequired[protocol.DispatchJob](ctx, db, "dispatch_jobs", fixture.reference.Name, "id", ended.ID.String())
			if err != nil {
				return err
			}
			if len(events) != 0 || !reflect.DeepEqual(current, ended) {
				return errors.New("settlement replaced the stored outcome or repeated its notice")
			}
			// Completed history can be collected before an uncertain commit
			// is confirmed; this must not keep the settlement owner alive.
			if _, err := db.ExecContext(ctx, "DELETE FROM dispatch_jobs WHERE id=?1", ended.ID.String()); err != nil {
				return err
			}
			events, err = FinishDispatch(ctx, db, ended.ID, failure, 500)
			if err == nil && len(events) != 0 {
				return errors.New("collected completion produced another event")
			}
			return err
		})
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			job, err := enqueueDispatch(ctx, db, &fixture.reference, fixture.handler, fixture.event(), 600, false)
			if err != nil {
				return err
			}
			_, err = FinishDispatch(ctx, db, job.ID, nil, 700)
			var failed *protocol.Fault
			if !errors.As(err, &failed) || failed.Code != protocol.ErrorCodeConflict {
				return fmt.Errorf("unexecuted callback accepted a completion: %v", err)
			}
			return nil
		})
	})
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
			// Another handler fills the queue: the schedule's own handler is
			// idle, so its occurrence waits for capacity instead of being skipped.
			filler := fixture.handler
			filler.Name, filler.Revision = "filler", 0
			filler.Source.Topic = "filler"
			filler, err = saveDispatchHandler(ctx, db, fixture.reference, filler)
			if err != nil {
				return err
			}
			var firstJob protocol.JobID
			for index := 0; index < dispatchMaxPending; index++ {
				job, err := enqueueDispatch(ctx, db, &fixture.reference, filler, fixture.event(), 100, false)
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
	test.Run("scheduled_occurrence_is_skipped_while_its_handler_runs", func(test *testing.T) {
		fixture := newDispatchFixture(test, "every")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			item := protocol.DispatchSchedule{ID: protocol.NewScheduleID(), Enabled: true, Topic: "every", Payload: json.RawMessage("null"),
				Timing: protocol.DispatchTiming{Kind: "every", Data: protocol.DispatchTimingEveryPayload{AnchorMs: 1000, IntervalMs: 1000}}}
			if _, err := saveDispatchSchedule(ctx, db, fixture.reference.Name, item, 0); err != nil {
				return err
			}
			// A second handler shares the first one's queue name: the queue is a
			// label and never makes one handler wait for another.
			neighbour := fixture.handler
			neighbour.Name, neighbour.Revision = "neighbour", 0
			neighbour.Source.Topic = "neighbour"
			neighbour, err := saveDispatchHandler(ctx, db, fixture.reference, neighbour)
			if err != nil {
				return err
			}
			jobs := func() ([]protocol.DispatchJob, error) {
				page, err := listDispatchJobs(ctx, db, fixture.reference.Name, nil, 100)
				return page.Jobs, err
			}
			skipped := func(job protocol.DispatchJob, scheduled int64) bool {
				return job.Status == protocol.DispatchStatusCancelled && job.Error != nil && job.Error.Code == protocol.ErrorCodeBusy &&
					job.FinishedMs != nil && job.StartedMs == nil && job.Event.ScheduledMs != nil && *job.Event.ScheduledMs == scheduled
			}
			// The first occurrence queues; the next one finds it still queued.
			for _, observed := range []int64{1500, 2500} {
				if _, err := dueDispatchSchedules(ctx, db, observed); err != nil {
					return err
				}
			}
			list, err := jobs()
			if err != nil {
				return err
			}
			if len(list) != 2 || !skipped(list[0], 2000) || list[1].Status != protocol.DispatchStatusQueued {
				return fmt.Errorf("occurrence behind a queued run was not skipped: %+v", list)
			}
			running := list[1].ID
			work, _, _, err := claimDispatchJobs(ctx, db, fixture.node, 2600)
			if err != nil {
				return err
			}
			if len(work) != 1 || work[0].Job != running {
				return fmt.Errorf("a skipped occurrence was claimed: %+v", work)
			}
			if _, err := dueDispatchSchedules(ctx, db, 3500); err != nil {
				return err
			}
			// Run now and another handler of the same queue are not schedule
			// occurrences of this handler and still start.
			manual, err := enqueueDispatch(ctx, db, &fixture.reference, fixture.handler, fixture.event(), 3600, false)
			if err != nil {
				return err
			}
			scheduled := fixture.event()
			scheduled.Schedule, scheduled.ScheduledMs = &item.ID, &manual.CreatedMs
			other, err := enqueueDispatch(ctx, db, &fixture.reference, neighbour, scheduled, 3600, false)
			if err != nil {
				return err
			}
			if manual.Status != protocol.DispatchStatusQueued || other.Status != protocol.DispatchStatusQueued {
				return errors.New("the overlap rule skipped a run that is not this handler's schedule")
			}
			list, err = jobs()
			if err != nil {
				return err
			}
			if len(list) != 5 || !skipped(list[2], 3000) {
				return fmt.Errorf("occurrence behind a running job was not skipped: %+v", list)
			}
			stored, err := dispatchRequired[protocol.DispatchSchedule](ctx, db, "dispatch_schedules", fixture.reference.Name, "id", item.ID.String())
			if err != nil {
				return err
			}
			if stored.NextMs == nil || *stored.NextMs != 4000 {
				return errors.New("a skipped occurrence was not consumed")
			}
			for _, id := range []protocol.JobID{manual.ID, other.ID} {
				if _, err := cancelDispatchJob(ctx, db, fixture.reference.Name, id, 3700); err != nil {
					return err
				}
			}
			if _, err := FinishDispatch(ctx, db, running, nil, 3800); err != nil {
				return err
			}
			if _, err := dueDispatchSchedules(ctx, db, 4500); err != nil {
				return err
			}
			list, err = jobs()
			if err == nil && (len(list) != 6 || list[0].Status != protocol.DispatchStatusQueued || list[0].Event.ScheduledMs == nil || *list[0].Event.ScheduledMs != 4000) {
				return fmt.Errorf("the occurrence after the run finished did not start: %+v", list)
			}
			return err
		})
	})
	test.Run("restart_in_the_middle_of_a_fire_does_not_run_it_again", func(test *testing.T) {
		fixture := newDispatchFixture(test, "every")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			item := protocol.DispatchSchedule{ID: protocol.NewScheduleID(), Enabled: true, Topic: "every", Payload: json.RawMessage("null"),
				Timing: protocol.DispatchTiming{Kind: "every", Data: protocol.DispatchTimingEveryPayload{AnchorMs: 1000, IntervalMs: 1000}}}
			if _, err := saveDispatchSchedule(ctx, db, fixture.reference.Name, item, 0); err != nil {
				return err
			}
			// One tick consumes the occurrence and claims its job in one write,
			// so no restart can fall between the two.
			batch, _, err := DispatchTick(ctx, db, fixture.node, 1500, false)
			if err != nil {
				return err
			}
			if len(batch.Work) != 1 {
				return fmt.Errorf("the due occurrence was not claimed: %+v", batch)
			}
			fired := batch.Work[0].Job
			// The Node stops before the callback's outcome is recorded. The
			// next start neither replays it nor fires the occurrence again,
			// and says so once.
			notices := 0
			for range 2 {
				batch, events, err := DispatchTick(ctx, db, fixture.node, 1600, true)
				if err != nil {
					return err
				}
				if len(batch.Work) != 0 {
					return fmt.Errorf("a restart ran the interrupted occurrence again: %+v", batch.Work)
				}
				for _, event := range events {
					if event.Kind == "notification_changed" {
						notices++
					}
				}
			}
			page, err := listDispatchJobs(ctx, db, fixture.reference.Name, nil, 100)
			if err != nil {
				return err
			}
			if len(page.Jobs) != 1 || page.Jobs[0].ID != fired || page.Jobs[0].Status != protocol.DispatchStatusUnknown || notices != 1 {
				return fmt.Errorf("interrupted occurrence: %+v, %d notices", page.Jobs, notices)
			}
			// The schedule itself goes on with its next occurrence.
			batch, _, err = DispatchTick(ctx, db, fixture.node, 2500, true)
			if err != nil {
				return err
			}
			page, err = listDispatchJobs(ctx, db, fixture.reference.Name, nil, 100)
			if err == nil && (len(batch.Work) != 1 || len(page.Jobs) != 2 || page.Jobs[0].Event.ScheduledMs == nil || *page.Jobs[0].Event.ScheduledMs != 2000) {
				return fmt.Errorf("the occurrence after the restart did not run once: %+v", page.Jobs)
			}
			return err
		})
	})
	test.Run("schedule_edit_and_removal_leave_the_run_in_flight", func(test *testing.T) {
		fixture := newDispatchFixture(test, "every")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			item := protocol.DispatchSchedule{ID: protocol.NewScheduleID(), Enabled: true, Topic: "every", Payload: json.RawMessage(`"first"`),
				Timing: protocol.DispatchTiming{Kind: "every", Data: protocol.DispatchTimingEveryPayload{AnchorMs: 1000, IntervalMs: 1000}}}
			item, err := saveDispatchSchedule(ctx, db, fixture.reference.Name, item, 0)
			if err != nil {
				return err
			}
			batch, _, err := DispatchTick(ctx, db, fixture.node, 1500, false)
			if err != nil {
				return err
			}
			if len(batch.Work) != 1 {
				return fmt.Errorf("the due occurrence was not claimed: %+v", batch)
			}
			running := batch.Work[0].Job
			status := func() (protocol.DispatchJob, error) {
				return dispatchRequired[protocol.DispatchJob](ctx, db, "dispatch_jobs", fixture.reference.Name, "id", running.String())
			}
			// A new timing starts over at its anchor, which is already due:
			// the edit must not start a second run beside the one in flight.
			item.Payload = json.RawMessage(`"second"`)
			item.Timing.Data = protocol.DispatchTimingEveryPayload{AnchorMs: 1000, IntervalMs: 500}
			if item, err = saveDispatchSchedule(ctx, db, fixture.reference.Name, item, 1600); err != nil {
				return err
			}
			if batch, _, err = DispatchTick(ctx, db, fixture.node, 1700, false); err != nil {
				return err
			}
			page, err := listDispatchJobs(ctx, db, fixture.reference.Name, nil, 100)
			if err != nil {
				return err
			}
			if len(batch.Work) != 0 || len(page.Jobs) != 2 || page.Jobs[0].Status != protocol.DispatchStatusCancelled ||
				page.Jobs[0].Error == nil || page.Jobs[0].Error.Code != protocol.ErrorCodeBusy {
				return fmt.Errorf("an edit started a second run: %+v", page.Jobs)
			}
			// The run in flight keeps the payload it was fired with.
			job, err := status()
			if err != nil {
				return err
			}
			if job.Status != protocol.DispatchStatusRunning || string(job.Event.Payload) != `"first"` {
				return fmt.Errorf("the edit changed the run in flight: %+v", job)
			}
			// Removing the schedule stops later occurrences, not this run.
			if _, _, err := ExecuteDispatchAdmitted(ctx, db, fixture.node, fixture.reference, protocol.DispatchCommand{Kind: "remove_schedule",
				Data: protocol.DispatchCommandRemoveSchedulePayload{ID: item.ID, ExpectedRevision: item.Revision}}, 1800); err != nil {
				return err
			}
			if job, err = status(); err != nil {
				return err
			}
			if job.Status != protocol.DispatchStatusRunning {
				return fmt.Errorf("removing the schedule ended its run: %+v", job)
			}
			if _, err := FinishDispatch(ctx, db, running, nil, 1900); err != nil {
				return err
			}
			if batch, _, err = DispatchTick(ctx, db, fixture.node, 9000, false); err != nil {
				return err
			}
			page, err = listDispatchJobs(ctx, db, fixture.reference.Name, nil, 100)
			if err == nil && (len(batch.Work) != 0 || batch.NextMs != nil || len(page.Jobs) != 2 || page.Jobs[1].Status != protocol.DispatchStatusCompleted) {
				return fmt.Errorf("a removed schedule fired or its run was lost: %+v", page.Jobs)
			}
			return err
		})
	})
	// A reminder is a one-time schedule with its own handler: completing it
	// disables the schedule and a new due time is a new timing.
	test.Run("one_time_schedule_fires_once_and_again_only_for_a_new_time", func(test *testing.T) {
		fixture := newDispatchFixture(test, "once")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			at := func(due int64) protocol.DispatchTiming {
				return protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: due}}
			}
			item, err := saveDispatchSchedule(ctx, db, fixture.reference.Name, protocol.DispatchSchedule{
				ID: protocol.NewScheduleID(), Enabled: true, Topic: "once", Payload: json.RawMessage("null"), Timing: at(5000)}, 0)
			if err != nil {
				return err
			}
			// tick reports the jobs started and finishes them.
			tick := func(timestamp int64) ([]int64, *int64, error) {
				batch, _, err := DispatchTick(ctx, db, fixture.node, timestamp, false)
				if err != nil {
					return nil, nil, err
				}
				scheduled := []int64{}
				for _, work := range batch.Work {
					job, err := dispatchRequired[protocol.DispatchJob](ctx, db, "dispatch_jobs", fixture.reference.Name, "id", work.Job.String())
					if err != nil {
						return nil, nil, err
					}
					scheduled = append(scheduled, *job.Event.ScheduledMs)
					if _, err := FinishDispatch(ctx, db, work.Job, nil, timestamp); err != nil {
						return nil, nil, err
					}
				}
				return scheduled, batch.NextMs, nil
			}
			save := func(change func(*protocol.DispatchSchedule), timestamp int64) error {
				change(&item)
				var err error
				item, err = saveDispatchSchedule(ctx, db, fixture.reference.Name, item, timestamp)
				return err
			}
			for _, step := range []struct {
				name      string
				change    func(*protocol.DispatchSchedule)
				timestamp int64
				fired     []int64
				next      int64
			}{
				{"waits for its time", nil, 4000, []int64{}, 5000},
				{"completed before its time stays silent", func(item *protocol.DispatchSchedule) { item.Enabled = false }, 6000, []int64{}, 0},
				{"reopened after its time fires once, late", func(item *protocol.DispatchSchedule) { item.Enabled = true }, 6500, []int64{5000}, 0},
				{"never fires twice", nil, 7000, []int64{}, 0},
				{"an edit of its text does not fire it again", func(item *protocol.DispatchSchedule) { item.Payload = json.RawMessage(`"text"`) }, 7500, []int64{}, 0},
				{"a new time arms it again", func(item *protocol.DispatchSchedule) { item.Timing = at(9000) }, 8000, []int64{}, 9000},
				{"and fires at the new time", nil, 9500, []int64{9000}, 0},
			} {
				if step.change != nil {
					if err := save(step.change, step.timestamp); err != nil {
						return fmt.Errorf("%s: %w", step.name, err)
					}
				}
				fired, next, err := tick(step.timestamp)
				if err != nil {
					return fmt.Errorf("%s: %w", step.name, err)
				}
				if !reflect.DeepEqual(fired, step.fired) || (next == nil) != (step.next == 0) || next != nil && *next != step.next {
					return fmt.Errorf("%s: fired %v, next %v", step.name, fired, next)
				}
			}
			return nil
		})
	})
	test.Run("retained_version_requires_claimed_request", func(test *testing.T) {
		fixture := newDispatchFixture(test, "retained")
		fixture.write(test, func(ctx context.Context, db *sql.Conn) error {
			old := fixture.reference
			// The stored compatibility records the installing build; the
			// running build decides whether the package dispatches.
			stored, err := readBody[protocol.PluginInfo](ctx, db, "SELECT body FROM plugins WHERE name=?1", old.Name)
			if err != nil {
				return err
			}
			stored.Summary.Compatibility = protocol.PluginCompatibility{Kind: "requires_newer", Data: protocol.PluginCompatibilityRequiresNewerPayload{Required: "0.0.1"}}
			stale, err := encode(stored)
			if err != nil {
				return err
			}
			if _, err := db.ExecContext(ctx, "UPDATE plugins SET body=?1 WHERE name=?2", stale, old.Name); err != nil {
				return err
			}
			current, err := dispatchCurrent(ctx, db, old.Name)
			if err != nil {
				return fmt.Errorf("stale stored compatibility: %w", err)
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
			job, err := enqueueDispatch(ctx, db, &old, handler, fixture.event(), 1, false)
			if err != nil {
				return err
			}
			work, _, _, err := claimDispatchJobs(ctx, db, fixture.node, 2)
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
				if _, err := enqueueDispatch(ctx, db, &fixture.reference, handler, fixture.event(), 1, false); err != nil {
					return err
				}
			}
			work, _, _, err := claimDispatchJobs(ctx, db, fixture.node, 2)
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
