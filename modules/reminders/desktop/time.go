package remindersdesktop

import (
	"fmt"
	"math"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	MinuteMs = 60_000
	HourMs   = 3_600_000
	DayMs    = 86_400_000
)

// Translate resolves a locale key with optional alternating name/value arguments.
type Translate func(key string, args ...any) string

// ReminderItem is a reminder entry projected for the desktop UI.
type ReminderItem struct {
	Reminder    protocol.Reminder
	Revision    *uint64
	Unavailable bool
}

// StatusBadge describes the label and tone for a reminder's status.
type StatusBadge struct {
	LabelKey string
	Tone     kit.Tone
}

// SortReminders returns a sorted copy ordered by (due_ms ?? MaxInt64, title).
func SortReminders(items []ReminderItem) []ReminderItem {
	out := slices.Clone(items)
	slices.SortStableFunc(out, func(a, b ReminderItem) int {
		da := int64(math.MaxInt64)
		if a.Reminder.DueMs != nil {
			da = *a.Reminder.DueMs
		}
		db := int64(math.MaxInt64)
		if b.Reminder.DueMs != nil {
			db = *b.Reminder.DueMs
		}
		if da < db {
			return -1
		}
		if da > db {
			return 1
		}
		return strings.Compare(strings.ToLower(a.Reminder.Title), strings.ToLower(b.Reminder.Title))
	})
	return out
}

// IsOverdue reports whether a reminder is open and past (or at) its due time.
func IsOverdue(item protocol.Reminder, nowMs int64) bool {
	return !item.Completed && item.DueMs != nil && *item.DueMs <= nowMs
}

// ReminderStatus resolves the status badge for a reminder.
// Completion wins; an open reminder past its due time is overdue even after notification.
func ReminderStatus(item protocol.Reminder, nowMs int64) StatusBadge {
	if item.Completed {
		return StatusBadge{LabelKey: "reminders_completed", Tone: kit.Positive}
	}
	if IsOverdue(item, nowMs) {
		return StatusBadge{LabelKey: "reminders_overdue", Tone: kit.Negative}
	}
	return StatusBadge{LabelKey: "reminders_pending", Tone: kit.Caution}
}

// DeliveryLabelKey returns the translation key for a delivery job status.
func DeliveryLabelKey(status protocol.DispatchStatus) string {
	switch status {
	case protocol.DispatchStatusCompleted:
		return "reminders_notified"
	case protocol.DispatchStatusFailed:
		return "reminders_delivery_failed"
	case protocol.DispatchStatusRunning, protocol.DispatchStatusQueued:
		return "reminders_waiting"
	case protocol.DispatchStatusCancelled:
		return "reminders_delivery_cancelled"
	default:
		return "reminders_delivery_unknown"
	}
}

// DeliveryTime returns the best timestamp for a reminder delivery job.
func DeliveryTime(job protocol.DispatchJob) int64 {
	if job.FinishedMs != nil {
		return *job.FinishedMs
	}
	if job.StartedMs != nil {
		return *job.StartedMs
	}
	return job.CreatedMs
}

func isoWeekday(w time.Weekday) uint8 {
	if w == time.Sunday {
		return 7
	}
	return uint8(w)
}

func localLoc(loc *time.Location) *time.Location {
	if loc == nil {
		return time.Local
	}
	return loc
}

func startOfDay(ms int64, loc *time.Location) time.Time {
	t := time.UnixMilli(ms).In(localLoc(loc))
	return time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, t.Location())
}

// FormatClock formats HH:mm or HH:mm:ss in loc.
func FormatClock(ms int64, seconds bool, loc *time.Location) string {
	t := time.UnixMilli(ms).In(localLoc(loc))
	if seconds {
		return fmt.Sprintf("%02d:%02d:%02d", t.Hour(), t.Minute(), t.Second())
	}
	return fmt.Sprintf("%02d:%02d", t.Hour(), t.Minute())
}

