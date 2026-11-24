package kit

import (
	"errors"
	"fmt"
	"log/slog"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
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
		{"setup", Failed, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "Git executable not found on execution Node"}, slog.LevelWarn, "not_configured"},
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

func TestRecordFailureReportRef(t *testing.T) {
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	var fields logging.Fields
	logging.SetReport(func(_ slog.Level, values logging.Fields) { fields = values })
	defer logging.SetReport(nil)
	const ref = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	fault := protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "private path", ReportRef: ref}
	for _, test := range []struct {
		name string
		err  error
		want any
	}{
		{"admission", &fault, ref},
		{"client completion", fmt.Errorf("save: %w", &link.FaultError{Fault: fault}), ref},
		{"legacy peer", &link.FaultError{Fault: protocol.Fault{Code: fault.Code}}, nil},
		{"invalid peer reference", &link.FaultError{Fault: protocol.Fault{Code: fault.Code, ReportRef: "private/path"}}, nil},
		{"cancelled", &protocol.Fault{Code: protocol.ErrorCodeCancelled, ReportRef: ref}, nil},
	} {
		fields = nil
		RecordFailure("desktop", "write_file", Failed, test.err)
		if fields == nil || fields["report_ref"] != test.want {
			t.Errorf("%s local fields = %v", test.name, fields)
		}
		if safe := logging.Remote(fields); safe["report_ref"] != test.want || safe["detail"] != nil {
			t.Errorf("%s remote fields = %v", test.name, safe)
		}
	}
}
