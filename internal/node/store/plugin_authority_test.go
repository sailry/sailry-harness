package store

import (
	"database/sql"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginControllerAuthority(t *testing.T) {
	storage, _, session, _, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("controller-fixture", 'a')
		info.Extension = new(protocol.DefaultPluginExtension())
		info.Extension.Actions = []protocol.PluginAction{
			protocol.PluginActionReadModels, protocol.PluginActionReadStorage, protocol.PluginActionWriteStorage,
			protocol.PluginActionReadActivity, protocol.PluginActionReadUsage, protocol.PluginActionReadSearchSettings,
			protocol.PluginActionReadTerminals, protocol.PluginActionControlTerminals,
		}
		info.Extension.SettingsPage = new(protocol.DefaultPluginDesktopSettings())
		scope := protocol.DefaultPluginContext()
		scope.Package = plugins.PackageReference(info)
		scope.Surface.Kind = "settings"
		for _, scenario := range []struct {
			name   string
			kind   string
			change func(*protocol.PluginContext, *protocol.PluginInfo)
			code   protocol.ErrorCode
		}{
			{"disabled public settings", "read_plugin_settings", nil, ""},
			{"disabled private storage", "read_plugin_value", nil, ""},
			{"read models alternative", "list_plugin_models", nil, ""},
			{"disabled search settings", "read_search_settings", nil, ""},
			{"disabled mutation remains refused", "call_plugin", nil, protocol.ErrorCodeNotConfigured},
			{"missing settings declaration", "read_plugin_settings", func(_ *protocol.PluginContext, info *protocol.PluginInfo) {
				info.Extension.SettingsPage = nil
			}, protocol.ErrorCodePermissionDenied},
			{"workspace disabled storage", "read_plugin_value", func(scope *protocol.PluginContext, _ *protocol.PluginInfo) {
				scope.Surface.Kind = "workspace"
			}, protocol.ErrorCodeNotConfigured},
			{"settings resource scope", "read_search_settings", func(scope *protocol.PluginContext, _ *protocol.PluginInfo) {
				scope.Worktree = new(protocol.NewWorktreeID())
			}, protocol.ErrorCodePermissionDenied},
			{"activity resource scope", "read_activity_catalog", func(scope *protocol.PluginContext, info *protocol.PluginInfo) {
				info.Summary.Enabled = true
				scope.Worktree = new(protocol.NewWorktreeID())
			}, protocol.ErrorCodePermissionDenied},
			{"activity Node scope", "read_activity_catalog", func(_ *protocol.PluginContext, info *protocol.PluginInfo) {
				info.Summary.Enabled = true
			}, ""},
			{"usage composer refusal", "read_usage", func(scope *protocol.PluginContext, info *protocol.PluginInfo) {
				info.Summary.Enabled = true
				scope.Surface.Kind = "composer"
			}, protocol.ErrorCodePermissionDenied},
		} {
			captured := scope
			current := info
			extension := *info.Extension
			current.Extension = &extension
			if scenario.change != nil {
				scenario.change(&captured, &current)
			}
			request := protocol.Request{Plugin: &captured, Command: protocol.Command{Kind: scenario.kind}}
			err := CheckPluginRequest(ctx, db, request, current)
			var rejected *protocol.Fault
			if scenario.code == "" && err != nil || scenario.code != "" && (!errors.As(err, &rejected) || rejected.Code != scenario.code) {
				return fmt.Errorf("%s: unexpected plugin authority result: %v", scenario.name, err)
			}
		}
		info.Summary.Enabled = true
		scope.Surface.Kind = "workspace"
		worktree := session.Worktree
		scope.Worktree = &worktree
		terminal := protocol.DefaultTerminalInfo()
		terminal.ID, terminal.Worktree = protocol.NewTerminalID(), &worktree
		terminal.Status.Kind, terminal.Revision = "running", 1
		terminal.Access = new(protocol.ProcessAccessUnrestricted)
		if _, err := SaveTerminal(ctx, db, terminal, false); err != nil {
			return err
		}
		open := protocol.DefaultCommandOpenTerminalPayload()
		open.Terminal = terminal.ID
		open.Appearance.ColorScheme = protocol.TerminalColorSchemeLight
		launch := protocol.DefaultTerminalLaunch()
		launch.Worktree, launch.Access = worktree, protocol.ProcessAccessUnrestricted
		launch.Appearance.ColorScheme = protocol.TerminalColorSchemeLight
		for _, scenario := range []struct {
			name    string
			command protocol.Command
			change  func(*protocol.PluginContext)
			denied  bool
		}{
			{"native desktop open", protocol.Command{Kind: "open_terminal", Data: &open}, nil, false},
			{"native read", protocol.Command{Kind: "inspect_terminal", Data: &protocol.CommandInspectTerminalPayload{Terminal: terminal.ID}}, nil, false},
			{"unrestricted script input", protocol.Command{Kind: "input_terminal", Data: &protocol.CommandInputTerminalPayload{Terminal: terminal.ID}}, nil, true},
			{"another worktree", protocol.Command{Kind: "inspect_terminal", Data: &protocol.CommandInspectTerminalPayload{Terminal: terminal.ID}}, func(scope *protocol.PluginContext) {
				scope.Worktree = new(protocol.NewWorktreeID())
			}, true},
			{"callback unrestricted open", protocol.Command{Kind: "open_terminal", Data: &open}, func(scope *protocol.PluginContext) {
				scope.Invocation = new(protocol.NewRequestID())
			}, true},
			{"settings unrestricted launch", protocol.Command{Kind: "open_tool_terminal", Data: &protocol.CommandOpenToolTerminalPayload{
				Tool: protocol.TerminalToolsToolClaude, Launch: launch,
			}}, func(scope *protocol.PluginContext) {
				scope.Surface.Kind = "settings"
			}, true},
		} {
			captured := scope
			if scenario.change != nil {
				scenario.change(&captured)
			}
			err := CheckPluginRequest(ctx, db, protocol.Request{Plugin: &captured, Command: scenario.command}, info)
			var rejected *protocol.Fault
			if !scenario.denied && err != nil || scenario.denied && (!errors.As(err, &rejected) || rejected.Code != protocol.ErrorCodePermissionDenied) {
				return fmt.Errorf("%s: unexpected terminal authority result: %v", scenario.name, err)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func testPluginPlanActions(t *testing.T) {
	storage, _, session, turn, _ := agentHistoryFixture(t, protocol.WorkModePlan)
	scope := protocol.DefaultPluginContext()
	scope.Worktree, scope.Session, scope.Turn = &session.Worktree, &session.ID, &turn
	err := storage.Node.Read(t.Context(), func(db *sql.Conn) error {
		cases := []struct {
			command protocol.Command
			action  protocol.PluginAction
			allowed bool
		}{
			{protocol.Command{Kind: "inspect_terminal"}, protocol.PluginActionReadTerminals, true},
			{protocol.Command{Kind: "write_plugin_value"}, protocol.PluginActionWriteStorage, false},
			{protocol.Command{Kind: "list_plugin_models"}, protocol.PluginActionGenerateText, true},
			{protocol.Command{Kind: "read_plugin_connection"}, "", false},
			{protocol.Command{Kind: "write_artifact"}, protocol.PluginActionWriteArtifacts, true},
			{protocol.Command{Kind: "install_local_model"}, protocol.PluginActionInstallModels, false},
			{protocol.Command{Kind: "query_database", Data: &protocol.CommandQueryDatabasePayload{ReadOnly: true}}, protocol.PluginActionReadDatabases, true},
			{protocol.Command{Kind: "query_database", Data: &protocol.CommandQueryDatabasePayload{ReadOnly: false}}, protocol.PluginActionReadDatabases, false},
			{protocol.Command{Kind: "use_browser", Data: &protocol.CommandUseBrowserPayload{Action: protocol.BrowserAction{
				Kind: "screenshot", Data: protocol.BrowserActionScreenshotPayload{Save: false},
			}}}, protocol.PluginActionReadBrowser, true},
			{protocol.Command{Kind: "use_browser", Data: &protocol.CommandUseBrowserPayload{Action: protocol.BrowserAction{
				Kind: "screenshot", Data: protocol.BrowserActionScreenshotPayload{Save: true},
			}}}, protocol.PluginActionReadBrowser, false},
			{protocol.Command{Kind: "dispatch", Data: &protocol.CommandDispatchPayload{Action: protocol.DispatchCommand{Kind: "list_handlers"}}}, protocol.PluginActionDispatch, true},
		}
		for _, kind := range []string{"claim_terminal", "input_terminal", "resize_terminal", "set_terminal_appearance"} {
			cases = append(cases, struct {
				command protocol.Command
				action  protocol.PluginAction
				allowed bool
			}{protocol.Command{Kind: kind}, protocol.PluginActionControlTerminals, false})
		}
		for _, scenario := range cases {
			request := protocol.Request{Plugin: &scope, Command: scenario.command}
			err := CheckPluginPlanAction(t.Context(), db, request, scenario.action)
			var denied *protocol.Fault
			if scenario.allowed && err != nil || !scenario.allowed && (!errors.As(err, &denied) || denied.Code != protocol.ErrorCodePermissionDenied) {
				return fmt.Errorf("%s: unexpected Plan action result: %v", scenario.command.Kind, err)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
