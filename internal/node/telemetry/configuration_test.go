package telemetry

import (
	"bytes"
	"log/slog"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/logging"
)

func TestConfigurationReports(t *testing.T) {
	t.Parallel()
	c, remote := control(true, false)
	defer c.Close()
	fields := logging.Fields{"area": "command", "operation": "submit_turn", "phase": "failed", "code": "not_configured",
		"configuration_reason": "login_required", "command_stage": "admission", "detail": plantedSecret, "session": plantedName}
	c.Record(slog.LevelWarn, fields)
	c.reports.Wait()
	if _, reports := remote.sent(); len(reports) != 0 {
		t.Fatal("expected setup refusal sent a Sentry error")
	}
	fields["configuration_reason"] = "credential_missing"
	c.Record(slog.LevelError, fields)
	c.reports.Wait()
	_, reports := remote.sent()
	if len(reports) != 1 {
		t.Fatalf("reports = %d, want 1", len(reports))
	}
	for _, want := range []string{"command.submit_turn:not_configured:credential_missing", `"command_stage":"admission"`, `"configuration_reason":"login_required"`} {
		if !bytes.Contains(reports[0], []byte(want)) {
			t.Errorf("report lacks %q", want)
		}
	}
	assertClean(t, "configuration report", reports[0])
	first := c.eventLocked("operation", logging.Remote(fields), nil)
	fields["configuration_reason"] = "provider_capture_missing"
	second := c.eventLocked("operation", logging.Remote(fields), nil)
	if first.Fingerprint[0] == second.Fingerprint[0] {
		t.Fatal("distinct configuration failures grouped together")
	}
	fields["configuration_reason"] = "private_account_name"
	fields["command_stage"] = "private_path"
	unknown := c.eventLocked("operation", logging.Remote(fields), nil)
	if unknown.Message != "command.submit_turn:not_configured" {
		t.Fatalf("unknown reason entered the report title: %q", unknown.Message)
	}
}
