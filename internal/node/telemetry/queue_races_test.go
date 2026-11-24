package telemetry

import (
	"bytes"
	"log/slog"
	"testing"
	"time"

	"github.com/getsentry/sentry-go"
	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestGuideRaceReporting(t *testing.T) {
	c, endpoint := control(true, false)
	defer c.Close()
	fields := logging.Fields{"area": "command", "operation": "guide_queued_turn", "phase": "failed", "code": "conflict", "reason_code": "recipient_stopping", "detail": plantedPath, "request": plantedSecret}
	for range queueBurstCount - 1 {
		c.Record(slog.LevelWarn, fields)
	}
	c.reports.Wait()
	if _, events := endpoint.sent(); len(events) != 0 {
		t.Fatal("an isolated race was reported")
	}
	id := uuid.New()
	c.Configure(protocol.TelemetrySettings{InstallationID: id})
	c.Record(slog.LevelWarn, fields)
	c.Configure(protocol.TelemetrySettings{Errors: true, InstallationID: id})
	for range queueBurstCount - 1 {
		c.Record(slog.LevelWarn, fields)
	}
	c.reports.Wait()
	if _, events := endpoint.sent(); len(events) != 0 {
		t.Fatal("a race window survived consent withdrawal")
	}
	for range 4 {
		c.Record(slog.LevelWarn, fields)
	}
	c.reports.Wait()
	_, events := endpoint.sent()
	if len(events) != 1 || !bytes.Contains(events[0], []byte(`"sailry.reason_code":"recipient_stopping"`)) || !bytes.Contains(events[0], []byte(`"count":10`)) {
		t.Fatalf("expected one indexed queue burst, got %d reports", len(events))
	}
	assertClean(t, "queue burst", events[0])

	at := time.Now().Add(5 * time.Minute)
	for n := 1; n <= queueBurstCount; n++ {
		if c.queueBurstLocked(fields, at) != (n == queueBurstCount) {
			t.Fatalf("new window: unexpected threshold at %d", n)
		}
	}
	fields["reason_code"] = "recipient_not_task"
	for range queueBurstCount {
		if c.queueBurstLocked(fields, at) {
			t.Fatal("invalid recipient was counted as a race")
		}
	}
}

func TestReasonFingerprint(t *testing.T) {
	c, _ := control(true, false)
	defer c.Close()
	fields := logging.Fields{"area": "desktop", "operation": "save_all_documents", "phase": "failed", "code": "unspecified", "reason_code": "draft_not_saveable"}
	group := func(line int) string {
		return c.eventLocked("operation", logging.Remote(fields), &sentry.Stacktrace{Frames: []sentry.Frame{{Lineno: line}}}).Fingerprint[0]
	}
	original := group(1)
	fields["report_ref"], fields["count"] = uuid.NewString(), uint64(3)
	if group(2) != original {
		t.Fatal("source line, count or delivery ref split a classified failure")
	}
	fields["reason_code"] = "selection_unmapped"
	if group(2) == original {
		t.Fatal("distinct reasons grouped together")
	}
}

// A failure without a reason groups by its stack's functions: a release that
// moves its lines keeps the group, and another call path opens its own.
func TestStackFingerprintIgnoresLines(t *testing.T) {
	c, _ := control(true, false)
	defer c.Close()
	fields := logging.Remote(logging.Fields{"area": "desktop", "operation": "inspect_git", "phase": "failed", "code": "busy"})
	group := func(function string, line int) string {
		return c.eventLocked("operation", fields, &sentry.Stacktrace{Frames: []sentry.Frame{{Module: "live", Function: function, Filename: "workspace_state.go", Lineno: line}}}).Fingerprint[0]
	}
	if group("reportRead", 401) != group("reportRead", 412) {
		t.Fatal("a moved line split an unchanged failure")
	}
	if group("reportRead", 401) == group("reportWrite", 401) {
		t.Fatal("distinct call paths grouped together")
	}
}