// FormatDate formats month/day + weekday in loc.
func FormatDate(ms int64, loc *time.Location, t Translate) string {
	dt := time.UnixMilli(ms).In(localLoc(loc))
	monthName := t(fmt.Sprintf("reminders_month_%d", int(dt.Month())))
	monthDay := t("reminders_month_day", "month", monthName, "day", dt.Day())
	weekdayName := t(fmt.Sprintf("reminders_weekday_%d", isoWeekday(dt.Weekday())))
	return monthDay + " " + weekdayName
}

// FormatDayLabel formats Today / Yesterday / Tomorrow or month/day + weekday.
func FormatDayLabel(ms, nowMs int64, loc *time.Location, t Translate) string {
	l := localLoc(loc)
	days := int(math.Round(startOfDay(ms, l).Sub(startOfDay(nowMs, l)).Hours() / 24))
	switch days {
	case 0:
		return t("reminders_today")
	case -1:
		return t("reminders_yesterday")
	case 1:
		return t("reminders_tomorrow")
	}
	return FormatDate(ms, l, t)
}

// FormatWhen formats day label + HH:mm.
func FormatWhen(ms, nowMs int64, loc *time.Location, t Translate) string {
	return FormatDayLabel(ms, nowMs, loc, t) + " " + FormatClock(ms, false, loc)
}

// FormatRelative formats remaining or elapsed time relative to nowMs.
func FormatRelative(ms, nowMs int64, t Translate) string {
	past := ms <= nowMs
	diff := ms - nowMs
	if diff < 0 {
		diff = -diff
	}
	minutes := int64(math.Round(float64(diff) / float64(MinuteMs)))
	if minutes < 1 {
		minutes = 1
	}
	if minutes < 60 {
		if past {
			return t("reminders_ago_minutes", "minutes", minutes)
		}
		return t("reminders_in_minutes", "minutes", minutes)
	}
	if minutes < 24*60 {
		hours := minutes / 60
		rem := minutes % 60
		if rem == 0 {
			if past {
				return t("reminders_ago_hours", "hours", hours)
			}
			return t("reminders_in_hours", "hours", hours)
		}
		if past {
			return t("reminders_ago_hours_minutes", "hours", hours, "minutes", rem)
		}
		return t("reminders_in_hours_minutes", "hours", hours, "minutes", rem)
	}
	days := minutes / (24 * 60)
	if days == 1 {
		if past {
			return t("reminders_ago_days_one")
		}
		return t("reminders_in_days_one")
	}
	if past {
		return t("reminders_ago_days_other", "days", days)
	}
	return t("reminders_in_days_other", "days", days)
}

// CombineLocalDateTime merges date and time picker values into an unambiguous local millisecond timestamp.
func CombineLocalDateTime(datePart, timePart time.Time, original int64, loc *time.Location) (int64, bool) {
	if datePart.IsZero() || timePart.IsZero() {
		return 0, false
	}
	l := localLoc(loc)
	year, month, day := datePart.In(l).Date()
	hour, minute, second := timePart.In(l).Clock()
	wall := time.Date(year, month, day, hour, minute, second, 0, time.UTC)
	possible := map[int64]bool{}
	for _, observed := range []time.Time{wall.Add(-48 * time.Hour), wall, wall.Add(48 * time.Hour)} {
		_, offset := observed.In(l).Zone()
		at := wall.Add(-time.Duration(offset) * time.Second).In(l)
		cy, cm, cd := at.Date()
		ch, cmin, cs := at.Clock()
		if cy == year && cm == month && cd == day && ch == hour && cmin == minute && cs == second {
			possible[at.UnixMilli()] = true
		}
	}
	if len(possible) != 1 {
		return 0, false
	}
	for at := range possible {
		if original > 0 && time.UnixMilli(original).Truncate(time.Second).UnixMilli() == at {
			return original, true
		}
		return at, true
	}
	return 0, false
}

// ValidateDueMs allows an unchanged past due_ms when editing an existing reminder,
// while requiring any new or modified due_ms to be strictly in the future.
func ValidateDueMs(dueMs int64, originalDueMs *int64, nowMs int64) bool {
	if originalDueMs != nil && *originalDueMs == dueMs {
		return true
	}
	return dueMs > nowMs
}
