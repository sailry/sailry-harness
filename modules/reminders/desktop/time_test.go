package remindersdesktop

import (
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

func TestSortRemindersAndOverdue(t *testing.T) {
	now := int64(1_700_000_000_000)
	items := []ReminderItem{
		{Reminder: protocol.Reminder{ID: "untimed-b", Title: "Beta"}},
		{Reminder: protocol.Reminder{ID: "future", Title: "Later", DueMs: ptrInt64(now + HourMs)}},
		{Reminder: protocol.Reminder{ID: "untimed-a", Title: "Alpha"}},
		{Reminder: protocol.Reminder{ID: "overdue", Title: "Urgent", DueMs: ptrInt64(now - MinuteMs)}},
	}

	sorted := SortReminders(items)
	gotIDs := []string{sorted[0].Reminder.ID, sorted[1].Reminder.ID, sorted[2].Reminder.ID, sorted[3].Reminder.ID}
	wantIDs := []string{"overdue", "future", "untimed-a", "untimed-b"}
	for i := range wantIDs {
		if gotIDs[i] != wantIDs[i] {
			t.Fatalf("SortReminders order = %v, want %v", gotIDs, wantIDs)
		}
	}

	if !IsOverdue(sorted[0].Reminder, now) {
		t.Fatal("past open reminder should be overdue")
	}
	if IsOverdue(protocol.Reminder{Completed: true, DueMs: ptrInt64(now - MinuteMs)}, now) {
		t.Fatal("completed reminder must not be overdue")
	}
	if IsOverdue(sorted[1].Reminder, now) || IsOverdue(sorted[2].Reminder, now) {
		t.Fatal("future or untimed reminder must not be overdue")
	}

	if st := ReminderStatus(protocol.Reminder{Completed: true, DueMs: ptrInt64(now - MinuteMs)}, now); st.LabelKey != "reminders_completed" || st.Tone != kit.Positive {
		t.Fatalf("completed status = %+v", st)
	}
	if st := ReminderStatus(sorted[0].Reminder, now); st.LabelKey != "reminders_overdue" || st.Tone != kit.Negative {
		t.Fatalf("overdue status = %+v", st)
	}
	if st := ReminderStatus(sorted[1].Reminder, now); st.LabelKey != "reminders_pending" || st.Tone != kit.Caution {
		t.Fatalf("pending status = %+v", st)
	}
}

func TestValidateDueMsAndTimeFormatting(t *testing.T) {
	now := time.Date(2026, 4, 6, 10, 0, 0, 0, time.UTC).UnixMilli()
	past := now - HourMs
	if !ValidateDueMs(past, &past, now) {
		t.Fatal("unchanged past due_ms must be allowed on edit")
	}
	if ValidateDueMs(past+MinuteMs, &past, now) {
		t.Fatal("changed past due_ms must be rejected")
	}
	if ValidateDueMs(past, nil, now) {
		t.Fatal("new past due_ms must be rejected")
	}
	if !ValidateDueMs(now+MinuteMs, nil, now) {
		t.Fatal("future due_ms must be accepted")
	}

	en := testTranslator(t, "en")
	if got := FormatRelative(now+90*MinuteMs, now, en); got != "in 1 h 30 min" {
		t.Fatalf("FormatRelative future = %q", got)
	}
	if got := FormatRelative(now-2*DayMs, now, en); got != "2 days ago" {
		t.Fatalf("FormatRelative past = %q", got)
	}
	if got := FormatWhen(now+HourMs, now, time.UTC, en); got != "Today 11:00" {
		t.Fatalf("FormatWhen = %q", got)
	}
	zh := testTranslator(t, "zh-CN")
	if got := zh("reminders_page_title"); got != "待办提醒" {
		t.Fatalf("zh title = %q", got)
	}
}
