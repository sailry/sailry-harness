package telemetry

import (
	"bytes"
	"encoding/json"
	"errors"
	"log/slog"
	"maps"
	"net/http"
	"os"
	"path/filepath"
	"testing"

	"github.com/getsentry/sentry-go"
	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func modelRequestFields(outcome, reason string, attempts, status uint64) logging.Fields {
	fields := logging.Fields{
		"area": "agent", "operation": "model_request", "phase": logging.Failed, "code": "unavailable",
		"retry_outcome": outcome, "reason_code": reason, "attempts": attempts,
	}
	if outcome == "recovered" {
		fields["phase"] = logging.Completed
	}
	if status != 0 {
		fields["http_status"] = status
	}
	return fields
}

func checkModelRequestFields(t *testing.T, got, want logging.Fields) {
	t.Helper()
	if len(got) != len(want) {
		t.Errorf("model request fields = %v, want only %v", got, want)
	}
	for key, value := range want {
		if n, ok := value.(uint64); ok {
			value = float64(n) // JSON numbers in an untyped Sentry context.
		}
		if got[key] != value {
			t.Errorf("model request %s = %v, want %v", key, got[key], value)
		}
	}
}

func checkModelRequestReport(t *testing.T, body []byte, level sentry.Level, fields logging.Fields) sentry.Event {
	t.Helper()
	assertClean(t, "model request report", body)
	event := productReportEvent(t, body)
	if event.Message != "agent.model_request:unavailable" || event.Level != level {
		t.Errorf("model request report message/level = %q/%s, want agent.model_request:unavailable/%s", event.Message, event.Level, level)
	}
	if event.Tags["sailry.reason_code"] != fields["reason_code"] {
		t.Errorf("model request reason tag = %q, want %q", event.Tags["sailry.reason_code"], fields["reason_code"])
	}
	checkModelRequestFields(t, logging.Fields(event.Contexts["sailry"]), fields)
	if event.EventID == "" || len(event.Fingerprint) != 1 || event.Fingerprint[0] == "" || len(event.Exception) != 1 {
		t.Fatalf("missing reporting identity or exception: id=%q fingerprint=%v exceptions=%d", event.EventID, event.Fingerprint, len(event.Exception))
	}
	if m := event.Exception[0].Mechanism; m == nil || m.Handled == nil || !*m.Handled {
		t.Error("model request exception is not marked handled")
	}
	return event
}

func TestModelRequestReportsAndRetryQueue(t *testing.T) {
	for _, test := range []struct {
		name   string
		log    slog.Level
		level  sentry.Level
		fields logging.Fields
	}{
		{"recovered", slog.LevelWarn, sentry.LevelWarning, modelRequestFields("recovered", "model_rate_limited", 2, http.StatusTooManyRequests)},
		{"failed after retries", slog.LevelError, sentry.LevelError, modelRequestFields("failed", "model_unavailable", 4, http.StatusServiceUnavailable)},
		{"failed on first attempt", slog.LevelError, sentry.LevelError, modelRequestFields("failed", "model_authentication", 1, http.StatusUnauthorized)},
	} {
		t.Run(test.name, func(t *testing.T) {
			dir := t.TempDir()
			settings := protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: true}
			c, endpoint := operationControl(t, dir, retryDSN, false, settings, http.StatusServiceUnavailable)
			c.Record(slog.LevelWarn, logging.Fields{"area": "agent", "operation": "breadcrumb_only", "detail": plantedSecret})
			fields := maps.Clone(test.fields)
			maps.Copy(fields, logging.Fields{
				"provider": plantedName, "provider_name": plantedName, "model": "planted-private-model",
				"endpoint": "https://planted.invalid" + plantedPath, "credential": plantedSecret,
				"prompt": "planted prompt", "response": "planted response", "detail": plantedPath,
				"request": "planted request ID", "session": "planted session ID", "turn": "planted turn ID",
			})
			c.Record(test.log, fields)
			first := checkModelRequestReport(t, receiveOperation(t, endpoint.calls), test.level, test.fields)
			if len(first.Breadcrumbs) != 1 || first.Breadcrumbs[0].Data["operation"] != "breadcrumb_only" {
				t.Fatal("initial report did not carry the test breadcrumb")
			}
			receiveOperation(t, endpoint.timers)
			name := filepath.Join(dir, operationName(0))
			body, err := os.ReadFile(name)
			if err != nil {
				t.Fatal(err)
			}
			var item pendingOperation
			if err := json.Unmarshal(body, &item); err != nil {
				t.Fatal(err)
			}
			checkModelRequestFields(t, item.Fields, test.fields)
			assertClean(t, "queued model request report", body)
			for _, denied := range []string{"stacktrace", "breadcrumbs", "breadcrumb_only", "attachments", retryDSN, settings.InstallationID.String()} {
				if bytes.Contains(body, []byte(denied)) {
					t.Errorf("queued model request report carries %q", denied)
				}
			}
			if item.Event != first.EventID || item.Fingerprint != first.Fingerprint[0] || !item.At.Equal(first.Timestamp) {
				t.Fatal("queue did not preserve the initial reporting identity")
			}

			c.Close()
			// Restore the bytes an interrupted process could leave behind.
			if err := os.WriteFile(name, body, 0o600); err != nil {
				t.Fatal(err)
			}
			_, restarted := operationControl(t, dir, retryDSN, false, settings, http.StatusOK)
			receiveOperation(t, restarted.timers)()
			replay := checkModelRequestReport(t, receiveOperation(t, restarted.calls), test.level, test.fields)
			if replay.EventID != first.EventID || replay.Fingerprint[0] != first.Fingerprint[0] || !replay.Timestamp.Equal(first.Timestamp) {
				t.Fatal("queue replay changed the reporting identity")
			}
			if len(replay.Breadcrumbs) != 0 || replay.Exception[0].Stacktrace != nil {
				t.Error("queue replay carried breadcrumbs or a stacktrace")
			}
			if _, err := os.Lstat(name); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("successful replay did not retire the queue item: %v", err)
			}
		})
	}
}

