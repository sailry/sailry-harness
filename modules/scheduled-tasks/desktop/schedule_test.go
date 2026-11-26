package scheduledtasksdesktop

import (
	"slices"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testTranslator(t *testing.T, lang string) Translate {
	t.Helper()
	cat := i18n.New(Locales)
	return func(key string, args ...any) string {
		return cat.In(lang, key, args...)
	}
}

func ptrInt64(v int64) *int64 { return &v }

func TestTaskStatusPrecedence(t *testing.T) {
	next := ptrInt64(1_700_000_000_000)

	cases := []struct {
		name        string
		item        TaskItem
		wantKey     string
		wantTone    kit.Tone
		wantOutline bool
	}{
		{
			name:        "unavailable beats running",
			item:        TaskItem{Unavailable: true, Status: protocol.DispatchStatusRunning, Task: protocol.ScheduledTask{Enabled: true}, NextMs: next},
			wantKey:     "scheduled_tasks_execution_unavailable",
			wantTone:    kit.Neutral,
			wantOutline: true,
		},
		{
			name:     "running beats paused and failed",
			item:     TaskItem{Status: protocol.DispatchStatusRunning, Task: protocol.ScheduledTask{Enabled: false}, NextMs: next},
			wantKey:  "scheduled_tasks_running",
			wantTone: kit.Info,
		},
		{
			name:     "queued beats paused",
			item:     TaskItem{Status: protocol.DispatchStatusQueued, Task: protocol.ScheduledTask{Enabled: false}, NextMs: next},
			wantKey:  "scheduled_tasks_queued",
			wantTone: kit.Info,
		},
		{
			name:        "paused beats failed and pending",
			item:        TaskItem{Status: protocol.DispatchStatusFailed, Task: protocol.ScheduledTask{Enabled: false}, NextMs: next},
			wantKey:     "scheduled_tasks_paused",
			wantTone:    kit.Neutral,
			wantOutline: true,
		},
		{
			name:     "failed beats pending",
			item:     TaskItem{Status: protocol.DispatchStatusFailed, Task: protocol.ScheduledTask{Enabled: true}, NextMs: next},
			wantKey:  "scheduled_tasks_run_failed",
			wantTone: kit.Negative,
		},
		{
			name:     "pending beats completed",
			item:     TaskItem{Status: protocol.DispatchStatusCompleted, Task: protocol.ScheduledTask{Enabled: true}, NextMs: next},
			wantKey:  "scheduled_tasks_scheduled",
			wantTone: kit.Neutral,
		},
		{
			name:     "completed when no next run",
			item:     TaskItem{Status: protocol.DispatchStatusCompleted, Task: protocol.ScheduledTask{Enabled: true}},
			wantKey:  "scheduled_tasks_completed",
			wantTone: kit.Positive,
		},
		{
			name:     "cancelled fallback",
			item:     TaskItem{Status: protocol.DispatchStatusCancelled, Task: protocol.ScheduledTask{Enabled: true}},
			wantKey:  "scheduled_tasks_cancelled",
			wantTone: kit.Neutral,
		},
		{
			name:     "finished fallback",
			item:     TaskItem{Task: protocol.ScheduledTask{Enabled: true}},
			wantKey:  "scheduled_tasks_finished",
			wantTone: kit.Neutral,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := TaskStatus(tc.item)
			if got.LabelKey != tc.wantKey || got.Tone != tc.wantTone || got.Outline != tc.wantOutline {
				t.Fatalf("TaskStatus() = %+v, want key=%s tone=%v outline=%v", got, tc.wantKey, tc.wantTone, tc.wantOutline)
			}
		})
	}
}

