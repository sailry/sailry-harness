package live

import (
	"log/slog"
	"path/filepath"
	"testing"

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
	for _, test := range []struct {
		name, operation, stage, code, reason string
		writable                             bool
	}{
		{"read-only draft", "save_all_documents", "preflight", "unspecified", "draft_not_saveable", false},
		{"refused write", "write_file", "workspace_mutation", "permission_denied", "", true},
	} {
		t.Run(test.name, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			project := closeTestDraft(fixture)
			project.panel.SetFileWritable("a.go", test.writable)
			fixture.source.dispatch = func(protocol.Request) (link.Admission, error) {
				return link.Admission{}, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "private path in source"}}
			}
			var records []logging.Fields
			logging.SetReport(func(level slog.Level, fields logging.Fields) {
				if fields["area"] == "desktop" {
					if level != slog.LevelError {
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
			if reason, _ := remote["reason_code"].(string); reason != test.reason || remote["detail"] != nil {
				t.Errorf("remote fields = %v", remote)
			}
		})
	}
}
