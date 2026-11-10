package store

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"math"
	"slices"
	"sort"

	dispatcher "github.com/sailry/sailry-harness/internal/node/dispatch"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func saveDispatchSchedule(ctx context.Context, db *sql.Conn, packageName string, schedule protocol.DispatchSchedule, timestamp int64) (protocol.DispatchSchedule, error) {
	if err := dispatchName(schedule.Topic); err != nil {
		return protocol.DispatchSchedule{}, err
	}
	if err := dispatchData(schedule.Payload); err != nil {
		return protocol.DispatchSchedule{}, err
	}
	if schedule.Timing.Kind == "weekly" {
		value, ok := dispatchValue[protocol.DispatchTimingWeeklyPayload](schedule.Timing.Data)
		if !ok {
			return protocol.DispatchSchedule{}, invalid("schedule timing is invalid")
		}
		value.Weekdays = append(protocol.Bytes{}, value.Weekdays...)
		slices.Sort(value.Weekdays)
		schedule.Timing.Data = value
	}
	first, err := dispatcher.First(schedule.Timing, timestamp)
	if err != nil {
		return protocol.DispatchSchedule{}, err
	}
	previous, err := readBody[protocol.DispatchSchedule](ctx, db, "SELECT body FROM dispatch_schedules WHERE package=?1 AND id=?2", packageName, schedule.ID.String())
	if err != nil {
		return protocol.DispatchSchedule{}, err
	}
	var revision uint64
	if previous != nil {
		revision = previous.Revision
	}
	if err := checkRevision(revision, schedule.Revision); err != nil {
		return protocol.DispatchSchedule{}, err
	}
	if previous == nil {
		if err := dispatchCapacity(ctx, db, "dispatch_schedules", packageName); err != nil {
			return protocol.DispatchSchedule{}, err
		}
	}
	schedule.Revision, err = nextRevision(schedule.Revision)
	if err != nil {
		return protocol.DispatchSchedule{}, err
	}
	schedule.NextMs = &first
	if previous != nil {
		oldTiming, err := encode(previous.Timing)
		if err != nil {
			return protocol.DispatchSchedule{}, err
		}
		newTiming, err := encode(schedule.Timing)
		if err != nil {
			return protocol.DispatchSchedule{}, err
		}
		if bytes.Equal(oldTiming, newTiming) {
			schedule.NextMs = previous.NextMs
		}
	}
	return schedule, putDispatchSchedule(ctx, db, packageName, schedule)
}

func putDispatchSchedule(ctx context.Context, db *sql.Conn, packageName string, schedule protocol.DispatchSchedule) error {
	if err := dispatchBudget(ctx, db, "dispatch_schedules", packageName, "id", schedule.ID.String(), schedule); err != nil {
		return err
	}
	body, err := encode(schedule)
	if err != nil {
		return err
	}
	var due *int64
	if schedule.Enabled {
		due = schedule.NextMs
	}
	_, err = db.ExecContext(ctx, "INSERT INTO dispatch_schedules(package,id,next_ms,body) VALUES(?1,?2,?3,?4) ON CONFLICT(package,id) DO UPDATE SET next_ms=excluded.next_ms,body=excluded.body",
		packageName, schedule.ID.String(), due, body)
	return err
}

func dueDispatchSchedules(ctx context.Context, db *sql.Conn, timestamp int64) ([]string, error) {
	plugins, err := readBodies[protocol.PluginInfo](ctx, db, "SELECT body FROM plugins WHERE installed=1 ORDER BY name")
	if err != nil {
		return nil, err
	}
	enabled := map[string]bool{}
	for _, info := range plugins {
		if info.Summary.Enabled {
			enabled[info.Summary.Name] = true
		}
	}
	type record struct{ packageName, identity string }
	rows, err := db.QueryContext(ctx, "SELECT package,id FROM dispatch_schedules WHERE next_ms<=?1 ORDER BY next_ms", timestamp)
	if err != nil {
		return nil, err
	}
	records := []record{}
	for rows.Next() {
		var item record
		if err = rows.Scan(&item.packageName, &item.identity); err != nil {
			break
		}
		if item.packageName == dispatchNativeOwner || enabled[item.packageName] {
			records = append(records, item)
		}
	}
	readErr := rows.Err()
	closeErr := rows.Close()
	if err = errors.Join(err, readErr, closeErr); err != nil {
		return nil, err
	}
	changed := []string{}
	attempted := 0
	for _, item := range records {
		if attempted == 64 {
			break
		}
		schedule, err := dispatchRequired[protocol.DispatchSchedule](ctx, db, "dispatch_schedules", item.packageName, "id", item.identity)
		if err != nil {
			return nil, err
		}
		scheduled, next, err := dispatcher.Occurrence(schedule, timestamp)
		if err != nil {
			var failure *protocol.Fault
			if errors.As(err, &failure) && failure.Code == protocol.ErrorCodeInvalidRequest {
				continue
			}
			return nil, err
		}
		attempted++
		event := protocol.DispatchEvent{
			ID: protocol.NewEventID(), Source: protocol.DispatchSource{Package: item.packageName, Topic: schedule.Topic},
			Payload: schedule.Payload, TimestampMs: timestamp, Schedule: &schedule.ID, ScheduledMs: &scheduled,
		}
		if _, err := db.ExecContext(ctx, "SAVEPOINT dispatch_due"); err != nil {
			return nil, err
		}
		_, owners, publishErr := publishDispatch(ctx, db, event, timestamp)
		if publishErr != nil {
			_, rollbackErr := db.ExecContext(ctx, "ROLLBACK TO dispatch_due")
			_, releaseErr := db.ExecContext(ctx, "RELEASE dispatch_due")
			if err := errors.Join(rollbackErr, releaseErr); err != nil {
				return nil, err
			}
			var failure *protocol.Fault
			if errors.As(publishErr, &failure) && failure.Code == protocol.ErrorCodeBusy {
				continue
			}
			return nil, publishErr
		}
		if _, err := db.ExecContext(ctx, "RELEASE dispatch_due"); err != nil {
			return nil, err
		}
		changed = append(changed, owners...)
		schedule.NextMs = next
		if err := putDispatchSchedule(ctx, db, item.packageName, schedule); err != nil {
			return nil, err
		}
	}
	sort.Strings(changed)
	return slices.Compact(changed), nil
}

func nextDispatchSchedule(ctx context.Context, db *sql.Conn, timestamp int64) (*int64, error) {
	type record struct {
		packageName string
		due         int64
	}
	rows, err := db.QueryContext(ctx, "SELECT package,min(next_ms) FROM dispatch_schedules WHERE next_ms IS NOT NULL GROUP BY package")
	if err != nil {
		return nil, err
	}
	records := []record{}
	for rows.Next() {
		var item record
		if err = rows.Scan(&item.packageName, &item.due); err != nil {
			break
		}
		records = append(records, item)
	}
	readErr := rows.Err()
	closeErr := rows.Close()
	if err = errors.Join(err, readErr, closeErr); err != nil {
		return nil, err
	}
	var next *int64
	for _, item := range records {
		if item.packageName != dispatchNativeOwner {
			info, err := readBody[protocol.PluginInfo](ctx, db, "SELECT body FROM plugins WHERE name=?1 AND installed=1", item.packageName)
			if err != nil {
				return nil, err
			}
			if info == nil || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
				continue
			}
		}
		due := item.due
		if due <= timestamp {
			due = math.MaxInt64
			if timestamp <= math.MaxInt64-1000 {
				due = timestamp + 1000
			}
		}
		if next == nil || due < *next {
			next = &due
		}
	}
	return next, nil
}
