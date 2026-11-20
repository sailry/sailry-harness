package node

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync/atomic"
	"testing"

	gogit "github.com/go-git/go-git/v5"
	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceReadCommandDiagnostics(t *testing.T) {
	home := t.TempDir()
	for _, key := range []string{"HOME", "USERPROFILE", "XDG_CONFIG_HOME"} {
		t.Setenv(key, home)
	}
	t.Setenv("GIT_CONFIG_NOSYSTEM", "1")
	t.Setenv("GIT_CONFIG_GLOBAL", "")
	root := filepath.Join(t.TempDir(), "private-project-path")
	if err := os.Mkdir(root, 0o700); err != nil {
		t.Fatal(err)
	}
	if _, err := gogit.PlainInit(root, false); err != nil {
		t.Fatal(err)
	}
	const entry, contents, missing = "private-entry.txt", "private file contents", "private-missing-directory"
	if err := os.WriteFile(filepath.Join(root, entry), []byte(contents), 0o600); err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(t.TempDir(), "profile")
	closeLog, err := logging.Install(profile, "desktop")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(closeLog)
	server := newSentryFixture(t)
	const localCause = "private Git process failure"
	var inspected atomic.Bool
	node, err := StartWithOptions(t.Context(), profile, Options{
		Environment: new(process.NewEnvironment(nil)),
		MCP:         fixtureMCPOptions(),
		Git: workspacegit.Options{Run: func(_ context.Context, _ string, args []string, _ []byte) (workspacegit.CommandResult, error) {
			switch {
			case slices.Contains(args, "status"):
				inspected.Store(true)
				return workspacegit.CommandResult{}, errors.New(localCause)
			case slices.Contains(args, "worktree"):
				return workspacegit.CommandResult{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "Git executable not found on execution Node"}
			default:
				return workspacegit.CommandResult{}, fmt.Errorf("unexpected Git arguments: %v", args)
			}
		}},
		Telemetry: telemetry.Options{Lookup: environment(map[string]string{"SAILRY_SENTRY_DSN": server.dsn()})},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = node.Stop(context.Background()) })
	if !node.Telemetry().Errors() {
		t.Fatal("fixture must allow automatic error reports")
	}
	project := commandFixture(t, node, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Workspace", Path: root}}).Data.(protocol.Project)
	trees := snapshotFixture(t, node).Worktrees
	index := slices.IndexFunc(trees, func(tree protocol.Worktree) bool { return tree.Project != nil && *tree.Project == project.ID })
	if index < 0 {
		t.Fatal("registered project has no worktree")
	}
	tree := trees[index].ID
	directory := func(path string) protocol.Command {
		return protocol.Command{Kind: "list_directory", Data: protocol.CommandListDirectoryPayload{Worktree: tree, Path: path}}
	}
	success := protocol.NewRequest(node.ID(), directory("."))
	if got := dispatchFixture(t, node, node.ID(), success); got.Fault != nil || got.Output.Kind != "directory" {
		t.Fatalf("workspace root listing = %v", got)
	}
	type failure struct {
		id    protocol.RequestID
		kind  string
		level string
		code  protocol.ErrorCode
		ref   string
	}
	var failures []failure
	for _, test := range []struct {
		command protocol.Command
		level   string
		code    protocol.ErrorCode
	}{
		{directory(missing), "WARN", protocol.ErrorCodeNotFound},
		{protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: tree}}, "ERROR", protocol.ErrorCodeUnavailable},
		{protocol.Command{Kind: "list_worktrees", Data: protocol.CommandListWorktreesPayload{Worktree: tree}}, "WARN", protocol.ErrorCodeNotConfigured},
	} {
		request := protocol.NewRequest(node.ID(), test.command)
		fault := dispatchFixture(t, node, node.ID(), request).Fault
		if fault == nil || fault.Code != test.code {
			t.Fatalf("%s = %v, want %s", test.command.Kind, fault, test.code)
		}
		if !logging.ValidReportRef(fault.ReportRef) {
			t.Fatalf("%s failure has no valid reference: %q", test.command.Kind, fault.ReportRef)
		}
		failures = append(failures, failure{request.ID, test.command.Kind, test.level, test.code, fault.ReportRef})
	}
	if !inspected.Load() {
		t.Fatal("inspection did not reach native Git status")
	}
	const invalidPackage = "private package/name"
	install := protocol.NewRequest(node.ID(), protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: invalidPackage}})
	admission, err := node.Dispatch(t.Context(), node.ID(), install)
	if err != nil || !admission.Receipt.Durable {
		t.Fatalf("fixture installation was not admitted durably: %v, %v", admission.Receipt, err)
	}
	if completion := <-admission.Completion; completion.Fault == nil || completion.Fault.Code != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("invalid package did not fail after admission: %+v", completion)
	}
	if err := node.Stop(t.Context()); err != nil {
		t.Fatal(err)
	}
	if got := server.count(); got != 0 {
		t.Errorf("controller workspace reads sent %d automatic reports", got)
	}
	path, err := logging.CurrentFile()
	if err != nil {
		t.Fatal(err)
	}
	local, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(string(local), "\n")
	for _, line := range lines {
		if strings.Contains(line, " request="+success.ID.String()+" ") {
			_, elapsed, _ := strings.Cut(line, " duration_us=")
			var duration uint64
			if _, err := fmt.Sscanf(elapsed, "%d", &duration); err != nil || duration < 100_000 || !strings.Contains(line, " phase=completed ") {
				t.Errorf("routine success was logged as %q", line)
			}
		}
	}
	for _, want := range failures {
		matched := 0
		for _, line := range lines {
			if !strings.Contains(line, " request="+want.id.String()+" ") {
				continue
			}
			matched++
			if !strings.Contains(line, " "+want.level+" desktop area=command phase=failed ") ||
				!strings.Contains(line, " worktree="+tree.String()+" command="+want.kind+" ") ||
				!strings.Contains(line, " command_stage=completion ") || !slices.Contains(strings.Fields(line), "report_ref="+want.ref) || !strings.Contains(line, " code="+string(want.code)+" ") {
				t.Errorf("%s failure logged as %q", want.kind, line)
			}
		}
		if matched != 1 {
			t.Errorf("%s failure recorded %d times", want.kind, matched)
		}
	}
	installFailures := 0
	for _, line := range lines {
		if strings.Contains(line, " request="+install.ID.String()+" ") && strings.Contains(line, " phase=failed ") {
			installFailures++
			if !strings.Contains(line, " WARN desktop area=command phase=failed ") ||
				!strings.Contains(line, " command=install_bundled_plugin ") || !strings.Contains(line, " command_stage=completion code=invalid_request ") {
				t.Errorf("installation failure logged as %q", line)
			}
		}
	}
	if installFailures != 1 {
		t.Errorf("installation failure recorded %d times", installFailures)
	}
	tail, err := logging.Tail(path)
	if err != nil {
		t.Fatal(err)
	}
	for _, private := range []string{filepath.Base(root), entry, contents, missing, localCause, invalidPackage} {
		if bytes.Contains(local, []byte(private)) || bytes.Contains(tail, []byte(private)) {
			t.Errorf("diagnostics kept private value %q", private)
		}
	}
	if bytes.Contains(tail, []byte(tree.String())) || bytes.Contains(tail, []byte(success.ID.String())) {
		t.Error("diagnostic attachment kept a local worktree or request identifier")
	}
	for _, failure := range failures {
		if bytes.Contains(tail, []byte(failure.id.String())) {
			t.Errorf("diagnostic attachment kept request identifier %s", failure.id)
		}
	}
}