func TestUpcomingPreview(t *testing.T) {
	now := time.Date(2026, 4, 6, 8, 0, 0, 0, time.UTC).UnixMilli() // Monday

	t.Run("disabled returns none", func(t *testing.T) {
		item := TaskItem{
			Task: protocol.ScheduledTask{
				Enabled: false,
				Timing:  protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: now + HourMs}},
			},
		}
		if got := Upcoming(item, now, 5); len(got) != 0 {
			t.Fatalf("disabled task returned %v", got)
		}
	})

	t.Run("once future vs past", func(t *testing.T) {
		future := TaskItem{
			Task: protocol.ScheduledTask{
				Enabled: true,
				Timing:  protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: now + HourMs}},
			},
		}
		if got := Upcoming(future, now, 5); !slices.Equal(got, []int64{now + HourMs}) {
			t.Fatalf("once future = %v", got)
		}
		past := TaskItem{
			Task: protocol.ScheduledTask{
				Enabled: true,
				Timing:  protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: now - MinuteMs}},
			},
		}
		if got := Upcoming(past, now, 5); len(got) != 0 {
			t.Fatalf("once past = %v", got)
		}
	})

	t.Run("every computes 5 upcoming runs", func(t *testing.T) {
		item := TaskItem{
			Task: protocol.ScheduledTask{
				Enabled: true,
				Timing: protocol.DispatchTiming{Kind: "every", Data: protocol.DispatchTimingEveryPayload{
					AnchorMs:   now - 90*MinuteMs,
					IntervalMs: HourMs,
				}},
			},
		}
		got := Upcoming(item, now, 5)
		first := now + 30*MinuteMs
		want := []int64{first, first + HourMs, first + 2*HourMs, first + 3*HourMs, first + 4*HourMs}
		if !slices.Equal(got, want) {
			t.Fatalf("every upcoming = %v, want %v", got, want)
		}
	})

	t.Run("weekly Mon/Wed/Fri computes 5 upcoming runs", func(t *testing.T) {
		next := time.Date(2026, 4, 6, 9, 30, 0, 0, time.UTC).UnixMilli() // Mon 09:30 UTC
		item := TaskItem{
			NextMs: ptrInt64(next),
			Task: protocol.ScheduledTask{
				Enabled: true,
				Timing: protocol.DispatchTiming{Kind: "weekly", Data: protocol.DispatchTimingWeeklyPayload{
					Timezone: "UTC",
					Weekdays: protocol.Bytes{1, 3, 5},
					Hour:     9,
					Minute:   30,
				}},
			},
		}
		got := Upcoming(item, now, 5)
		want := []int64{
			time.Date(2026, 4, 6, 9, 30, 0, 0, time.UTC).UnixMilli(),  // Mon
			time.Date(2026, 4, 8, 9, 30, 0, 0, time.UTC).UnixMilli(),  // Wed
			time.Date(2026, 4, 10, 9, 30, 0, 0, time.UTC).UnixMilli(), // Fri
			time.Date(2026, 4, 13, 9, 30, 0, 0, time.UTC).UnixMilli(), // Mon
			time.Date(2026, 4, 15, 9, 30, 0, 0, time.UTC).UnixMilli(), // Wed
		}
		if !slices.Equal(got, want) {
			t.Fatalf("weekly upcoming = %v, want %v", got, want)
		}
	})

	t.Run("weekly without next_ms returns empty", func(t *testing.T) {
		item := TaskItem{
			Task: protocol.ScheduledTask{
				Enabled: true,
				Timing: protocol.DispatchTiming{Kind: "weekly", Data: protocol.DispatchTimingWeeklyPayload{
					Timezone: "UTC",
					Weekdays: protocol.Bytes{1, 2, 3, 4, 5},
					Hour:     9,
					Minute:   0,
				}},
			},
		}
		if got := Upcoming(item, now, 5); len(got) != 0 {
			t.Fatalf("weekly without next = %v", got)
		}
	})
}

