package node

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/getsentry/sentry-go"
	gogit "github.com/go-git/go-git/v5"
	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestGitBadgeCommandReporting(t *testing.T) {
	home := t.TempDir()
	for _, key := range []string{"HOME", "USERPROFILE", "XDG_CONFIG_HOME"} {
		t.Setenv(key, home)
	}
	root := t.TempDir()
	if _, err := gogit.PlainInit(root, false); err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(t.TempDir(), "profile")
	closeLog, err := logging.Install(profile, "desktop")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(closeLog)
	server := newSentryFixture(t)
	const localCause = "private fixture Git process failure"
	var inspected atomic.Bool
	node, err := StartWithOptions(t.Context(), profile, Options{
		Environment: new(process.NewEnvironment(nil)),
		Git: workspacegit.Options{Run: func(_ context.Context, _ string, args []string, _ []byte) (workspacegit.CommandResult, error) {
			if slices.Contains(args, "status") {
				inspected.Store(true)
			}
			return workspacegit.CommandResult{}, errors.New(localCause)
		}},
		Telemetry: telemetry.Options{Lookup: environment(map[string]string{"SAILRY_SENTRY_DSN": server.dsn()})},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = node.Stop(context.Background()) })
	project := commandFixture(t, node, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Git badge", Path: root}}).Data.(protocol.Project)
	config := sessionConfigFixture(t, node)
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	output := commandFixture(t, node, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "git"}})
	info, ok := dispatchPayload[protocol.PluginInfo](output.Data)
	if !ok || info.Extension == nil || info.Extension.Host == nil || info.Extension.Host.Status == nil {
		t.Fatal("bundled Git Status callback is unavailable")
	}
	command := protocol.Command{Kind: "read_plugin_callback", Data: protocol.CommandReadPluginCallbackPayload{Handler: info.Extension.Host.Status.Read, Input: json.RawMessage(`{}`)}}
	scope := protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "composer"}, Package: plugins.PackageReference(info), Worktree: &session.Worktree, Session: &session.ID}
	missing := protocol.NewRequest(node.ID(), command)
	missingScope := scope
	missingScope.Package.Name = "missing-git-fixture"
	missing.Plugin = &missingScope
	if fault := dispatchFixture(t, node, node.ID(), missing).Fault; fault == nil || fault.Code != protocol.ErrorCodeNotFound {
		t.Fatalf("missing package returned %v", fault)
	}
	request := protocol.NewRequest(node.ID(), command)
	request.Plugin = &scope
	fault := dispatchFixture(t, node, node.ID(), request).Fault
	if fault == nil || fault.Code != protocol.ErrorCodeUnavailable || fault.Message != "Git repository operation failed: "+localCause {
		t.Fatalf("Git callback returned %v", fault)
	}
	if !inspected.Load() {
		t.Fatal("callback did not reach native Git status")
	}
	report, ok := faultDiagnostic(fault)
	if !ok || report.Source != protocol.NotificationSystemSystemSourcePluginCallback || report.Phase != protocol.NotificationSystemDiagnosticPhaseCallback ||
		report.Reason != protocol.NotificationSystemDiagnosticReasonFailure || report.Code != fault.Code || report.Exception != nil ||
		report.Worktree == nil || *report.Worktree != session.Worktree || report.Session == nil || *report.Session != session.ID {
		t.Fatalf("callback diagnostic = %+v", report)
	}
	if err := node.Stop(t.Context()); err != nil {
		t.Fatal(err)
	}
	logFile, err := logging.CurrentFile()
	if err != nil {
		t.Fatal(err)
	}
	logged, err := os.ReadFile(logFile)
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []struct{ id, level, code, stage string }{
		{missing.ID.String(), "WARN", "not_found", "admission"},
		{request.ID.String(), "ERROR", "unavailable", "completion"},
	} {
		if !slices.ContainsFunc(strings.Split(string(logged), "\n"), func(line string) bool {
			return strings.Contains(line, " "+want.level+" desktop area=command phase=failed ") && strings.Contains(line, " request="+want.id+" ") &&
				strings.Contains(line, " code="+want.code) && strings.Contains(line, " command_stage="+want.stage)
		}) {
			t.Errorf("missing local %s record for %s (%s)", want.level, want.code, want.stage)
		}
	}
	server.mu.Lock()
	defer server.mu.Unlock()
	if len(server.bodies) == 0 {
		t.Fatal("Git failure did not reach the Sentry fixture")
	}
	for _, body := range server.bodies {
		lines := bytes.SplitN(body, []byte("\n"), 4)
		var event sentry.Event
		if len(lines) < 3 || json.Unmarshal(lines[2], &event) != nil {
			t.Fatal("invalid Sentry fixture envelope")
		}
		fields := event.Contexts["sailry"]
		if fields["code"] != "unavailable" || fields["command_stage"] != "completion" || fields["package"] != "third_party" ||
			len(event.Exception) != 1 || event.Exception[0].Type != "SailryError" || event.Exception[0].Mechanism == nil ||
			event.Exception[0].Mechanism.Handled == nil || !*event.Exception[0].Mechanism.Handled {
			t.Fatalf("unexpected Sentry event: %s", lines[2])
		}
		for _, private := range []string{localCause, root, session.Worktree.String(), session.ID.String(), missingScope.Package.Name} {
			if bytes.Contains(body, []byte(private)) {
				t.Fatal("Sentry fixture received private data")
			}
		}
	}
}
