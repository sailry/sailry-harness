package kit

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A failed toast does not make a cancellation, setup state, or conflict an
// error report. An unclassified local failure is still an error.
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
		{"sign-in required", Failed, protocol.NewConfigurationFault(protocol.ConfigurationLoginRequired, "private account"), slog.LevelWarn, "not_configured"},
		{"lost credential", Failed, protocol.NewConfigurationFault(protocol.ConfigurationCredentialMissing, "private account"), slog.LevelError, "not_configured"},
		{"network", Failed, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Details: &protocol.FaultDetails{Kind: "network"}}, slog.LevelWarn, "unavailable"},
		{"cancelled", Failed, &protocol.Fault{Code: protocol.ErrorCodeCancelled}, slog.LevelWarn, "cancelled"},
		{"local cancellation", Failed, fmt.Errorf("reading: %w", context.Canceled), slog.LevelWarn, "cancelled"},
		{"conflict", Conflict, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}, slog.LevelWarn, "revision_conflict"},
		{"conflict shown as failed", Failed, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}, slog.LevelWarn, "revision_conflict"},
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
	var severity slog.Level
	var fields logging.Fields
	logging.SetReport(func(level slog.Level, values logging.Fields) { severity, fields = level, values })
	defer logging.SetReport(nil)
	const ref = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	fault := protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "private path", ReportRef: ref}
	owned := fault
	owned.ReportOwned = true
	delivered := func(kind string, fault protocol.Fault, completion bool) error {
		var node protocol.NodeID
		controller := client.New(link.NewLocal(node, node, diagnosticHandler{fault: fault, completion: completion}))
		t.Cleanup(func() { _ = controller.Close() })
		_, err := controller.Do(t.Context(), protocol.Command{Kind: kind})
		return err
	}
	for _, test := range []struct {
		name      string
		area      string
		operation string
		err       error
		level     slog.Level
		want      any
	}{
		{"legacy mutation admission", "desktop", "write_file", delivered("write_file", fault, false), slog.LevelWarn, ref},
		{"client completion", "desktop", "write_file", fmt.Errorf("save: %w", delivered("write_file", fault, true)), slog.LevelWarn, ref},
		{"legacy read", "desktop", "provider_query_failed", delivered("discover_models", fault, true), slog.LevelError, ref},
		{"owned read", "desktop", "provider_query_failed", delivered("discover_models", owned, true), slog.LevelWarn, ref},
		{"legacy peer without reference", "desktop", "write_file", delivered("write_file", protocol.Fault{Code: fault.Code}, true), slog.LevelError, nil},
		{"invalid peer reference", "desktop", "write_file", delivered("write_file", protocol.Fault{Code: fault.Code, ReportRef: "private/path", ReportOwned: true}, true), slog.LevelError, nil},
		{"ownership without reference", "desktop", "write_file", delivered("write_file", protocol.Fault{Code: fault.Code, ReportOwned: true}, true), slog.LevelError, nil},
		{"cancelled", "desktop", "write_file", delivered("write_file", protocol.Fault{Code: protocol.ErrorCodeCancelled, ReportRef: ref, ReportOwned: true}, true), slog.LevelWarn, nil},
		{"locally supplied ownership", "desktop", "inspect_git", &owned, slog.LevelError, nil},
		{"plugin fabricated reference", "plugin", "git-error", &link.FaultError{Fault: owned}, slog.LevelError, nil},
		{"plugin standalone", "plugin", "git-error", protocol.Fault{Code: protocol.ErrorCodeNotConfigured}, slog.LevelError, nil},
	} {
		fields = nil
		RecordFailure(test.area, test.operation, Failed, test.err)
		var detail any
		if message := test.err.Error(); message != "" {
			detail = message
		}
		if fields == nil || severity != test.level || fields["area"] != test.area || fields["operation"] != test.operation || fields["report_ref"] != test.want || fields["detail"] != detail {
			t.Errorf("%s: level %v, local fields %v", test.name, severity, fields)
		}
		if safe := logging.Remote(fields); safe["report_ref"] != test.want || safe["detail"] != nil {
			t.Errorf("%s remote fields = %v", test.name, safe)
		}
	}
}

type diagnosticHandler struct {
	link.Handler
	fault      protocol.Fault
	completion bool
}

func (handler diagnosticHandler) Dispatch(context.Context, protocol.NodeID, protocol.Request) (link.Admission, error) {
	if !handler.completion {
		return link.Admission{}, &handler.fault
	}
	completed := make(chan link.Response, 1)
	completed <- link.Response{Fault: &handler.fault}
	close(completed)
	return link.Admission{Completion: completed}, nil
}

func TestRecordRefusalKeepsLocalDiagnostic(t *testing.T) {
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	var severity slog.Level
	var fields logging.Fields
	logging.SetReport(func(level slog.Level, values logging.Fields) { severity, fields = level, values })
	defer logging.SetReport(nil)
	RecordWarning("desktop", "save_all_documents", nil, slog.String("stage", "preflight"), slog.String("reason_code", "draft_not_saveable"))
	if severity != slog.LevelWarn || fields["phase"] != logging.Failed || fields["stage"] != "preflight" || fields["reason_code"] != "draft_not_saveable" {
		t.Fatalf("read-only draft: level %v, fields %v", severity, fields)
	}
	RecordFailure("desktop", "edit_markdown_source", Failed, nil, slog.String("stage", "map_selection"), slog.String("reason_code", "selection_unmapped"))
	if severity != slog.LevelError || fields["stage"] != "map_selection" || fields["reason_code"] != "selection_unmapped" {
		t.Fatalf("local UI failure: level %v, fields %v", severity, fields)
	}
}

func TestRecordPreferencesWriteFailure(t *testing.T) {
	profile := t.TempDir()
	closeLog, err := logging.Install(filepath.Join(profile, "logs-profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	var fields logging.Fields
	logging.SetReport(func(_ slog.Level, values logging.Fields) { fields = values })
	defer logging.SetReport(nil)
	prefs := preferences.ForStartup(profile)
	if err := os.WriteFile(filepath.Join(profile, "desktop"), []byte("private contents"), 0o600); err != nil {
		t.Fatal(err)
	}
	err = prefs.FinishStartup()
	stage, reason := preferences.WriteFailure(err)
	if err == nil || stage != "create_directory" {
		t.Fatalf("fixture did not fail to write: %v", err)
	}
	RecordFailure("desktop", "preferences_save_failed", Failed, fmt.Errorf("save: %w", err))
	if fields["stage"] != stage || fields["reason"] != reason || fields["code"] != "internal" || strings.Contains(fields["detail"].(string), profile) {
		t.Fatalf("preference write record = %v", fields)
	}
	path, err := logging.CurrentFile()
	if err != nil {
		t.Fatal(err)
	}
	tail, err := logging.Tail(path)
	if err != nil {
		t.Fatal(err)
	}
	var diagnostic logging.Fields
	if err := json.Unmarshal(tail, &diagnostic); err != nil {
		t.Fatal(err)
	}
	if diagnostic["stage"] != stage || diagnostic["reason"] != reason || diagnostic["detail"] != nil || strings.Contains(string(tail), profile) || strings.Contains(string(tail), "private") {
		t.Fatalf("preference write diagnostic = %s", tail)
	}
}
