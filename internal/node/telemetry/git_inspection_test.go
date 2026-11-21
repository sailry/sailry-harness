package telemetry

import (
	"bytes"
	"encoding/json"
	"log/slog"
	"testing"

	"github.com/getsentry/sentry-go"
	"github.com/sailry/sailry-harness/internal/node/logging"
)

func TestGitInspectionOperationReport(t *testing.T) {
	t.Parallel()
	c, remote := control(true, false)
	defer c.Close()
	const ref = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	c.Record(slog.LevelError, logging.Fields{
		"area": "desktop", "operation": "inspect_git", "phase": logging.Failed, "code": "unavailable", "report_ref": ref,
		"stage": "parse_status", "reason": "unsupported_path_encoding", "detail": plantedPath, "stderr": plantedSecret,
	})
	c.reports.Wait()
	_, reports := remote.sent()
	if len(reports) != 1 {
		t.Fatalf("reports = %d, want 1", len(reports))
	}
	assertClean(t, "Git inspection report", reports[0])
	lines := bytes.SplitN(reports[0], []byte("\n"), 4)
	var event sentry.Event
	if len(lines) < 3 || json.Unmarshal(lines[2], &event) != nil {
		t.Fatal("invalid Sentry envelope")
	}
	fields := event.Contexts["sailry"]
	if event.Message != "desktop.inspect_git:unavailable" || fields["stage"] != "parse_status" ||
		fields["reason"] != "unsupported_path_encoding" || fields["report_ref"] != ref {
		t.Fatalf("Git inspection report = %q, %v", event.Message, fields)
	}
}
