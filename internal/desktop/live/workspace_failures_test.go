package live

import (
	"log/slog"
	"path/filepath"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceSaveFailureRecords(t *testing.T) {
	stop, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	defer stop()
	const ref = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	for _, test := range []struct {
		name, operation, stage, code, reason string
		writable                             bool
		ref                                  string
		level                                slog.Level
	}{
		{"read-only draft", "save_all_documents", "preflight", "unspecified", "draft_not_saveable", false, "", slog.LevelWarn},
		{"unclassified permission failure", "write_file", "workspace_mutation", "permission_denied", "", true, "", slog.LevelError},
		{"Node-classified write", "write_file", "workspace_mutation", "permission_denied", "", true, ref, slog.LevelWarn},
	} {
		t.Run(test.name, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			project := closeTestDraft(fixture)
			project.panel.SetFileWritable("a.go", test.writable)
			fixture.source.dispatch = func(protocol.Request) (link.Admission, error) {
				return link.Admission{}, &link.FaultError{Fault: protocol.Fault{
					Code: protocol.ErrorCodePermissionDenied, Message: "private path in source", ReportRef: test.ref, ReportOwned: test.ref != "",
				}}
			}
			var records []logging.Fields
			logging.SetReport(func(level slog.Level, fields logging.Fields) {
				if fields["area"] == "desktop" {
					if level != test.level {
						t.Errorf("level = %v", level)
					}
					records = append(records, fields)
				}
			})
			defer logging.SetReport(nil)
			window := closeTestWindow(fixture.root)
			fixture.root.SaveDocuments()
			window.Frame()
			fixture.root.workers.Wait()
			window.Frame()
			window.Frame()
			if !window.HasText(i18n.In("en", "workspace_action_failed")) || len(fixture.root.UnsavedDocuments()) != 1 {
				t.Fatalf("failed save lost its toast or draft: %q", window.Texts())
			}
			if len(records) != 1 || records[0]["operation"] != test.operation || records[0]["stage"] != test.stage || records[0]["code"] != test.code {
				t.Fatalf("failure records = %v", records)
			}
			remote := logging.Remote(records[0])
			reason, _ := remote["reason_code"].(string)
			gotRef, _ := remote["report_ref"].(string)
			if reason != test.reason || gotRef != test.ref || remote["detail"] != nil {
				t.Errorf("remote fields = %v", remote)
			}
		})
	}
}

func TestGitInspectionFailureToastRecordsDiagnostic(t *testing.T) {
	stop, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	defer stop()
	// Start lumberjack's rotation worker outside the synctest bubble.
	logging.Emit(slog.LevelDebug)

	const ref = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	for _, test := range []struct {
		name  string
		owned bool
		level slog.Level
	}{
		{"legacy ref leaves reporting to Desktop", false, slog.LevelError},
		{"Node-owned ref keeps a local warning", true, slog.LevelWarn},
	} {
		t.Run(test.name, func(t *testing.T) {
			fault := &protocol.Fault{
				Code: protocol.ErrorCodeUnavailable, Message: "Git repository operation failed: /private/worktree",
				ReportRef: ref, ReportOwned: test.owned, GitInspectionReason: protocol.GitInspectionStatusCommandFailed,
			}
			var records []logging.Fields
			logging.SetReport(func(level slog.Level, fields logging.Fields) {
				if fields["area"] == "desktop" && fields["operation"] == "inspect_git" {
					if level != test.level {
						t.Errorf("Git inspection record level = %v", level)
					}
					records = append(records, fields)
				}
			})
			defer logging.SetReport(nil)
			synctest.Test(t, func(t *testing.T) {
				testDraftProjectSelectionReadFailure(t, nil, fault, false, []string{"git_inspection_failed"})
			})
			if len(records) == 0 {
				t.Fatal("the Git inspection toast had no failure record")
			}
			for _, fields := range records {
				if fields["detail"] != fault.Message || fields["report_ref"] != ref {
					t.Fatalf("local Git inspection fields = %v", fields)
				}
				remote := logging.Remote(fields)
				if remote["stage"] != "run_status" || remote["reason"] != "status_command_failed" || remote["report_ref"] != ref || remote["detail"] != nil {
					t.Fatalf("remote Git inspection fields = %v", remote)
				}
			}
		})
	}
}
