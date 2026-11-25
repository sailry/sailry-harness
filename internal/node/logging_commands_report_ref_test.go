package node

import (
	"context"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
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
	if err != nil || stored.State != client.RequestFailed || stored.Fault == nil || stored.Fault.ReportRef != "" || stored.Fault.ReportOwned || stored.Fault.Code != first.Code || stored.Fault.Message != first.Message {
		t.Fatalf("reference changed the durable result: %+v, %v", stored, err)
	}
}

func TestDispatchShutdownAdmission(t *testing.T) {
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
	owner, err := startFixtureNode(t.Context(), profile)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = owner.Stop(context.Background()) })

	catalog := protocol.Command{Kind: "read_resource_catalog"}
	during := protocol.NewRequest(owner.ID(), catalog)
	after := protocol.NewRequest(owner.ID(), catalog)
	unreachable := protocol.NewRequest(protocol.NodeID{1}, protocol.Command{Kind: "list_knowledge_collections"})
	var reports []logging.Fields
	logging.SetReport(func(_ slog.Level, fields logging.Fields) {
		switch fields["request"] {
		case during.ID.String(), after.ID.String(), unreachable.ID.String():
			reports = append(reports, fields)
		}
	})
	defer logging.SetReport(nil)
	dispatch := func(ctx context.Context, request protocol.Request) *protocol.Fault {
		t.Helper()
		admitted, err := owner.Dispatch(ctx, owner.ID(), request)
		fault, ok := err.(*protocol.Fault)
		if !ok || fault.Code != protocol.ErrorCodeUnavailable || admitted.Completion != nil {
			t.Fatalf("%s: admission = %+v, error = %v", request.Command.Kind, admitted, err)
		}
		return fault
	}
	fault := dispatch(t.Context(), unreachable)
	if !fault.ReportOwned || !logging.ValidReportRef(fault.ReportRef) || len(reports) != 1 ||
		reports[0]["command_stage"] != "admission" || reports[0]["request"] != unreachable.ID.String() || reports[0]["report_ref"] != fault.ReportRef {
		t.Fatalf("running Node lost an unavailable admission report: fault %+v, reports %v", fault, reports)
	}

	// Hold one operation so reporting is still attached when the catalog read
	// reaches the closed gate after authorization.
	if err := owner.begin(); err != nil {
		t.Fatal(err)
	}
	release := sync.OnceFunc(owner.work.Done)
	defer release()
	wait, cancel := context.WithCancel(t.Context())
	cancel()
	var stopErr error
	readCtx := stopOnCommandAuthorization{Context: t.Context(), stop: sync.OnceFunc(func() { stopErr = owner.Stop(wait) })}
	checkClosed := func(ctx context.Context, request protocol.Request) {
		t.Helper()
		if fault := dispatch(ctx, request); fault.Message != unavailable().Message || fault.ReportOwned || fault.ReportRef != "" || len(reports) != 1 {
			t.Fatalf("closed admission produced a fault report: fault %+v, reports %v", fault, reports)
		}
	}
	checkClosed(readCtx, during)
	if !errors.Is(stopErr, context.Canceled) {
		t.Fatalf("stop waiter = %v", stopErr)
	}
	release()
	if err := owner.Stop(t.Context()); err != nil {
		t.Fatal(err)
	}
	checkClosed(t.Context(), after)
}

type stopOnCommandAuthorization struct {
	context.Context
	stop func()
}

func (ctx stopOnCommandAuthorization) Value(key any) any {
	if _, captured := key.(capturedDispatchKey); captured {
		ctx.stop()
	}
	return ctx.Context.Value(key)
}

func TestReportFaultScope(t *testing.T) {
	original := protocol.NewConfigurationFault(protocol.ConfigurationCredentialMissing, "private detail")
	node := &Node{profile: &Profile{}, logs: true}
	for _, kind := range []string{"guide_queued_turn", "create_managed_worktree", "register_worktree", "move_conversation", "list_worktrees", "search_files", "finish_file_upload", "snapshot", "read_git_diff", "fork_conversation_at", "upload_file"} {
		span := node.commandSpan(node.ID(), protocol.Request{Command: protocol.Command{Kind: kind}})
		if span == nil {
			t.Fatalf("%s cannot report its failure", kind)
		}
		got := reportFault(span, original)
		if got == original || original.ReportRef != "" || original.ReportOwned || !got.ReportOwned || !logging.ValidReportRef(got.ReportRef) || got.Code != original.Code || got.Message != original.Message || got.ConfigurationReason() != original.ConfigurationReason() {
			t.Errorf("%s changed or failed to correlate its fault: %+v", kind, *got)
		}
	}
	cancelled := &protocol.Fault{Code: protocol.ErrorCodeCancelled}
	if got := reportFault(logging.NewQuery("command"), cancelled); got != cancelled || got.ReportRef != "" {
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
		{stage: "completion", code: protocol.ErrorCodeUnavailable, kind: "network", want: true},
		{stage: "completion", code: protocol.ErrorCodeNotFound, want: true},
		{stage: "admission", code: protocol.ErrorCodeUnavailable, want: true},
		{stage: "completion", code: protocol.ErrorCodeCancelled},
	} {
		span := node.commandSpan(node.ID(), request)
		span.Set(slog.String("command_stage", test.stage))
		original := &protocol.Fault{Code: test.code, Message: "private provider address and stderr"}
		if test.kind != "" {
			original.Details = &protocol.FaultDetails{Kind: test.kind}
		}
		fault := reportFault(span, original)
		recorded = nil
		rejectCommand(span, kind, fault, false)
		if (fault != original) != test.want || logging.ValidReportRef(fault.ReportRef) != test.want || original.ReportRef != "" {
			t.Fatalf("%s %s (%s): unexpected reference %q", test.stage, test.code, test.kind, fault.ReportRef)
		}
		if test.code == protocol.ErrorCodeCancelled {
			if recorded != nil {
				t.Fatalf("cancel reached error reporting: %v", recorded)
			}
			continue
		}
		ref := fault.ReportRef
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
