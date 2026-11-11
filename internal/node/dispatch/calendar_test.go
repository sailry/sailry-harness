package dispatch

import (
	"context"
	"math"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchTiming(test *testing.T) {
	test.Parallel()
	for _, scenario := range []struct {
		name  string
		check func(*testing.T)
	}{
		{"skips_weekends_and_consumed_times", func(test *testing.T) {
			calendar := requireCalendar(test, "Asia/Shanghai", []uint8{1, 2, 3, 4, 5}, 9, 0)
			friday := calendarAt(test, "2026-10-16T01:00:00Z")
			monday := calendarAt(test, "2026-10-19T01:00:00Z")
			requireNext(test, calendar, friday-1, &friday)
			requireNext(test, calendar, friday, &monday)
			requireNext(test, calendar, friday+1, &monday)
			requireDue(test, calendar, friday, calendarAt(test, "2026-10-18T15:00:00Z"), friday, &monday)
		}},
		{"uses_local_weekdays_and_fractional_offsets", func(test *testing.T) {
			calendar := requireCalendar(test, "Asia/Kolkata", []uint8{1}, 0, 15)
			expected := calendarAt(test, "2026-10-18T18:45:00Z")
			requireNext(test, calendar, calendarAt(test, "2026-10-18T18:30:00Z"), &expected)
		}},
		{"preserves_wall_time_across_offset_changes", func(test *testing.T) {
			calendar := requireCalendar(test, "Europe/London", []uint8{1, 2, 3, 4, 5, 6, 7}, 9, 0)
			spring := calendarAt(test, "2024-03-31T08:00:00Z")
			autumn := calendarAt(test, "2024-10-27T09:00:00Z")
			requireNext(test, calendar, calendarAt(test, "2024-03-30T09:00:00Z"), &spring)
			requireNext(test, calendar, calendarAt(test, "2024-10-26T08:00:00Z"), &autumn)
		}},
		{"skips_nonexistent_occurrences", func(test *testing.T) {
			calendar := requireCalendar(test, "Europe/London", []uint8{7}, 1, 30)
			expected := calendarAt(test, "2024-04-07T00:30:00Z")
			requireNext(test, calendar, calendarAt(test, "2024-03-31T00:00:00Z"), &expected)
		}},
		{"consumes_ambiguous_occurrences_once", func(test *testing.T) {
			calendar := requireCalendar(test, "Europe/London", []uint8{7}, 1, 30)
			earlier := calendarAt(test, "2024-10-27T00:30:00Z")
			following := calendarAt(test, "2024-11-03T01:30:00Z")
			requireNext(test, calendar, calendarAt(test, "2024-10-27T00:00:00Z"), &earlier)
			for _, observed := range []int64{calendarAt(test, "2024-10-27T00:45:00Z"), calendarAt(test, "2024-10-27T01:45:00Z")} {
				requireDue(test, calendar, earlier, observed, earlier, &following)
				requireNext(test, calendar, observed, &following)
			}
		}},
		// Every row is one question to the calendar: observed at an instant,
		// when is the next occurrence. Past years keep the zone rules fixed.
		{"follows_wall_time_through_each_kind_of_transition", func(test *testing.T) {
			daily := []uint8{1, 2, 3, 4, 5, 6, 7}
			for _, row := range []struct {
				name, timezone string
				weekdays       []uint8
				hour, minute   uint8
				observed, next string
			}{
				{"a time inside the spring gap is skipped that day", "America/New_York", daily, 2, 30, "2024-03-09T12:00:00Z", "2024-03-11T06:30:00Z"},
				{"a time after the spring gap comes an hour earlier in UTC", "America/New_York", daily, 3, 0, "2024-03-10T06:00:00Z", "2024-03-10T07:00:00Z"},
				{"a repeated autumn time is its first pass", "America/New_York", daily, 1, 30, "2024-11-03T04:00:00Z", "2024-11-03T05:30:00Z"},
				{"the first pass consumes the day", "America/New_York", daily, 1, 30, "2024-11-03T05:30:00Z", "2024-11-04T06:30:00Z"},
				{"the second pass is not an occurrence", "America/New_York", daily, 1, 30, "2024-11-03T06:00:00Z", "2024-11-04T06:30:00Z"},
				{"a half-hour shift has a half-hour gap", "Australia/Lord_Howe", daily, 2, 15, "2024-10-05T12:00:00Z", "2024-10-06T15:15:00Z"},
				{"a half-hour shift repeats half an hour", "Australia/Lord_Howe", daily, 1, 45, "2024-04-06T12:00:00Z", "2024-04-06T14:45:00Z"},
				{"and repeats it once", "Australia/Lord_Howe", daily, 1, 45, "2024-04-06T15:15:00Z", "2024-04-07T15:15:00Z"},
				{"midnight that the clocks skip is skipped", "America/Havana", daily, 0, 0, "2024-03-09T12:00:00Z", "2024-03-11T04:00:00Z"},
				{"a repeated half past midnight is its first pass", "America/Havana", daily, 0, 30, "2024-11-03T03:00:00Z", "2024-11-03T04:30:00Z"},
				{"winter time as the shifted one", "Europe/Dublin", daily, 1, 30, "2024-03-30T12:00:00Z", "2024-04-01T00:30:00Z"},
				{"a whole day the zone never had is skipped", "Pacific/Apia", daily, 9, 0, "2011-12-29T20:00:00Z", "2011-12-30T19:00:00Z"},
				{"and its weekday waits a week", "Pacific/Apia", []uint8{5}, 9, 0, "2011-12-29T20:00:00Z", "2012-01-05T19:00:00Z"},
				{"a quarter-hour offset decides the local weekday", "Asia/Kathmandu", []uint8{1}, 0, 0, "2024-10-20T18:00:00Z", "2024-10-20T18:15:00Z"},
				{"a day ahead of UTC", "Pacific/Kiritimati", []uint8{1}, 0, 30, "2024-10-20T10:00:00Z", "2024-10-20T10:30:00Z"},
				{"a day behind UTC", "Pacific/Pago_Pago", []uint8{7}, 23, 30, "2024-10-21T10:00:00Z", "2024-10-21T10:30:00Z"},
			} {
				test.Run(row.name, func(test *testing.T) {
					expected := calendarAt(test, row.next)
					requireNext(test, requireCalendar(test, row.timezone, row.weekdays, row.hour, row.minute), calendarAt(test, row.observed), &expected)
				})
			}
		}},
		// The runner and the calendar together, on a clock that sleeps: each
		// row is a schedule, the instants at which the runner looks, and the
		// occurrences it must have started by then.
		{"runner_starts_each_local_occurrence_once", func(test *testing.T) {
			for _, row := range []struct {
				name, timezone string
				hour, minute   uint8
				start          string
				looks          []string
				started        [][]string
			}{
				{"spring gap", "America/New_York", 2, 30, "2024-03-09T07:00:00Z",
					[]string{"2024-03-09T07:29:30Z", "2024-03-09T07:30:00Z", "2024-03-10T07:30:00Z", "2024-03-11T06:29:30Z", "2024-03-11T06:30:00Z"},
					[][]string{nil, {"2024-03-09T07:30:00Z"}, nil, nil, {"2024-03-11T06:30:00Z"}}},
				{"autumn repeat", "America/New_York", 1, 30, "2024-11-03T05:00:00Z",
					[]string{"2024-11-03T05:30:00Z", "2024-11-03T06:30:00Z", "2024-11-04T06:29:30Z", "2024-11-04T06:30:00Z"},
					[][]string{{"2024-11-03T05:30:00Z"}, nil, nil, {"2024-11-04T06:30:00Z"}}},
				{"days asleep across the change are one run of the latest", "Europe/London", 9, 0, "2024-03-29T10:00:00Z",
					[]string{"2024-04-02T12:00:00Z", "2024-04-03T07:59:30Z", "2024-04-03T08:00:00Z"},
					[][]string{{"2024-04-02T08:00:00Z"}, nil, {"2024-04-03T08:00:00Z"}}},
			} {
				test.Run(row.name, func(test *testing.T) {
					test.Parallel()
					calendar := requireCalendar(test, row.timezone, []uint8{1, 2, 3, 4, 5, 6, 7}, row.hour, row.minute)
					start := calendarAt(test, row.start)
					clock := &sleepingClock{FakeClock: clockwork.NewFakeClockAt(time.UnixMilli(start))}
					cursor, err := calendar.Next(start)
					if err != nil || cursor == nil {
						test.Fatal(cursor, err)
					}
					ctx, cancel := context.WithCancel(test.Context())
					defer cancel()
					// Real time bounds only a failing run; a passing one never waits.
					waiting, release := context.WithTimeout(test.Context(), 10*time.Second)
					defer release()
					ticks := make(chan *int64, 1)
					runner := Runner{
						Clock: clock, Wake: make(chan struct{}),
						Tick: func(context.Context, bool) (Batch, error) {
							now := clock.Now().UnixMilli()
							if *cursor > now {
								ticks <- nil
								return Batch{NextMs: cursor}, nil
							}
							latest, next, err := calendar.Due(*cursor, now)
							if err != nil || next == nil {
								test.Error(next, err)
								return Batch{}, err
							}
							cursor = next
							ticks <- &latest
							return Batch{Work: []Work{{Job: protocol.NewJobID(), Completion: protocol.DispatchCompletionCommand}}, NextMs: cursor}, nil
						},
						Execute:  func(context.Context, Work) error { return nil },
						Finished: func(context.Context, protocol.JobID, error) error { return nil },
					}
					done := make(chan struct{})
					go func() {
						defer close(done)
						runner.Serve(ctx)
					}()
					// look waits for the tick after the wait that just ended. A
					// tick that starts a job is followed by the one its
					// completion wakes, and only then is the next wait armed.
					look := func() *int64 {
						var started *int64
						select {
						case started = <-ticks:
						case <-waiting.Done():
							test.Fatal("the runner did not look at the clock")
						}
						if started != nil {
							select {
							case again := <-ticks:
								if again != nil {
									test.Fatal("one look started the schedule twice")
								}
							case <-waiting.Done():
								test.Fatal("the finished job did not wake the runner")
							}
						}
						if err := clock.BlockUntilContext(waiting, 1); err != nil {
							test.Fatal(err)
						}
						return started
					}
					if look() != nil {
						test.Fatal("the schedule started before its first occurrence")
					}
					for index, value := range row.looks {
						// The system sleeps up to one recheck before the instant.
						clock.Slept(time.UnixMilli(calendarAt(test, value)).Sub(clock.Now()) - Recheck)
						clock.Advance(Recheck)
						started, expected := look(), row.started[index]
						if (started == nil) != (expected == nil) || started != nil && *started != calendarAt(test, expected[0]) {
							test.Fatalf("at %s started %v, expected %v", value, started, expected)
						}
					}
					cancel()
					<-done
				})
			}
		}},
		{"coalesces_long_downtime", func(test *testing.T) {
			calendar := requireCalendar(test, "Asia/Shanghai", []uint8{1, 2, 3, 4, 5}, 9, 0)
			expected := calendarAt(test, "2026-10-22T01:00:00Z")
			requireDue(test, calendar, calendarAt(test, "2020-01-06T01:00:00Z"),
				calendarAt(test, "2026-10-21T03:00:00Z"), calendarAt(test, "2026-10-21T01:00:00Z"), &expected)
		}},
		{"rejects_invalid_definitions", func(test *testing.T) {
			if _, err := NewCalendar("Mars/Olympus", []uint8{1}, 9, 0); err == nil {
				test.Fatal("invalid timezone was accepted")
			}
			for _, weekdays := range [][]uint8{{}, {0}, {8}, {1, 1}} {
				if _, err := NewCalendar("UTC", weekdays, 9, 0); err == nil {
					test.Fatalf("invalid weekdays were accepted: %v", weekdays)
				}
			}
			if _, err := NewCalendar("UTC", []uint8{1}, 24, 0); err == nil {
				test.Fatal("invalid hour was accepted")
			}
			if _, err := NewCalendar("UTC", []uint8{1}, 9, 60); err == nil {
				test.Fatal("invalid minute was accepted")
			}
		}},
		{"handles_calendar_bounds", func(test *testing.T) {
			calendar := requireCalendar(test, "UTC", []uint8{1, 2, 3, 4, 5, 6, 7}, 23, 59)
			if _, err := calendar.Next(math.MaxInt64); err == nil {
				test.Fatal("unrepresentable calendar timestamp was accepted")
			}
			maximum := time.Date(maximumYear, time.December, 31, 23, 59, 59, 999999999, time.UTC).UnixMilli()
			requireNext(test, calendar, maximum, nil)
			positive := requireCalendar(test, "Asia/Shanghai", []uint8{1}, 9, 0)
			if _, err := positive.Next(maximum); err == nil {
				test.Fatal("timezone overflow beyond the calendar boundary was accepted")
			}
		}},
	} {
		test.Run(scenario.name, scenario.check)
	}
}

func requireCalendar(test *testing.T, timezone string, weekdays []uint8, hour, minute uint8) *Calendar {
	test.Helper()
	calendar, err := NewCalendar(timezone, weekdays, hour, minute)
	if err != nil {
		test.Fatal(err)
	}
	return calendar
}

func calendarAt(test *testing.T, value string) int64 {
	test.Helper()
	instant, err := time.Parse(time.RFC3339, value)
	if err != nil {
		test.Fatal(err)
	}
	return instant.UnixMilli()
}

func requireNext(test *testing.T, calendar *Calendar, observed int64, expected *int64) {
	test.Helper()
	actual, err := calendar.Next(observed)
	if err != nil {
		test.Fatal(err)
	}
	if actual == nil && expected == nil {
		return
	}
	if actual == nil || expected == nil || *actual != *expected {
		test.Fatalf("next occurrence at %d = %v, expected %v", observed, actual, expected)
	}
}

func requireDue(test *testing.T, calendar *Calendar, first, observed, latest int64, expected *int64) {
	test.Helper()
	actual, next, err := calendar.Due(first, observed)
	if err != nil {
		test.Fatal(err)
	}
	if actual != latest || next == nil != (expected == nil) || next != nil && *next != *expected {
		test.Fatalf("due occurrence = %d/%v, expected %d/%v", actual, next, latest, expected)
	}
}
