package dispatch

import (
	"math"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func First(timing protocol.DispatchTiming, timestamp int64) (int64, error) {
	var first int64
	switch timing.Kind {
	case "once":
		value, ok := timingValue[protocol.DispatchTimingOncePayload](timing.Data)
		if !ok {
			return 0, invalid("schedule timing is invalid")
		}
		first = value.AtMs
	case "every":
		value, ok := timingValue[protocol.DispatchTimingEveryPayload](timing.Data)
		if !ok {
			return 0, invalid("schedule timing is invalid")
		}
		interval, err := timingInterval(value.IntervalMs)
		if err != nil {
			return 0, err
		}
		if value.AnchorMs > math.MaxInt64-interval {
			return 0, invalid("schedule time is not representable")
		}
		first = value.AnchorMs
	case "weekly":
		value, ok := timingValue[protocol.DispatchTimingWeeklyPayload](timing.Data)
		if !ok {
			return 0, invalid("schedule timing is invalid")
		}
		calendar, err := NewCalendar(value.Timezone, value.Weekdays, value.Hour, value.Minute)
		if err != nil {
			return 0, err
		}
		next, err := calendar.Next(timestamp)
		if err != nil {
			return 0, err
		}
		if next == nil {
			return 0, invalid("schedule has no future occurrence")
		}
		first = *next
	default:
		return 0, invalid("schedule timing is invalid")
	}
	if first < 0 {
		return 0, invalid("schedule time is invalid")
	}
	return first, nil
}

func Occurrence(schedule protocol.DispatchSchedule, timestamp int64) (int64, *int64, error) {
	if schedule.NextMs == nil || *schedule.NextMs < 0 || *schedule.NextMs > timestamp {
		return 0, nil, invalid("due schedule has an invalid cursor")
	}
	first := *schedule.NextMs
	switch schedule.Timing.Kind {
	case "once":
		return first, nil, nil
	case "every":
		value, ok := timingValue[protocol.DispatchTimingEveryPayload](schedule.Timing.Data)
		if !ok {
			return 0, nil, invalid("schedule timing is invalid")
		}
		interval, err := timingInterval(value.IntervalMs)
		if err != nil {
			return 0, nil, err
		}
		latest := first + ((timestamp-first)/interval)*interval
		if latest > math.MaxInt64-interval {
			return latest, nil, nil
		}
		next := latest + interval
		return latest, &next, nil
	case "weekly":
		value, ok := timingValue[protocol.DispatchTimingWeeklyPayload](schedule.Timing.Data)
		if !ok {
			return 0, nil, invalid("schedule timing is invalid")
		}
		calendar, err := NewCalendar(value.Timezone, value.Weekdays, value.Hour, value.Minute)
		if err != nil {
			return 0, nil, err
		}
		return calendar.Due(first, timestamp)
	default:
		return 0, nil, invalid("schedule timing is invalid")
	}
}

func timingInterval(milliseconds uint64) (int64, error) {
	if milliseconds == 0 || milliseconds > math.MaxInt64 {
		return 0, invalid("schedule interval must be positive and representable")
	}
	return int64(milliseconds), nil
}

func timingValue[Value any](input any) (Value, bool) {
	switch value := input.(type) {
	case Value:
		return value, true
	case *Value:
		if value != nil {
			return *value, true
		}
	}
	var empty Value
	return empty, false
}
