package dispatch

import (
	"time"
	_ "time/tzdata"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	minimumYear = -262143
	maximumYear = 262142
)

type Calendar struct {
	timezone *time.Location
	weekdays [8]bool
	hour     int
	minute   int
}

func NewCalendar(timezone string, weekdays []uint8, hour, minute uint8) (*Calendar, error) {
	if timezone == "" || timezone == "Local" {
		return nil, invalid("schedule timezone is invalid")
	}
	location, err := time.LoadLocation(timezone)
	if err != nil {
		return nil, invalid("schedule timezone is invalid")
	}
	calendar := &Calendar{timezone: location, hour: int(hour), minute: int(minute)}
	if len(weekdays) == 0 {
		return nil, invalid("schedule weekdays are invalid")
	}
	for _, day := range weekdays {
		if day < 1 || day > 7 || calendar.weekdays[day] {
			return nil, invalid("schedule weekdays are invalid")
		}
		calendar.weekdays[day] = true
	}
	if hour > 23 || minute > 59 {
		return nil, invalid("schedule local time is invalid")
	}
	return calendar, nil
}

func (calendar *Calendar) date(timestamp int64) (time.Time, error) {
	utc := time.UnixMilli(timestamp).UTC()
	if utc.Year() < minimumYear || utc.Year() > maximumYear {
		return time.Time{}, invalid("schedule time is invalid")
	}
	local := utc.In(calendar.timezone)
	if local.Year() < minimumYear || local.Year() > maximumYear {
		return time.Time{}, invalid("schedule time is invalid")
	}
	return time.Date(local.Year(), local.Month(), local.Day(), 0, 0, 0, 0, time.UTC), nil
}

func (calendar *Calendar) occurrence(date time.Time) *int64 {
	weekday := int(date.Weekday())
	if weekday == 0 {
		weekday = 7
	}
	if !calendar.weekdays[weekday] {
		return nil
	}
	local := time.Date(date.Year(), date.Month(), date.Day(), calendar.hour, calendar.minute, 0, 0, calendar.timezone)
	start, end := local.ZoneBounds()
	offsets := []int{}
	addOffset := func(instant time.Time) {
		_, offset := instant.Zone()
		for _, existing := range offsets {
			if existing == offset {
				return
			}
		}
		offsets = append(offsets, offset)
	}
	addOffset(local)
	if !start.IsZero() {
		addOffset(start.Add(-time.Nanosecond))
	}
	if !end.IsZero() {
		addOffset(end)
	}
	wall := time.Date(date.Year(), date.Month(), date.Day(), calendar.hour, calendar.minute, 0, 0, time.UTC)
	var earliest *int64
	for _, offset := range offsets {
		candidate := wall.Add(-time.Duration(offset) * time.Second)
		value := candidate.In(calendar.timezone)
		if value.Year() != date.Year() || value.Month() != date.Month() || value.Day() != date.Day() ||
			value.Hour() != calendar.hour || value.Minute() != calendar.minute || value.Second() != 0 {
			continue
		}
		timestamp := candidate.UnixMilli()
		if earliest == nil || timestamp < *earliest {
			earliest = &timestamp
		}
	}
	return earliest
}

func (calendar *Calendar) Next(timestamp int64) (*int64, error) {
	date, err := calendar.date(timestamp)
	if err != nil {
		return nil, err
	}
	for {
		if occurrence := calendar.occurrence(date); occurrence != nil && *occurrence > timestamp {
			return occurrence, nil
		}
		if date.Year() == maximumYear && date.Month() == time.December && date.Day() == 31 {
			return nil, nil
		}
		date = date.AddDate(0, 0, 1)
	}
}

func (calendar *Calendar) Due(first, timestamp int64) (int64, *int64, error) {
	if first > timestamp {
		return 0, nil, invalid("schedule occurrence is not due")
	}
	lower, err := calendar.date(first)
	if err != nil {
		return 0, nil, err
	}
	date, err := calendar.date(timestamp)
	if err != nil {
		return 0, nil, err
	}
	latest := first
	for !date.Before(lower) {
		if occurrence := calendar.occurrence(date); occurrence != nil && *occurrence <= timestamp {
			latest = max(latest, *occurrence)
			break
		}
		if date.Year() == minimumYear && date.Month() == time.January && date.Day() == 1 {
			break
		}
		date = date.AddDate(0, 0, -1)
	}
	next, err := calendar.Next(timestamp)
	return latest, next, err
}

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
