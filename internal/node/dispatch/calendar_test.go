package dispatch

import (
	"math"
	"testing"
	"time"
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
