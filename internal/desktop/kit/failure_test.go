package kit

import (
	"errors"
	"fmt"
	"log/slog"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A failure the user was shown reaches error reporting with its operation
// and code; its message stays local and only failed outcomes are errors.
func TestRecordFailure(t *testing.T) {
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	var severity slog.Level
	var fields logging.Fields
	logging.SetReport(func(level slog.Level, values logging.Fields) { severity, fields = level, values })
	defer logging.SetReport(nil)
	for _, test := range []struct {
		name    string
		outcome Outcome
		err     error
		level   slog.Level
		code    string
	}{
		{"fault", Failed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "/private/path"}, slog.LevelError, "unavailable"},
		{"wrapped", Failed, fmt.Errorf("reading: %w", protocol.Fault{Code: protocol.ErrorCodeNotFound}), slog.LevelError, "not_found"},
		{"other error", Failed, errors.New("disk failed"), slog.LevelError, "internal"},
		{"no error", Failed, nil, slog.LevelError, "unspecified"},
		{"cancelled", Failed, &protocol.Fault{Code: protocol.ErrorCodeCancelled}, slog.LevelWarn, "cancelled"},
		{"conflict", Conflict, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}, slog.LevelWarn, "revision_conflict"},
		{"uncertain", Uncertain, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown}, slog.LevelWarn, "outcome_unknown"},
	} {
		fields = nil
		RecordFailure("desktop", "workspace_action_failed", test.outcome, test.err)
		if fields == nil || severity != test.level || fields["phase"] != logging.Failed || fields["code"] != test.code || fields["operation"] != "workspace_action_failed" {
			t.Fatalf("%s: level %v, fields %v", test.name, severity, fields)
		}
		if test.err != nil && fields["detail"] != test.err.Error() {
			t.Fatalf("%s: the local record lacks the failure's message: %v", test.name, fields)
		}
		remote := logging.Remote(fields)
		if _, leaked := remote["detail"]; leaked || remote["operation"] != "workspace_action_failed" || remote["code"] != test.code {
			t.Fatalf("%s: remote fields %v", test.name, remote)
		}
	}
}