func TestFailureStreakAndElapsedMinutes(t *testing.T) {
	runs := []protocol.ScheduledTaskHistoryEntry{
		{Job: protocol.DispatchJob{Status: protocol.DispatchStatusFailed}},
		{Job: protocol.DispatchJob{Status: protocol.DispatchStatusFailed}},
		{Job: protocol.DispatchJob{Status: protocol.DispatchStatusFailed}},
		{Job: protocol.DispatchJob{Status: protocol.DispatchStatusCompleted}},
		{Job: protocol.DispatchJob{Status: protocol.DispatchStatusFailed}},
	}
	if got := FailureStreak(runs); got != 3 {
		t.Fatalf("FailureStreak() = %d, want 3", got)
	}
	counts := TallyRuns(runs)
	if counts.Failed != 4 || counts.Completed != 1 {
		t.Fatalf("TallyRuns() = %+v", counts)
	}

	now := int64(1_700_000_600_000)
	started := now - 7*MinuteMs - 35_000
	mins, ok := ElapsedMinutes(TaskItem{Status: protocol.DispatchStatusRunning, StartedMs: &started}, now)
	if !ok || mins != 7 {
		t.Fatalf("ElapsedMinutes() = (%d, %v), want (7, true)", mins, ok)
	}
	if _, ok := ElapsedMinutes(TaskItem{Status: protocol.DispatchStatusQueued, StartedMs: &started}, now); ok {
		t.Fatal("queued task reported elapsed running minutes")
	}
}

func TestScheduleSummariesAndRibbon(t *testing.T) {
	tr := testTranslator(t, "en")
	now := time.Date(2026, 4, 6, 12, 0, 0, 0, time.UTC).UnixMilli()

	every := protocol.DispatchTiming{Kind: "every", Data: protocol.DispatchTimingEveryPayload{AnchorMs: now, IntervalMs: 2 * HourMs}}
	if got := FormatScheduleSummary(every, now, time.UTC, tr); got != "Every 2 h" {
		t.Fatalf("every summary = %q", got)
	}

	weeklyWorkdays := protocol.DispatchTiming{Kind: "weekly", Data: protocol.DispatchTimingWeeklyPayload{
		Timezone: "Asia/Shanghai", Weekdays: protocol.Bytes{1, 2, 3, 4, 5}, Hour: 9, Minute: 5,
	}}
	if got := FormatScheduleSummary(weeklyWorkdays, now, time.UTC, tr); got != "Weekdays 09:05 · Asia/Shanghai" {
		t.Fatalf("weekly workdays summary = %q", got)
	}

	weeklyDaily := protocol.DispatchTiming{Kind: "weekly", Data: protocol.DispatchTimingWeeklyPayload{
		Timezone: "UTC", Weekdays: protocol.Bytes{1, 2, 3, 4, 5, 6, 7}, Hour: 18, Minute: 0,
	}}
	if got := FormatScheduleSummary(weeklyDaily, now, time.UTC, tr); got != "Daily 18:00 · UTC" {
		t.Fatalf("weekly daily summary = %q", got)
	}

	once := protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: now + 2*HourMs}}
	if got := FormatScheduleSummary(once, now, time.UTC, tr); got != "Once Today 14:00" {
		t.Fatalf("once summary = %q", got)
	}

	rib := ComputeRibbon(protocol.DispatchTimingEveryPayload{AnchorMs: now - 12*HourMs, IntervalMs: 6 * HourMs}, now, time.UTC, 96)
	if rib.Overflow || len(rib.Ticks) != 4 || rib.Now != 0.5 {
		t.Fatalf("ComputeRibbon() = %+v", rib)
	}

	overflow := ComputeRibbon(protocol.DispatchTimingEveryPayload{AnchorMs: now - 12*HourMs, IntervalMs: 5 * MinuteMs}, now, time.UTC, 96)
	if !overflow.Overflow || len(overflow.Ticks) != 0 {
		t.Fatalf("overflow ribbon = %+v", overflow)
	}
}

func TestParseWallClockAndLocales(t *testing.T) {
	if h, m, ok := ParseWallClock("09:30"); !ok || h != 9 || m != 30 {
		t.Fatalf("ParseWallClock(09:30) = (%d, %d, %v)", h, m, ok)
	}
	if _, _, ok := ParseWallClock("24:00"); ok {
		t.Fatal("ParseWallClock accepted 24:00")
	}
	zh := testTranslator(t, "zh-CN")
	if got := zh("scheduled_tasks_title"); got != "定时任务" {
		t.Fatalf("zh title = %q", got)
	}
}
