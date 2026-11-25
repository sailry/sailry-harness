package node

import (
	"context"
	"log/slog"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/installations"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/tasks"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginCommandTerminalObservation(t *testing.T) {
	path := filepath.Join(t.TempDir(), "profile")
	closeLog, err := logging.Install(path, "host")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	profile, err := OpenProfile(t.Context(), path)
	if err != nil {
		t.Fatal(err)
	}
	defer profile.Close()
	var reportsMu sync.Mutex
	reports := map[string][]logging.Fields{}
	logging.SetReport(func(level slog.Level, fields logging.Fields) {
		if id, ok := fields["request"].(string); level == slog.LevelError && ok {
			reportsMu.Lock()
			reports[id] = append(reports[id], fields)
			reportsMu.Unlock()
		}
	})
	defer logging.SetReport(nil)
	reportsFor := func(id protocol.RequestID) []logging.Fields {
		reportsMu.Lock()
		defer reportsMu.Unlock()
		return append([]logging.Fields(nil), reports[id.String()]...)
	}
	owner := tasks.New(t.Context())
	defer owner.Close(context.Background())
	node := &Node{profile: profile, tasks: owner, ctx: t.Context(), logs: true, auth: &authControls{effects: map[protocol.RequestID]func(bool){}}}
	queue := installations.New(owner, node.ID(), t.Context().Done())
	// The absent manager makes the preparation and catalog work panic.
	node.plugins = &pluginControls{installations: queue}
	for _, test := range []struct {
		command protocol.Command
		code    protocol.ErrorCode
		count   int
	}{
		{protocol.Command{Kind: "search_plugin_catalog", Data: protocol.CommandSearchPluginCatalogPayload{Source: protocol.PluginCatalogSourceOfficial, Page: 1}}, protocol.ErrorCodeOutcomeUnknown, 1},
		{protocol.Command{Kind: "search_skill_catalog", Data: protocol.CommandSearchSkillCatalogPayload{Source: protocol.PluginSkillcatalogSourceOfficial, Page: 1}}, protocol.ErrorCodeOutcomeUnknown, 1},
		{protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: "fixture", Enabled: true}}, protocol.ErrorCodeOutcomeUnknown, 1},
		{protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: "fixture"}}, protocol.ErrorCodeOutcomeUnknown, 1},
		{protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "missing"}}, protocol.ErrorCodeNotFound, 0},
	} {
		t.Run(test.command.Kind, func(t *testing.T) {
			request := protocol.NewRequest(node.ID(), test.command)
			result := dispatchFixture(t, node, node.ID(), request)
			if result.Fault == nil || result.Fault.Code != test.code {
				t.Fatalf("completion fault = %v, want %s", result.Fault, test.code)
			}
			matching := reportsFor(request.ID)
			if len(matching) != test.count {
				t.Fatalf("terminal reports = %v, want %d", matching, test.count)
			}
			if test.count != 0 {
				fields := matching[0]
				if fields["operation"] != test.command.Kind || fields["code"] != string(test.code) ||
					!logging.ValidReportRef(result.Fault.ReportRef) || fields["report_ref"] != result.Fault.ReportRef ||
					fields["phase"] != logging.Failed || fields["command_stage"] != "completion" || fields["exception"] != nil || fields["detail"] != nil {
					t.Fatalf("terminal fields = %v", fields)
				}
			}
		})
	}
	t.Run("queue fallback closes completion for Dispatch", func(t *testing.T) {
		ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
		defer cancel()
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: "fixture"}})
		node.auth.mu.Lock()
		node.auth.effects[request.ID] = func(bool) { panic("fixture recovery failure") }
		node.auth.mu.Unlock()
		accepted, err := node.Dispatch(ctx, node.ID(), request)
		if err != nil {
			t.Fatal(err)
		}
		select {
		case result, ok := <-accepted.Completion:
			if ok {
				t.Fatalf("unexpected response following a panic in recovery: %v", result)
			}
		case <-ctx.Done():
			t.Fatalf("Dispatch did not observe the missing completion: %v", ctx.Err())
		}
		if err := owner.Close(ctx); err != nil {
			t.Fatal(err)
		}
		items, err := queue.Read(node.ID(), []protocol.RequestID{request.ID}, nil)
		if err != nil || len(items) != 1 || items[0].State.Kind != "uncertain" {
			t.Fatalf("installation fallback state = %v, error = %v", items, err)
		}
		matching := reportsFor(request.ID)
		if len(matching) != 1 || matching[0]["area"] != "command" || matching[0]["operation"] != request.Command.Kind ||
			matching[0]["phase"] != logging.Failed || matching[0]["command_stage"] != "completion" || matching[0]["code"] != string(protocol.ErrorCodeOutcomeUnknown) {
			t.Fatalf("terminal reports = %v", matching)
		}
	})
}
