package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginStatusContracts(t *testing.T) {
	info := pluginStatusFixtureInfo(t, "status-owner", 'a')
	value := json.RawMessage(`[{"id":"badge","visible":true,"enabled":true,"segments":[{"label":{"label":"Ready"},"tone":"success"}],"details":[{"label":{"label":"Items"},"value":"4"}]}]`)
	states, err := ValidatePluginStatusStates(info, value)
	if err != nil {
		t.Fatal(err)
	}
	entries := PluginStatusEntries(info, states)
	if len(entries) != 2 || entries[0].Declaration.ID != "badge" || !entries[0].State.Visible ||
		entries[0].Invoke == nil || *entries[0].Invoke != "invoke" || len(entries[0].State.Details) != 1 ||
		entries[1].State.Visible || entries[1].State.Enabled {
		t.Fatal("Status declaration capture or omitted pending state changed")
	}
	for _, invalid := range []string{
		`null`, `{}`, `[{"id":"missing"}]`, `[{"id":"settings"}]`,
		`[{"id":"badge","reply_to":1}]`, `[{"id":"badge","value":true}]`, `[{"id":"badge","choices":[]}]`,
		`[{"id":"badge"},{"id":"badge"}]`,
		`[{"id":"badge","label":{"label":"` + strings.Repeat("x", protocol.PluginUiMaxStateBytes) + `"}}]`,
		`[` + strings.Repeat(`{"id":"badge"},`, protocol.PluginUiMaxEntries) + `{"id":"badge"}]`,
	} {
		if _, err := ValidatePluginStatusStates(info, json.RawMessage(invalid)); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
			t.Fatalf("invalid Status output was accepted: %v", err)
		}
	}
	popover := info
	extension := *info.Extension
	extension.Ui = slices.Clone(extension.Ui)
	extension.Ui[0].Kind = protocol.PluginUiKindPopover
	popover.Extension = &extension
	if projected := PluginStatusEntries(popover, states); projected[0].Invoke != nil {
		t.Fatal("controller-local popover gained a host invoke")
	}
	storage, _, session, _, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		scope := protocol.PluginUiStatusScope{Session: &session.ID}
		effective, err := ResolvePluginStatusScope(ctx, db, scope)
		if err != nil || effective.Worktree == nil || *effective.Worktree != session.Worktree || effective.Session == nil || *effective.Session != session.ID {
			return fmt.Errorf("Status session scope did not resolve its worktree: %v", err)
		}
		other := protocol.NewWorktreeID()
		if _, err := ResolvePluginStatusScope(ctx, db, protocol.PluginUiStatusScope{Session: &session.ID, Worktree: &other}); !providerFixtureFault(err, protocol.ErrorCodeWrongTarget) {
			return fmt.Errorf("Status accepted a different session worktree: %v", err)
		}
		if _, err := ResolvePluginStatusScope(ctx, db, protocol.PluginUiStatusScope{}); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
			return fmt.Errorf("Status accepted a Node-wide scope: %v", err)
		}
		for index, name := range []string{"status-owner", "status-peer", "status-shared", "status-desktop", "status-disabled"} {
			current := pluginStatusFixtureInfo(t, name, byte('a'+index))
			if name == "status-shared" {
				current.Extension.Desktop.UiShared = true
			}
			if name == "status-desktop" {
				current.Extension.Scope = protocol.PluginScopeDesktop
			}
			if _, err := InstallPlugin(ctx, db, current, 0, name != "status-disabled"); err != nil {
				return err
			}
		}
		selected, err := ReadPluginStatusPackages(ctx, db, effective)
		if err != nil || len(selected) != 3 {
			return fmt.Errorf("Status selected disabled, desktop-only or non-Status packages: %v, %d", err, len(selected))
		}
		current, err := ReadPlugin(ctx, db, info.Summary.Name)
		if err != nil || current == nil {
			return fmt.Errorf("Status package capture is unavailable: %v", err)
		}
		session.Config.Assistant = &protocol.PluginConversationBinding{Package: plugins.PackageReference(*current), ID: "assistant"}
		if _, _, err := reviseSession(ctx, db, session); err != nil {
			return err
		}
		selected, err = ReadPluginStatusPackages(ctx, db, effective)
		if err != nil || len(selected) != 2 || selected[0].Summary.Name != "status-owner" || selected[1].Summary.Name != "status-shared" {
			return fmt.Errorf("assistant Status did not retain only its package and shared UI: %v", err)
		}
		report := protocol.DefaultNotificationSystemSystemDiagnostic()
		report.Incident, report.Source = protocol.NewRequestID(), protocol.NotificationSystemSystemSourcePluginCallback
		report.Phase, report.Reason, report.Code = protocol.NotificationSystemDiagnosticPhaseCallback, protocol.NotificationSystemDiagnosticReasonFailure, protocol.ErrorCodeInvalidRequest
		report.Version, report.Platform, report.Session, report.Worktree = "0.1.0", "fixture", &session.ID, &session.Worktree
		report.Plugin = &protocol.NotificationSystemDiagnosticPlugin{Name: info.Summary.Name, Version: "0.1.0"}
		changes, err := RecordPluginStatusDiagnostic(ctx, db, report, 1000)
		if err != nil || len(changes) != 1 {
			return fmt.Errorf("Status diagnostic was not recorded: %v", err)
		}
		if repeated, err := RecordPluginStatusDiagnostic(ctx, db, report, 1001); err != nil || len(repeated) != 0 {
			return fmt.Errorf("Status diagnostic incident was repeated: %v", err)
		}
		notices, err := ReadNotifications(ctx, db)
		if err != nil || len(notices) != 1 || notices[0].Package != "" || notices[0].Content.Title != "" ||
			notices[0].Content.Message != "" || notices[0].Content.Details == nil || notices[0].Content.Details.Kind != "system" {
			return fmt.Errorf("Status diagnostic did not use the safe system projection: %v", err)
		}
		report.Incident, report.Code = protocol.NewRequestID(), protocol.ErrorCodeCancelled
		if changes, err := RecordPluginStatusDiagnostic(ctx, db, report, 1002); err != nil || len(changes) != 0 {
			return fmt.Errorf("cancelled Status evaluation created a failure notice: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func pluginStatusFixtureInfo(t *testing.T, name string, digest byte) protocol.PluginInfo {
	t.Helper()
	info := pluginTestInfo(name, digest)
	var extension protocol.PluginExtension
	if err := json.Unmarshal([]byte(`{
		"api_version":"v1","scope":"host",
		"host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
			"handlers":["read","invoke"],"read_only":["read"],"status":{"read":"read","invoke":"invoke"}},
		"desktop":{"ui_entry":"dev.sailry.platform/view.js","resources":["dev.sailry.platform/view.js"]},
		"ui":[
			{"id":"badge","slot":"status","kind":"button","label":{"label":"Badge"},"handler":"badge"},
			{"id":"other","slot":"status","kind":"button","label":{"label":"Other"},"handler":"other"},
			{"id":"settings","slot":"context","kind":"button","label":{"label":"Settings"},"handler":"settings"}
		]
	}`), &extension); err != nil {
		t.Fatal(err)
	}
	info.Extension = &extension
	return info
}
