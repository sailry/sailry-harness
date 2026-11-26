package scheduledtasksdesktop

import (
	"fmt"
	"math"
	"slices"
	"strconv"
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

// TaskItem pairs a scheduled task entry with live dispatch runtime state.
type TaskItem struct {
	Task        protocol.ScheduledTask
	Revision    *uint64
	NextMs      *int64
	Unavailable bool
	Status      protocol.DispatchStatus
	StartedMs   *int64
}

// StatusBadge describes the precedence-resolved status label and tone for a task.
type StatusBadge struct {
	LabelKey string
	Tone     kit.Tone
	Outline  bool
}

// Ribbon describes today's interval ticks as [0, 1) fractions of the local day.
type Ribbon struct {
	Ticks    []float64
	Overflow bool
	Now      float64
}

// RunCounts tallies run outcomes in a history window.
type RunCounts struct {
	Completed int
	Failed    int
	Cancelled int
	Running   int
	Queued    int
	Unknown   int
}

// TaskStatus resolves the status badge with precedence:
// unavailable > running > queued > paused > failed > pending > completed.
func TaskStatus(item TaskItem) StatusBadge {
	if item.Unavailable {
		return StatusBadge{LabelKey: "scheduled_tasks_execution_unavailable", Tone: kit.Neutral, Outline: true}
	}
	if item.Status == protocol.DispatchStatusRunning {
		return StatusBadge{LabelKey: "scheduled_tasks_running", Tone: kit.Info}
	}
	if item.Status == protocol.DispatchStatusQueued {
		return StatusBadge{LabelKey: "scheduled_tasks_queued", Tone: kit.Info}
	}
	if !item.Task.Enabled {
		return StatusBadge{LabelKey: "scheduled_tasks_paused", Tone: kit.Neutral, Outline: true}
	}
	if item.Status == protocol.DispatchStatusFailed {
		return StatusBadge{LabelKey: "scheduled_tasks_run_failed", Tone: kit.Negative}
	}
	if item.NextMs != nil {
		return StatusBadge{LabelKey: "scheduled_tasks_scheduled", Tone: kit.Neutral}
	}
	if item.Status == protocol.DispatchStatusCompleted {
		return StatusBadge{LabelKey: "scheduled_tasks_completed", Tone: kit.Positive}
	}
	if item.Status == protocol.DispatchStatusCancelled {
		return StatusBadge{LabelKey: "scheduled_tasks_cancelled", Tone: kit.Neutral}
	}
	return StatusBadge{LabelKey: "scheduled_tasks_finished", Tone: kit.Neutral}
}

// RunLabelKey returns the translation key for a dispatch job status.
func RunLabelKey(status protocol.DispatchStatus) string {
	switch status {
	case protocol.DispatchStatusQueued:
		return "scheduled_tasks_queued"
	case protocol.DispatchStatusRunning:
		return "scheduled_tasks_running"
	case protocol.DispatchStatusCompleted:
		return "scheduled_tasks_completed"
	case protocol.DispatchStatusFailed:
		return "scheduled_tasks_run_failed"
	case protocol.DispatchStatusCancelled:
		return "scheduled_tasks_cancelled"
	case protocol.DispatchStatusUnknown:
		return "scheduled_tasks_unknown"
	default:
		return "scheduled_tasks_finished"
	}
}

// ElapsedMinutes returns whole elapsed minutes for a running job, or (0, false) otherwise.
func ElapsedMinutes(item TaskItem, nowMs int64) (int64, bool) {
	if item.Status != protocol.DispatchStatusRunning || item.StartedMs == nil {
		return 0, false
	}
	diff := nowMs - *item.StartedMs
	if diff <= 0 {
		return 0, true
	}
	return diff / MinuteMs, true
}

// FailureStreak counts consecutive failed runs from the newest end of runs.
func FailureStreak(runs []protocol.ScheduledTaskHistoryEntry) int {
	count := 0
	for _, run := range runs {
		if run.Job.Status != protocol.DispatchStatusFailed {
			break
		}
		count++
	}
	return count
}

// TallyRuns counts runs by dispatch status.
func TallyRuns(runs []protocol.ScheduledTaskHistoryEntry) RunCounts {
	var counts RunCounts
	for _, run := range runs {
		switch run.Job.Status {
		case protocol.DispatchStatusCompleted:
			counts.Completed++
		case protocol.DispatchStatusFailed:
			counts.Failed++
		case protocol.DispatchStatusCancelled:
			counts.Cancelled++
		case protocol.DispatchStatusRunning:
			counts.Running++
		case protocol.DispatchStatusQueued:
			counts.Queued++
		case protocol.DispatchStatusUnknown:
			counts.Unknown++
		}
	}
	return counts
}

// RecentSummaryText formats a short summary of run outcomes.
func RecentSummaryText(runs []protocol.ScheduledTaskHistoryEntry, t Translate) string {
	counts := TallyRuns(runs)
	parts := make([]string, 0, 4)
	if counts.Completed > 0 {
		parts = append(parts, t("scheduled_tasks_count_completed", "count", counts.Completed))
	}
	if counts.Failed > 0 {
		parts = append(parts, t("scheduled_tasks_count_failed", "count", counts.Failed))
	}
	if counts.Cancelled > 0 {
		parts = append(parts, t("scheduled_tasks_count_cancelled", "count", counts.Cancelled))
	}
	if counts.Running > 0 {
		parts = append(parts, t("scheduled_tasks_count_running", "count", counts.Running))
	}
	if len(parts) == 0 {
		return t("scheduled_tasks_no_runs")
	}
	return strings.Join(parts, " · ")
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

// LocalWeekday returns ISO weekday 1 (Mon) .. 7 (Sun) in loc.
func LocalWeekday(ms int64, loc *time.Location) uint8 {
	return isoWeekday(time.UnixMilli(ms).In(localLoc(loc)).Weekday())
}

// WeeklyOffsetMinutes returns minutes east of UTC for the weekly schedule at nextMs.
func WeeklyOffsetMinutes(data protocol.DispatchTimingWeeklyPayload, nextMs int64) int {
	utc := time.UnixMilli(nextMs).UTC()
	minutes := ((int(data.Hour)*60+int(data.Minute)-utc.Hour()*60-utc.Minute())%1440 + 1440) % 1440
	if minutes > 840 {
		return minutes - 1440
	}
	return minutes
}

// ZoneWeekday returns ISO weekday 1..7 at ms for a fixed UTC offset in minutes.
func ZoneWeekday(ms int64, offsetMinutes int) uint8 {
	shifted := time.UnixMilli(ms + int64(offsetMinutes)*MinuteMs).UTC()
	return isoWeekday(shifted.Weekday())
}

// WeeklyTodayWeekday returns today's ISO weekday in the weekly schedule's timezone.
func WeeklyTodayWeekday(data protocol.DispatchTimingWeeklyPayload, nextMs *int64, nowMs int64, loc *time.Location) uint8 {
	if data.Timezone != "" {
		if tz, err := time.LoadLocation(data.Timezone); err == nil {
			return isoWeekday(time.UnixMilli(nowMs).In(tz).Weekday())
		}
	}
	if nextMs == nil {
		return LocalWeekday(nowMs, loc)
	}
	return ZoneWeekday(nowMs, WeeklyOffsetMinutes(data, *nextMs))
}

func timingOnce(timing protocol.DispatchTiming) (protocol.DispatchTimingOncePayload, bool) {
	if timing.Kind != "once" {
		return protocol.DispatchTimingOncePayload{}, false
	}
	switch v := timing.Data.(type) {
	case protocol.DispatchTimingOncePayload:
		return v, true
	case *protocol.DispatchTimingOncePayload:
		if v != nil {
			return *v, true
		}
	}
	return protocol.DispatchTimingOncePayload{}, false
}

func timingEvery(timing protocol.DispatchTiming) (protocol.DispatchTimingEveryPayload, bool) {
	if timing.Kind != "every" {
		return protocol.DispatchTimingEveryPayload{}, false
	}
	switch v := timing.Data.(type) {
	case protocol.DispatchTimingEveryPayload:
		return v, true
	case *protocol.DispatchTimingEveryPayload:
		if v != nil {
			return *v, true
		}
	}
	return protocol.DispatchTimingEveryPayload{}, false
}

func timingWeekly(timing protocol.DispatchTiming) (protocol.DispatchTimingWeeklyPayload, bool) {
	if timing.Kind != "weekly" {
		return protocol.DispatchTimingWeeklyPayload{}, false
	}
	switch v := timing.Data.(type) {
	case protocol.DispatchTimingWeeklyPayload:
		return v, true
	case *protocol.DispatchTimingWeeklyPayload:
		if v != nil {
			return *v, true
		}
	}
	return protocol.DispatchTimingWeeklyPayload{}, false
}

// Upcoming computes up to count upcoming run timestamps in milliseconds,
// matching the scheduled-tasks plugin preview semantics.
func Upcoming(item TaskItem, nowMs int64, count int) []int64 {
	if !item.Task.Enabled || count <= 0 {
		return nil
	}
	if once, ok := timingOnce(item.Task.Timing); ok {
		if once.AtMs > nowMs {
			return []int64{once.AtMs}
		}
		return nil
	}
	if every, ok := timingEvery(item.Task.Timing); ok {
		if every.IntervalMs == 0 {
			return nil
		}
		interval := int64(every.IntervalMs)
		if interval <= 0 {
			return nil
		}
		var first int64
		if item.NextMs != nil {
			first = *item.NextMs
		} else if every.AnchorMs >= nowMs {
			first = every.AnchorMs
		} else {
			diff := nowMs - every.AnchorMs
			steps := (diff + interval - 1) / interval
			first = every.AnchorMs + steps*interval
		}
		out := make([]int64, count)
		for i := range count {
			out[i] = first + int64(i)*interval
		}
		return out
	}
	if weekly, ok := timingWeekly(item.Task.Timing); ok {
		if item.NextMs == nil || len(weekly.Weekdays) == 0 {
			return nil
		}
		next := *item.NextMs
		if tz, err := time.LoadLocation(weekly.Timezone); err == nil {
			start := time.UnixMilli(next).In(tz)
			year, month, day := start.Date()
			out := make([]int64, 0, count)
			maxDays := 7*count + 7
			for offset := 0; len(out) < count && offset < maxDays; offset++ {
				candidate := time.Date(year, month, day+offset, int(weekly.Hour), int(weekly.Minute), 0, 0, tz)
				if !slices.Contains(weekly.Weekdays, isoWeekday(candidate.Weekday())) {
					continue
				}
				ms := candidate.UnixMilli()
				if ms < next {
					continue
				}
				out = append(out, ms)
			}
			return out
		}
		base := ZoneWeekday(next, WeeklyOffsetMinutes(weekly, next))
		out := make([]int64, 0, count)
		maxDays := 7*count + 7
		for day := 0; len(out) < count && day < maxDays; day++ {
			wd := uint8((int(base)-1+day)%7 + 1)
			if slices.Contains(weekly.Weekdays, wd) {
				out = append(out, next+int64(day)*DayMs)
			}
		}
		return out
	}
	return nil
}

// ComputeRibbon computes today's interval ticks in loc; Overflow is true when ticks reach limit.
func ComputeRibbon(data protocol.DispatchTimingEveryPayload, nowMs int64, loc *time.Location, limit int) Ribbon {
	if limit <= 0 {
		limit = 96
	}
	startTime := startOfDay(nowMs, loc)
	start := startTime.UnixMilli()
	end := start + DayMs
	nowFrac := float64(nowMs-start) / float64(DayMs)
	if nowFrac < 0 {
		nowFrac = 0
	} else if nowFrac > 1 {
		nowFrac = 1
	}
	interval := int64(data.IntervalMs)
	if interval <= 0 {
		return Ribbon{Now: nowFrac}
	}
	at := data.AnchorMs
	if at < start {
		diff := start - at
		steps := (diff + interval - 1) / interval
		at += steps * interval
	}
	ticks := make([]float64, 0, 16)
	for ; at < end; at += interval {
		if len(ticks) == limit {
			return Ribbon{Overflow: true, Now: nowFrac}
		}
		ticks = append(ticks, float64(at-start)/float64(DayMs))
	}
	return Ribbon{Ticks: ticks, Now: nowFrac}
}

// FormatClock formats HH:mm or HH:mm:ss in loc.
func FormatClock(ms int64, seconds bool, loc *time.Location) string {
	t := time.UnixMilli(ms).In(localLoc(loc))
	if seconds {
		return fmt.Sprintf("%02d:%02d:%02d", t.Hour(), t.Minute(), t.Second())
	}
	return fmt.Sprintf("%02d:%02d", t.Hour(), t.Minute())
}

// FormatWallClock formats an explicit hour and minute as HH:mm.
func FormatWallClock(hour, minute uint8) string {
	return fmt.Sprintf("%02d:%02d", hour, minute)
}

// ParseWallClock parses HH:mm or H:mm into hour (0..23) and minute (0..59).
func ParseWallClock(raw string) (uint8, uint8, bool) {
	trimmed := strings.TrimSpace(raw)
	hStr, mStr, ok := strings.Cut(trimmed, ":")
	if !ok || len(hStr) < 1 || len(hStr) > 2 || len(mStr) != 2 {
		return 0, 0, false
	}
	h, errH := strconv.Atoi(hStr)
	m, errM := strconv.Atoi(mStr)
	if errH != nil || errM != nil || h < 0 || h > 23 || m < 0 || m > 59 {
		return 0, 0, false
	}
	return uint8(h), uint8(m), true
}

// FormatDayLabel formats a date relative to nowMs (Today/Yesterday/Tomorrow or "Mon D Weekday").
func FormatDayLabel(ms, nowMs int64, loc *time.Location, t Translate) string {
	l := localLoc(loc)
	dayStart := startOfDay(ms, l)
	nowStart := startOfDay(nowMs, l)
	days := int(math.Round(dayStart.Sub(nowStart).Hours() / 24))
	switch days {
	case 0:
		return t("scheduled_tasks_today")
	case -1:
		return t("scheduled_tasks_yesterday")
	case 1:
		return t("scheduled_tasks_tomorrow")
	}
	dt := time.UnixMilli(ms).In(l)
	monthName := t(fmt.Sprintf("scheduled_tasks_month_%d", int(dt.Month())))
	monthDay := t("scheduled_tasks_month_day", "month", monthName, "day", dt.Day())
	weekdayName := t(fmt.Sprintf("scheduled_tasks_weekday_%d", isoWeekday(dt.Weekday())))
	return monthDay + " " + weekdayName
}

// FormatWhen formats day label + HH:mm.
func FormatWhen(ms, nowMs int64, loc *time.Location, t Translate) string {
	return FormatDayLabel(ms, nowMs, loc, t) + " " + FormatClock(ms, false, loc)
}

// FormatRelative formats a future timestamp relative to nowMs.
func FormatRelative(ms, nowMs int64, loc *time.Location, t Translate) string {
	l := localLoc(loc)
	dayStart := startOfDay(ms, l)
	nowStart := startOfDay(nowMs, l)
	days := int(math.Round(dayStart.Sub(nowStart).Hours() / 24))
	if days >= 1 {
		if days == 1 {
			return t("scheduled_tasks_in_days_one")
		}
		return t("scheduled_tasks_in_days_other", "days", days)
	}
	minutes := int64(math.Ceil(float64(ms-nowMs) / float64(MinuteMs)))
	if minutes < 1 {
		minutes = 1
	}
	if minutes < 60 {
		return t("scheduled_tasks_in_minutes", "minutes", minutes)
	}
	hours := minutes / 60
	rem := minutes % 60
	if rem == 0 {
		return t("scheduled_tasks_in_hours", "hours", hours)
	}
	return t("scheduled_tasks_in_hours_minutes", "hours", hours, "minutes", rem)
}

// FormatInterval formats a repeating interval duration.
func FormatInterval(intervalMs uint64, t Translate) string {
	if intervalMs > 0 && intervalMs%DayMs == 0 {
		days := intervalMs / DayMs
		if days == 1 {
			return t("scheduled_tasks_every_days_one")
		}
		return t("scheduled_tasks_every_days_other", "count", days)
	}
	if intervalMs > 0 && intervalMs%HourMs == 0 {
		hours := intervalMs / HourMs
		if hours == 1 {
			return t("scheduled_tasks_every_hours_one")
		}
		return t("scheduled_tasks_every_hours_other", "count", hours)
	}
	if intervalMs > 0 && intervalMs%MinuteMs == 0 {
		return t("scheduled_tasks_every_minutes", "count", intervalMs/MinuteMs)
	}
	return t("scheduled_tasks_every_seconds", "count", intervalMs/1000)
}

// FormatScheduleSummary returns a concise human-readable summary of a task's schedule.
func FormatScheduleSummary(timing protocol.DispatchTiming, nowMs int64, loc *time.Location, t Translate) string {
	if every, ok := timingEvery(timing); ok {
		return FormatInterval(every.IntervalMs, t)
	}
	if once, ok := timingOnce(timing); ok {
		return t("scheduled_tasks_once") + " " + FormatWhen(once.AtMs, nowMs, loc, t)
	}
	if weekly, ok := timingWeekly(timing); ok {
		sorted := slices.Clone([]byte(weekly.Weekdays))
		slices.Sort(sorted)
		var days string
		switch {
		case len(sorted) == 7:
			days = t("scheduled_tasks_daily")
		case slices.Equal(sorted, []byte{1, 2, 3, 4, 5}):
			days = t("scheduled_tasks_workdays")
		default:
			parts := make([]string, 0, len(sorted))
			for _, wd := range sorted {
				if wd >= 1 && wd <= 7 {
					parts = append(parts, t(fmt.Sprintf("scheduled_tasks_weekday_%d", wd)))
				}
			}
			days = strings.Join(parts, " ")
		}
		summary := days + " " + FormatWallClock(weekly.Hour, weekly.Minute)
		if weekly.Timezone != "" {
			summary += " · " + weekly.Timezone
		}
		return summary
	}
	return "—"
}

// CombineLocalDateTime merges a date picker value and a time picker value into
// an unambiguous local Unix millisecond timestamp.
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
