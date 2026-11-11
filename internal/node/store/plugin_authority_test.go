package store

import (
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginControllerAuthority(t *testing.T) {
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
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
		// A command addressed by turn stays inside the captured session and turn.
		out, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &session.Config})
		if err != nil {
			return err
		}
		foreign, err := delegationFixtureParent(ctx, db, node, out.Data.(protocol.Session), now)
		if err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		fork, _, err := forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: session.ID, Through: turn, ExpectedRevision: session.Revision})
		if err != nil {
			return err
		}
		info.Extension.Actions = append(info.Extension.Actions, protocol.PluginActionReadConversation, protocol.PluginActionControlConversation, protocol.PluginActionReadCommands, protocol.PluginActionControlCommands)
		scope.Session = &session.ID
		payloads := map[string]func(protocol.TurnID) any{
			"stop_turn":          func(id protocol.TurnID) any { return &protocol.CommandStopTurnPayload{Turn: id} },
			"start_queued_turn":  func(id protocol.TurnID) any { return &protocol.CommandStartQueuedTurnPayload{Turn: id} },
			"edit_queued_turn":   func(id protocol.TurnID) any { return &protocol.CommandEditQueuedTurnPayload{Turn: id} },
			"remove_queued_turn": func(id protocol.TurnID) any { return &protocol.CommandRemoveQueuedTurnPayload{Turn: id} },
			"withdraw_guidance":  func(id protocol.TurnID) any { return &protocol.CommandWithdrawGuidancePayload{Turn: id} },
			"send_queued_turn":   func(id protocol.TurnID) any { return &protocol.CommandSendQueuedTurnPayload{Turn: id} },
			"read_queued_turn":   func(id protocol.TurnID) any { return &protocol.CommandReadQueuedTurnPayload{Turn: id} },
			"read_tool_output":   func(id protocol.TurnID) any { return &protocol.CommandReadToolOutputPayload{Turn: id} },
			"run_command": func(id protocol.TurnID) any {
				value := protocol.DefaultCommandRunCommandPayload()
				value.Turn = id
				return &value
			},
		}
		type turnScenario struct {
			name    string
			kind    string
			target  protocol.TurnID
			change  func(*protocol.PluginContext)
			code    protocol.ErrorCode
			message string
		}
		scenarios := []turnScenario{
			{"captured tool output", "read_tool_output", turn, func(scope *protocol.PluginContext) { scope.Turn = &turn }, "", ""},
			{"tool output without a turn", "read_tool_output", turn, nil, protocol.ErrorCodePermissionDenied, "tool output read is outside the captured turn"},
			{"tool output of another turn", "read_tool_output", foreign, func(scope *protocol.PluginContext) { scope.Turn = &turn }, protocol.ErrorCodePermissionDenied, "tool output read is outside the captured turn"},
			{"command of another turn", "run_command", foreign, func(scope *protocol.PluginContext) { scope.Turn = &turn }, protocol.ErrorCodePermissionDenied, "command is outside the captured turn"},
		}
		for _, kind := range []string{"stop_turn", "start_queued_turn", "edit_queued_turn", "remove_queued_turn", "withdraw_guidance", "send_queued_turn", "read_queued_turn", "run_command"} {
			scenarios = append(scenarios,
				turnScenario{"own turn", kind, turn, nil, "", ""},
				turnScenario{"another session", kind, foreign, nil, protocol.ErrorCodeWrongTarget, "turn is not in this conversation"},
				turnScenario{"no session scope", kind, turn, func(scope *protocol.PluginContext) { scope.Session = nil }, protocol.ErrorCodePermissionDenied, "plugin has no session scope"},
				turnScenario{"inherited turn", kind, turn, func(scope *protocol.PluginContext) { scope.Session, scope.Worktree = &fork.ID, &fork.Worktree }, protocol.ErrorCodePermissionDenied, "plugin turn belongs to another session"},
			)
		}
		for _, scenario := range scenarios {
			captured := scope
			if scenario.change != nil {
				scenario.change(&captured)
			}
			err := CheckPluginRequest(ctx, db, protocol.Request{Plugin: &captured, Command: protocol.Command{Kind: scenario.kind, Data: payloads[scenario.kind](scenario.target)}}, info)
			var rejected *protocol.Fault
			if scenario.code == "" && err != nil || scenario.code != "" && (!errors.As(err, &rejected) || rejected.Code != scenario.code || rejected.Message != scenario.message) {
				return fmt.Errorf("%s %s: unexpected turn authority result: %v", scenario.kind, scenario.name, err)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func testPluginSessionAuthority(t *testing.T) {
	storage, node, session, turn, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("session-fixture", 'a')
		info.Summary.Enabled = true
		info.Extension = new(protocol.DefaultPluginExtension())
		info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionStartSessions, protocol.PluginActionReadConversation, protocol.PluginActionControlConversation}
		scope := protocol.DefaultPluginContext()
		scope.Package = plugins.PackageReference(info)
		associated := func(namespace string) (protocol.Session, error) {
			out, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &session.Config})
			if err != nil {
				return protocol.Session{}, err
			}
			created := out.Data.(protocol.Session)
			if namespace != "" {
				_, err = db.ExecContext(ctx, "INSERT INTO session_associations(session,namespace,kind,identity) VALUES(?1,?2,'task',?3)", created.ID.String(), namespace, created.ID.String())
			}
			return created, err
		}
		owned, err := associated("session-fixture")
		if err != nil {
			return err
		}
		foreign, err := associated("another-package")
		if err != nil {
			return err
		}
		draft := func(change func(*protocol.ConversationStart)) protocol.Command {
			value := protocol.DefaultConversationStart()
			value.Worktree, value.Execution = &session.Worktree, protocol.ConversationExecutionModel
			value.Title, value.Message.Text = "Task", "Run the task"
			value.Association = &protocol.ConversationAssociation{Namespace: "session-fixture", Kind: "task", ID: "task-1"}
			if change != nil {
				change(&value)
			}
			return protocol.Command{Kind: "start_session", Data: &value}
		}
		config := func(change func(*protocol.SessionConfig)) *protocol.SessionConfig {
			value := session.Config
			change(&value)
			return &value
		}
		other := protocol.NewWorktreeID()
		for _, scenario := range []struct {
			name    string
			command protocol.Command
			change  func(*protocol.PluginContext)
			code    protocol.ErrorCode
			message string
		}{
			{"Node-wide start names a worktree", draft(nil), nil, "", ""},
			{"start without association", draft(func(value *protocol.ConversationStart) { value.Association = nil }), nil, "", ""},
			{"start inside the captured worktree", draft(nil), func(scope *protocol.PluginContext) { scope.Worktree = &session.Worktree }, "", ""},
			{"start under another package's namespace", draft(func(value *protocol.ConversationStart) { value.Association.Namespace = "another-package" }), nil,
				protocol.ErrorCodePermissionDenied, "session association belongs to another package"},
			{"start with an invalid association", draft(func(value *protocol.ConversationStart) { value.Association.Kind = "" }), nil, protocol.ErrorCodeInvalidRequest, ""},
			{"start from a turn", draft(nil), func(scope *protocol.PluginContext) {
				scope.Worktree, scope.Session, scope.Turn = &session.Worktree, &session.ID, &turn
			}, protocol.ErrorCodePermissionDenied, "command is not exposed to plugin software actions"},
			{"start from a session", draft(nil), func(scope *protocol.PluginContext) { scope.Worktree, scope.Session = &session.Worktree, &session.ID },
				protocol.ErrorCodePermissionDenied, "command is not exposed to plugin software actions"},
			{"start outside the captured worktree", draft(nil), func(scope *protocol.PluginContext) { scope.Worktree = &other },
				protocol.ErrorCodePermissionDenied, "session creation is outside the captured worktree"},
			{"start an assistant conversation", draft(func(value *protocol.ConversationStart) {
				value.Config = config(func(value *protocol.SessionConfig) {
					value.Assistant = &protocol.PluginConversationBinding{Package: scope.Package, ID: "assistant"}
				})
			}), nil, protocol.ErrorCodePermissionDenied, "session creation requires a project or unassigned scope"},
			{"start with another package's options", draft(func(value *protocol.ConversationStart) {
				value.Config = config(func(value *protocol.SessionConfig) {
					value.Options = map[string]map[string]json.RawMessage{"another-package": {"depth": json.RawMessage(`"deep"`)}}
				})
			}), nil, protocol.ErrorCodePermissionDenied, "conversation options belong to another package"},
			{"read an owned session", protocol.Command{Kind: "read_session", Data: &protocol.CommandReadSessionPayload{Session: owned.ID}}, nil, "", ""},
			{"queue into an owned session", protocol.Command{Kind: "queue_turn", Data: &protocol.CommandQueueTurnPayload{Session: owned.ID}}, nil, "", ""},
			{"read an owned session inside its worktree", protocol.Command{Kind: "read_session", Data: &protocol.CommandReadSessionPayload{Session: owned.ID}},
				func(scope *protocol.PluginContext) { scope.Worktree = &owned.Worktree }, "", ""},
			{"read an owned session from another worktree", protocol.Command{Kind: "read_session", Data: &protocol.CommandReadSessionPayload{Session: owned.ID}},
				func(scope *protocol.PluginContext) { scope.Worktree = &other }, protocol.ErrorCodePermissionDenied, "plugin action is outside the captured resource scope"},
			{"read another package's session", protocol.Command{Kind: "read_session", Data: &protocol.CommandReadSessionPayload{Session: foreign.ID}}, nil,
				protocol.ErrorCodePermissionDenied, "plugin action is outside the captured resource scope"},
			{"read an unassociated session", protocol.Command{Kind: "read_session", Data: &protocol.CommandReadSessionPayload{Session: session.ID}}, nil,
				protocol.ErrorCodePermissionDenied, "plugin action is outside the captured resource scope"},
			{"read a missing session", protocol.Command{Kind: "read_session", Data: &protocol.CommandReadSessionPayload{Session: protocol.NewSessionID()}}, nil,
				protocol.ErrorCodePermissionDenied, "plugin action is outside the captured resource scope"},
			{"read an owned session from a session scope", protocol.Command{Kind: "read_session", Data: &protocol.CommandReadSessionPayload{Session: owned.ID}},
				func(scope *protocol.PluginContext) { scope.Worktree, scope.Session = &session.Worktree, &session.ID },
				protocol.ErrorCodePermissionDenied, "plugin action is outside the captured resource scope"},
		} {
			captured := scope
			if scenario.change != nil {
				scenario.change(&captured)
			}
			err := CheckPluginRequest(ctx, db, protocol.Request{Plugin: &captured, Command: scenario.command}, info)
			var rejected *protocol.Fault
			if scenario.code == "" && err != nil || scenario.code != "" && (!errors.As(err, &rejected) || rejected.Code != scenario.code || scenario.message != "" && rejected.Message != scenario.message) {
				return fmt.Errorf("%s: unexpected session authority result: %v", scenario.name, err)
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
