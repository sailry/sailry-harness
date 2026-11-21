package node

import (
	"context"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchReportRef(t *testing.T) {
	t.Setenv("SAILRY_TELEMETRY", "off")
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(root, "profile")
	closeLog, err := logging.Install(profile, "host")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(closeLog)
	node, err := startFixtureNode(t.Context(), profile)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = node.Stop(context.Background()) })
	controller := client.New(link.NewLocal(node.ID(), node.ID(), node))
	t.Cleanup(func() { _ = controller.Close() })
	session := sessionFixture(t, node)
	path, err := logging.CurrentFile()
	if err != nil {
		t.Fatal(err)
	}
	check := func(request protocol.Request, stage string, fault *protocol.Fault, code protocol.ErrorCode) {
		t.Helper()
		if fault.Code != code || !logging.ValidReportRef(fault.ReportRef) || fault.ReportRef == request.ID.String() {
			t.Fatalf("%s %s fault = %+v", request.Command.Kind, stage, *fault)
		}
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		matches := 0
		for line := range strings.SplitSeq(string(data), "\n") {
			fields := strings.Fields(line)
			if !slices.Contains(fields, "report_ref="+fault.ReportRef) {
				continue
			}
			matches++
			for _, field := range []string{"phase=failed", "command=" + request.Command.Kind, "request=" + request.ID.String(), "command_stage=" + stage, "code=" + string(code)} {
				if !slices.Contains(fields, field) {
					t.Fatalf("correlated record lacks %s: %s", field, line)
				}
			}
		}
		if matches != 1 {
			t.Fatalf("%s correlated records = %d, want exactly one", stage, matches)
		}
	}

	search := controller.Prepare(protocol.Command{Kind: "search_files", Data: protocol.CommandSearchFilesPayload{Worktree: session.Worktree, Options: protocol.FileSearch{Mode: protocol.SearchModeContent, Query: "notes"}}})
	search.Version++
	_, err = controller.Execute(t.Context(), search)
	var rejected *protocol.Fault
	if !errors.As(err, &rejected) {
		t.Fatalf("search admission error = %v", err)
	}
	check(search, "admission", rejected, protocol.ErrorCodeWrongTarget)
	abandoned, cancel := context.WithCancel(t.Context())
	cancel()
	_, err = controller.Execute(abandoned, search)
	if !errors.As(err, &rejected) || rejected.Code != protocol.ErrorCodeWrongTarget || rejected.ReportRef != "" {
		t.Fatalf("abandoned read changed its refusal or received a reference: %v", err)
	}

	write := protocol.CommandWriteFilePayload{Worktree: session.Worktree, Path: "note.txt", Text: "first"}
	commandFixture(t, node, protocol.Command{Kind: "write_file", Data: write})
	wrong := strings.Repeat("0", 64)
	write.ExpectedRevision, write.Text = &wrong, "replacement"
	request := controller.Prepare(protocol.Command{Kind: "write_file", Data: write})
	var first protocol.Fault
	for delivery := range 2 {
		_, err := controller.Settle(t.Context(), request)
		var completed *link.FaultError
		if !errors.As(err, &completed) {
			t.Fatalf("expected a completed failure, got %v", err)
		}
		check(request, "completion", &completed.Fault, protocol.ErrorCodeRevisionConflict)
		if delivery == 0 {
			first = completed.Fault
		} else if completed.Fault.ReportRef == first.ReportRef || completed.Fault.Message != first.Message {
			t.Fatalf("replay = %+v, first delivery = %+v", completed.Fault, first)
		}
	}
	stored, err := controller.Resolve(t.Context(), request)
	if err != nil || stored.State != client.RequestFailed || stored.Fault == nil || stored.Fault.ReportRef != "" || stored.Fault.Code != first.Code || stored.Fault.Message != first.Message {
		t.Fatalf("reference changed the durable result: %+v, %v", stored, err)
	}
}

func TestReportFaultScope(t *testing.T) {
	original := protocol.NewConfigurationFault(protocol.ConfigurationCredentialMissing, "private detail")
	for _, kind := range []string{"guide_queued_turn", "create_managed_worktree", "register_worktree", "move_conversation", "list_worktrees", "search_files", "finish_file_upload"} {
		got := reportFault(logging.NewQuery("command"), kind, original, false)
		if got == original || original.ReportRef != "" || !logging.ValidReportRef(got.ReportRef) || got.Code != original.Code || got.Message != original.Message || got.ConfigurationReason() != original.ConfigurationReason() {
			t.Errorf("%s changed or failed to correlate its fault: %+v", kind, *got)
		}
	}
	for _, kind := range []string{"snapshot", "read_git_diff", "fork_conversation_at"} {
		if got := reportFault(logging.NewQuery("command"), kind, original, false); got != original || got.ReportRef != "" {
			t.Errorf("uncorrelated %s = %+v", kind, *got)
		}
	}
	cancelled := &protocol.Fault{Code: protocol.ErrorCodeCancelled}
	if got := reportFault(logging.NewQuery("command"), "write_file", cancelled, false); got != cancelled || got.ReportRef != "" {
		t.Errorf("cancelled command = %+v", *got)
	}
}

func TestPluginCompletionReportRef(t *testing.T) {
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "host")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	var recorded logging.Fields
	logging.SetReport(func(_ slog.Level, values logging.Fields) { recorded = values })
	defer logging.SetReport(nil)
	node := &Node{profile: &Profile{}, logs: true}
	plugin := protocol.DefaultPluginContext()
	plugin.Package.Name = "private.plugin"
	const kind = "read_file"
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: kind, Data: protocol.CommandReadFilePayload{Path: "/private/path"}})
	request.Plugin = &plugin

	for _, test := range []struct {
		stage string
		code  protocol.ErrorCode
		kind  string
		want  bool
	}{
		{stage: "completion", code: protocol.ErrorCodeUnavailable, want: true},
		{stage: "completion", code: protocol.ErrorCodeInternal, want: true},
		{stage: "completion", code: protocol.ErrorCodeOutcomeUnknown, want: true},
		{stage: "completion", code: protocol.ErrorCodeUnavailable, kind: "network"},
		{stage: "completion", code: protocol.ErrorCodeNotFound},
		{stage: "admission", code: protocol.ErrorCodeUnavailable},
	} {
		span := node.commandSpan(node.ID(), request)
		span.Set(slog.String("command_stage", test.stage))
		original := &protocol.Fault{Code: test.code, Message: "private provider address and stderr"}
		if test.kind != "" {
			original.Details = &protocol.FaultDetails{Kind: test.kind}
		}
		fault := reportFault(span, kind, original, test.stage == "completion")
		recorded = nil
		rejectCommand(span, kind, fault, false)
		if (fault != original) != test.want || logging.ValidReportRef(fault.ReportRef) != test.want || original.ReportRef != "" {
			t.Fatalf("%s %s (%s): unexpected reference %q", test.stage, test.code, test.kind, fault.ReportRef)
		}
		var ref any
		if test.want {
			ref = fault.ReportRef
		}
		if recorded["source_kind"] != kind || recorded["command"] != "plugin_command" || recorded["code"] != string(test.code) ||
			recorded["command_stage"] != test.stage || recorded["detail"] != nil || recorded["report_ref"] != ref {
			t.Fatalf("local plugin record = %v", recorded)
		}
		remote := logging.Remote(recorded)
		if remote["operation"] != "plugin_command" || remote["package"] != "third_party" || remote["source_kind"] != nil ||
			remote["request"] != nil || remote["detail"] != nil || remote["report_ref"] != ref {
			t.Fatalf("remote plugin fields = %v", remote)
		}
	}
}