func TestModelRequestReportConsent(t *testing.T) {
	for _, test := range []struct {
		name                   string
		errors, metrics        bool
		envKey, envValue       string
		wantUsage, wantReports int
	}{
		{name: "error consent only", errors: true, wantReports: 2},
		{name: "usage consent only", metrics: true, wantUsage: 2},
		{name: "both on", errors: true, metrics: true, wantUsage: 2, wantReports: 2},
		{name: "both off"},
		{name: "do not track", errors: true, metrics: true, envKey: "DO_NOT_TRACK", envValue: "1"},
		{name: "telemetry off", errors: true, metrics: true, envKey: "SAILRY_TELEMETRY", envValue: "off"},
		{name: "no Sentry endpoint", errors: true, metrics: true, envKey: "SAILRY_SENTRY_DSN", envValue: " ", wantUsage: 2},
	} {
		t.Run(test.name, func(t *testing.T) {
			env := map[string]string{"SAILRY_SENTRY_DSN": retryDSN, "SAILRY_TELEMETRY_KEY": "test-key"}
			if test.envKey != "" {
				env[test.envKey] = test.envValue
			}
			remote := &endpoints{}
			c := New(Options{Transport: remote, Timer: remote.timer, Lookup: func(key string) string { return env[key] }})
			t.Cleanup(c.Close)
			c.Configure(protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: test.errors, Metrics: test.metrics})
			c.Observe(ModelRequest(Model{API: protocol.ConversationModelApiResponses, Authentication: protocol.AuthenticationApiKey, Model: "custom"}, "error", nil, nil, nil, Tokens{}))
			c.Record(slog.LevelWarn, modelRequestFields("recovered", "model_network", 2, 0))
			c.Record(slog.LevelError, modelRequestFields("failed", "model_network", 3, 0))
			c.reports.Wait()
			c.Close()
			usage, reports := remote.sent()
			if len(usage) != test.wantUsage || len(reports) != test.wantReports {
				t.Errorf("sent %d usage batches and %d reports, want %d and %d", len(usage), len(reports), test.wantUsage, test.wantReports)
			}
		})
	}
}

func TestModelRequestReportMalformedFields(t *testing.T) {
	c, endpoint := control(true, false)
	defer c.Close()
	for _, overrides := range []logging.Fields{
		{"retry_outcome": nil, "reason_code": nil, "attempts": nil},
		{"reason_code": "planted_private_reason"},
		{"attempts": uint64(1)},
		{"attempts": uint64(1_000_001)},
		{"phase": logging.Failed},
		{"operation": "unrelated_operation"},
	} {
		fields := modelRequestFields("recovered", "model_network", 2, http.StatusBadGateway)
		maps.Copy(fields, overrides)
		c.Record(slog.LevelWarn, fields)
	}
	want := modelRequestFields("failed", "model_network", 3, 0)
	fields := maps.Clone(want)
	fields["http_status"] = uint64(600)
	c.Record(slog.LevelError, fields)
	c.reports.Wait()
	_, reports := endpoint.sent()
	if len(reports) != 1 {
		t.Fatalf("unclassified recovery warnings must not report; got %d reports, want only the failure", len(reports))
	}
	event := checkModelRequestReport(t, reports[0], sentry.LevelError, want)
	for _, breadcrumb := range event.Breadcrumbs {
		for _, key := range []string{"retry_outcome", "reason_code", "attempts", "http_status"} {
			if _, exists := breadcrumb.Data[key]; exists {
				t.Errorf("unclassified recovery breadcrumb carried %q", key)
			}
		}
	}
}

func TestModelRequestReportFingerprint(t *testing.T) {
	c, _ := control(true, false)
	defer c.Close()
	group := func(outcome, reason string, attempts, status uint64, caller string) string {
		fields := modelRequestFields(outcome, reason, attempts, status)
		fields["request"], fields["model"] = caller+"-request", caller+"-model"
		safe := logging.Remote(fields)
		trace := &sentry.Stacktrace{Frames: []sentry.Frame{{Module: module, Function: caller, Filename: "model.go", Lineno: int(attempts)}}}
		return c.eventLocked(operationKind(safe), safe, trace).Fingerprint[0]
	}
	first := group("recovered", "model_network", 2, http.StatusBadGateway, "callA")
	if got := group("recovered", "model_network", 3, http.StatusServiceUnavailable, "callB"); got != first {
		t.Error("attempt count, status, request identity or call site split a model request group")
	}
	if got := group("failed", "model_network", 2, http.StatusBadGateway, "callA"); got == first {
		t.Error("failed and recovered model requests shared a fingerprint")
	}
	if got := group("recovered", "model_unavailable", 2, http.StatusBadGateway, "callA"); got == first {
		t.Error("distinct model request reasons shared a fingerprint")
	}
}
