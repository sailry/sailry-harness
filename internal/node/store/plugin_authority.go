package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func PluginRequestPackage(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request, invocation *protocol.PluginInfo) (protocol.PluginInfo, error) {
	scope := request.Plugin
	if scope == nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodePermissionDenied, "plugin provenance is required")
	}
	if invocation != nil {
		return *invocation, nil
	}
	if scope.Invocation != nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodePermissionDenied, "plugin invocation is unavailable")
	}
	if scope.Turn != nil {
		if caller != node {
			return protocol.PluginInfo{}, fault(protocol.ErrorCodePermissionDenied, "plugin turn belongs to the execution Node")
		}
		if _, err := activeAgentRun(ctx, db, *scope.Turn); err != nil {
			return protocol.PluginInfo{}, err
		}
		capture, err := readTurnCapture(ctx, db, *scope.Turn)
		if err != nil {
			return protocol.PluginInfo{}, err
		}
		session, err := ReadSession(ctx, db, capture.Session)
		if err != nil {
			return protocol.PluginInfo{}, err
		}
		if scope.Session == nil || *scope.Session != capture.Session || scope.Worktree == nil || *scope.Worktree != session.Worktree || !slices.Contains(capture.Plugins, scope.Package) {
			return protocol.PluginInfo{}, fault(protocol.ErrorCodePermissionDenied, "plugin does not belong to the captured turn")
		}
		info, err := ResolvePluginInfo(ctx, db, scope.Package)
		info.Summary.Enabled = true
		return info, err
	}
	claimed, err := DispatchPackage(ctx, db, node, caller, request)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if claimed != nil {
		claimed.Summary.Enabled = true
		return *claimed, nil
	}
	current, err := ReadPlugin(ctx, db, scope.Package.Name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if current == nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotFound, "plugin is not installed")
	}
	reference := protocol.PluginReference{Name: current.Summary.Name, Digest: current.Summary.Digest, SettingsRevision: current.Summary.SettingsRevision}
	if reference != scope.Package {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeRevisionConflict, "plugin package changed")
	}
	return *current, nil
}

func CheckPluginRequest(ctx context.Context, db *sql.Conn, request protocol.Request, info protocol.PluginInfo) error {
	scope := request.Plugin
	if scope == nil {
		return nil
	}
	if info.Summary.Compatibility.Kind != "compatible" || info.Extension == nil {
		return fault(protocol.ErrorCodeNotConfigured, "plugin is incompatible")
	}
	if intrinsic, err := checkPluginConnectorRequest(ctx, db, request); err != nil {
		return err
	} else if intrinsic {
		if !info.Summary.Enabled || plugins.PackageReference(info) != scope.Package {
			return fault(protocol.ErrorCodePermissionDenied, "connector package is unavailable or changed")
		}
		return nil
	}
	action, allowed, err := PluginConnectionAction(ctx, db, request)
	if err != nil {
		return err
	}
	if !allowed {
		action, allowed, err = pluginExternalBrowserAction(ctx, db, request)
		if err != nil {
			return err
		}
	}
	if !allowed {
		action, allowed = pluginAction(request.Command.Kind)
	}
	if !allowed {
		return fault(protocol.ErrorCodePermissionDenied, "command is not exposed to plugin software actions")
	}
	declared := action == "" || slices.Contains(info.Extension.Actions, action)
	if request.Command.Kind == "list_plugin_models" {
		declared = declared || slices.Contains(info.Extension.Actions, protocol.PluginActionReadModels)
	}
	if !declared {
		return fault(protocol.ErrorCodePermissionDenied, "plugin does not declare this software action")
	}
	settings := scope.Surface.Kind == "settings"
	if settings && info.Extension.SettingsPage == nil {
		return fault(protocol.ErrorCodePermissionDenied, "plugin has no settings contribution")
	}
	if !info.Summary.Enabled && !(settings && pluginSettingsAccess(request.Command.Kind)) {
		return fault(protocol.ErrorCodeNotConfigured, "plugin is disabled")
	}
	switch request.Command.Kind {
	case "read_activity", "read_activity_catalog":
		if scope.Worktree != nil || scope.Session != nil || scope.Turn != nil || scope.Invocation != nil {
			return fault(protocol.ErrorCodePermissionDenied, "activity requires a Node-wide plugin scope")
		}
	case "read_usage":
		if scope.Turn != nil || scope.Invocation != nil || scope.Surface.Kind != "workspace" && !settings {
			return fault(protocol.ErrorCodePermissionDenied, "usage requires a controller plugin scope")
		}
	case "read_code_index_settings", "save_code_index_settings", "read_search_settings", "save_search_settings", "test_search_driver":
		if !scope.ControllerSettings() {
			return fault(protocol.ErrorCodePermissionDenied, "settings require a Node-wide controller settings scope")
		}
	case "read_media_settings", "list_media_models", "save_media_settings", "save_plugin_settings":
		if !settings {
			return fault(protocol.ErrorCodePermissionDenied, "command requires a plugin settings surface")
		}
	case "read_plugin_connection":
		if err := CheckPluginConnectionRequest(request, info); err != nil {
			return err
		}
	case "list_associated_sessions":
		if scope.Turn != nil || scope.Session != nil {
			return fault(protocol.ErrorCodePermissionDenied, "command is not exposed to plugin software actions")
		}
		association := request.Command.Data.(*protocol.CommandListAssociatedSessionsPayload).Association
		if err := validateSessionAssociation(association); err != nil {
			return err
		}
		if association.Namespace != scope.Package.Name {
			return fault(protocol.ErrorCodePermissionDenied, "session association belongs to another package")
		}
	}
	if action == protocol.PluginActionReadTerminals || action == protocol.PluginActionControlTerminals {
		if err := checkPluginTerminalRequest(ctx, db, request); err != nil {
			return err
		}
	}
	if scope.Turn != nil && (request.Command.Kind == "write_plugin_value" || request.Command.Kind == "write_indexed_plugin_value" || request.Command.Kind == "remove_plugin_value") || request.Command.Kind == "write_plugin_conversation_value" || request.Command.Kind == "remove_plugin_conversation_value" {
		current, err := ReadPlugin(ctx, db, scope.Package.Name)
		if err != nil {
			return err
		}
		if current == nil || !current.Summary.Enabled {
			return fault(protocol.ErrorCodeNotConfigured, "plugin is disabled")
		}
	}
	if err := CheckPluginPlanAction(ctx, db, request, action); err != nil {
		return err
	}
	var target struct {
		Package  *protocol.PluginReference `json:"package"`
		Worktree *protocol.WorktreeID      `json:"worktree"`
		Session  *protocol.SessionID       `json:"session"`
		Turn     *protocol.TurnID          `json:"turn"`
	}
	body, err := json.Marshal(request.Command.Data)
	if err != nil {
		return err
	}
	if len(body) > 0 && body[0] == '{' {
		if err := json.Unmarshal(body, &target); err != nil {
			return err
		}
	}
	// A command addressed by turn names no session, so its turn is confined
	// to the captured session, or to the captured turn for tool output.
	switch request.Command.Kind {
	case "read_tool_output":
		if target.Turn == nil || scope.Turn == nil || *scope.Turn != *target.Turn {
			return fault(protocol.ErrorCodePermissionDenied, "tool output read is outside the captured turn")
		}
	case "run_command", "stop_turn", "start_queued_turn", "edit_queued_turn", "remove_queued_turn", "withdraw_guidance", "send_queued_turn", "read_queued_turn":
		if scope.Session == nil {
			return fault(protocol.ErrorCodePermissionDenied, "plugin has no session scope")
		}
		if target.Turn == nil {
			return invalid("plugin command names no turn")
		}
		if request.Command.Kind == "run_command" && scope.Turn != nil && *scope.Turn != *target.Turn {
			return fault(protocol.ErrorCodePermissionDenied, "command is outside the captured turn")
		}
		run, err := visibleAgentRun(ctx, db, *scope.Session, *target.Turn)
		if err != nil {
			return err
		}
		if run.Origin != nil {
			return fault(protocol.ErrorCodePermissionDenied, "plugin turn belongs to another session")
		}
	}
	if target.Package != nil && *target.Package != scope.Package || target.Worktree != nil && !reflect.DeepEqual(target.Worktree, scope.Worktree) || target.Session != nil && !reflect.DeepEqual(target.Session, scope.Session) {
		return fault(protocol.ErrorCodePermissionDenied, "plugin action is outside the captured resource scope")
	}
	if scope.Session != nil {
		session, err := ReadSession(ctx, db, *scope.Session)
		if err != nil {
			return err
		}
		if scope.Worktree == nil || session.Worktree != *scope.Worktree {
			return fault(protocol.ErrorCodePermissionDenied, "plugin session belongs to another worktree")
		}
	}
	if request.Command.Kind == "read_plugin_conversation_value" || request.Command.Kind == "write_plugin_conversation_value" || request.Command.Kind == "remove_plugin_conversation_value" {
		required := protocol.PluginActionReadConversation
		if request.Command.Kind != "read_plugin_conversation_value" {
			required = protocol.PluginActionControlConversation
		}
		if scope.Session == nil || !slices.Contains(info.Extension.Actions, required) {
			return fault(protocol.ErrorCodePermissionDenied, "plugin conversation storage is outside its declared actions")
		}
	}
	return nil
}

func pluginAction(kind string) (protocol.PluginAction, bool) {
	switch kind {
	case "call_plugin", "read_plugin_callback", "cancel_plugin_call", "read_plugin_settings", "read_plugin_connection", "plugin_transaction":
		return "", true
	case "save_plugin_settings":
		return "", true
	case "read_plugin_value", "list_plugin_keys", "search_plugin_values", "read_plugin_conversation_value":
		return protocol.PluginActionReadStorage, true
	case "write_plugin_value", "write_indexed_plugin_value", "remove_plugin_value", "write_plugin_conversation_value", "remove_plugin_conversation_value":
		return protocol.PluginActionWriteStorage, true
	case "dispatch", "stop_dispatch_turn":
		return protocol.PluginActionDispatch, true
	case "publish_notification":
		return protocol.PluginActionNotify, true
	case "request_plugin_http", "request_provider_http", "execute_provider_http":
		return protocol.PluginActionHttp, true
	case "list_plugin_models", "generate_plugin_text":
		return protocol.PluginActionGenerateText, true
	case "resolve_plugin_model":
		return protocol.PluginActionReadModels, true
	case "search_models", "read_model_versions", "resolve_model", "refresh_local_model", "read_local_models":
		return protocol.PluginActionReadModels, true
	case "decide":
		return protocol.PluginActionRunDecisions, true
	case "register_local_model", "install_local_model", "delete_local_model":
		return protocol.PluginActionInstallModels, true
	case "read_installations", "dismiss_installation":
		return protocol.PluginActionInstallModels, true
	case "read_activity", "read_activity_catalog":
		return protocol.PluginActionReadActivity, true
	case "read_usage":
		return protocol.PluginActionReadUsage, true
	case "list_roles":
		return protocol.PluginActionReadRoles, true
	case "list_teams":
		return protocol.PluginActionReadTeams, true
	case "put_role", "remove_role", "set_session_roles":
		return protocol.PluginActionWriteRoles, true
	case "read_project_catalog":
		return protocol.PluginActionReadProjects, true
	case "read_worktree_catalog", "list_worktrees":
		return protocol.PluginActionReadWorktrees, true
	case "register_worktree", "create_worktree", "create_managed_worktree", "remove_worktree":
		return protocol.PluginActionWriteWorktrees, true
	case "list_directory", "read_file", "read_file_lines", "read_image", "read_notebook", "find_files", "download_file", "search_files", "read_office":
		return protocol.PluginActionReadFiles, true
	case "upload_file", "write_file", "edit_file", "edit_notebook", "delete_file", "move_file", "apply_changes", "finish_file_upload", "create_directory", "rename_entry", "copy_entry", "copy_entry_to", "move_entry_to", "trash_entry", "trash_file":
		return protocol.PluginActionWriteFiles, true
	case "inspect_git", "read_git_head", "read_git_log", "read_git_show", "read_git_blame", "list_git_changes", "read_git_change", "read_git_context", "read_git_diff", "list_git_branches", "resolve_git_revision", "read_git_commit", "read_git_stash", "read_git_output", "list_git_remote_tags":
		return protocol.PluginActionReadGit, true
	case "update_git_index", "create_git_commit", "create_git_branch", "rename_git_branch", "delete_git_branch", "switch_git_branch", "merge_git_branch", "run_git_action":
		return protocol.PluginActionWriteGit, true
	case "list_commands", "read_command", "read_command_page", "read_tool_output":
		return protocol.PluginActionReadCommands, true
	case "run_command", "stop_commands", "stop_command", "write_command":
		return protocol.PluginActionControlCommands, true
	case "read_session", "read_conversation", "read_conversation_runs", "read_conversation_run", "read_turn_capture", "read_turn", "read_tool_detail", "search_conversation", "list_conversation_assets", "list_associated_sessions", "read_queued_turn":
		return protocol.PluginActionReadConversation, true
	case "queue_turn", "submit_turn", "continue_turn", "set_session_config", "set_session_member_model", "set_queue_paused", "move_queued_turn", "stop_turn", "start_queued_turn", "edit_queued_turn", "remove_queued_turn", "withdraw_guidance", "send_queued_turn":
		return protocol.PluginActionControlConversation, true
	case "start_session":
		return protocol.PluginActionStartSessions, true
	case "fetch_web", "cancel_web_fetch", "search_web", "cancel_web_search":
		return protocol.PluginActionReadWeb, true
	case "search_code", "outline_code", "graph_code", "cancel_code_index_query":
		return protocol.PluginActionReadCodeIndex, true
	case "query_knowledge", "read_knowledge_citation", "list_knowledge_collections":
		return protocol.PluginActionReadKnowledge, true
	case "create_knowledge_collection", "update_knowledge_collection", "delete_knowledge_collection", "rebuild_knowledge_collection", "pause_knowledge_collection":
		return protocol.PluginActionWriteKnowledge, true
	case "read_search_settings":
		return protocol.PluginActionReadSearchSettings, true
	case "save_search_settings", "test_search_driver":
		return protocol.PluginActionSaveSearchSettings, true
	case "read_code_index_settings":
		return protocol.PluginActionReadCodeIndexSettings, true
	case "save_code_index_settings":
		return protocol.PluginActionSaveCodeIndexSettings, true
	case "read_media_settings", "list_media_models":
		return protocol.PluginActionReadMediaSettings, true
	case "save_media_settings":
		return protocol.PluginActionSaveMediaSettings, true
	case "read_computer_permissions":
		return protocol.PluginActionReadComputer, true
	case "request_computer_permission":
		return protocol.PluginActionControlComputer, true
	case "list_terminals", "list_terminal_tools", "inspect_terminal":
		return protocol.PluginActionReadTerminals, true
	case "create_terminal", "open_tool_terminal", "open_terminal", "close_terminal", "claim_terminal", "input_terminal", "resize_terminal", "set_terminal_appearance":
		return protocol.PluginActionControlTerminals, true
	case "read_hooks":
		return protocol.PluginActionReadCodeSettings, true
	case "save_hooks":
		return protocol.PluginActionSaveCodeSettings, true
	case "read_artifact", "list_artifacts", "list_artifact_revisions":
		return protocol.PluginActionReadArtifacts, true
	case "write_artifact":
		return protocol.PluginActionWriteArtifacts, true
	default:
		return "", false
	}
}

func pluginSettingsAccess(kind string) bool {
	switch kind {
	case "list_roles", "list_teams", "list_associated_sessions", "read_project_catalog", "put_role", "remove_role",
		"list_plugin_models", "search_models", "read_model_versions", "resolve_model", "refresh_local_model", "read_local_models",
		"decide", "register_local_model", "install_local_model", "read_installations", "dismiss_installation",
		"read_plugin_settings", "read_plugin_value", "list_plugin_keys", "search_plugin_values", "write_plugin_value",
		"write_indexed_plugin_value", "remove_plugin_value", "plugin_transaction", "save_plugin_settings",
		"read_code_index_settings", "save_code_index_settings", "read_search_settings", "save_search_settings", "test_search_driver",
		"read_media_settings", "list_media_models", "save_media_settings", "read_computer_permissions", "request_computer_permission":
		return true
	}
	return false
}

// checkPluginConnectorRequest reports whether the command is a connector
// command every package may use without a declared action: its own public
// connection state and its own browser login. The lists are filtered to the
// calling package where they are answered; connection management stays with
// the controller.
func checkPluginConnectorRequest(ctx context.Context, db *sql.Conn, request protocol.Request) (bool, error) {
	scope := request.Plugin
	denied := func() error {
		return fault(protocol.ErrorCodePermissionDenied, "connector belongs to another package or surface")
	}
	switch request.Command.Kind {
	case "list_connector_types", "list_connections":
		return true, nil
	// The login owner binds input and cancellation to its original caller.
	case "submit_connector_input", "cancel_connector_login":
		return true, nil
	case "start_connector_login":
		input, err := providerPayload[protocol.CommandStartConnectorLoginPayload](request.Command)
		if err != nil {
			return false, err
		}
		if input.Package != scope.Package || scope.Turn != nil || scope.Invocation != nil || input.Input.Registration || len(input.Input.Secrets) != 0 ||
			input.Method != protocol.PluginConnectorKindOauth2 && input.Method != protocol.PluginConnectorKindMcpOauth {
			return false, denied()
		}
		if _, _, err := ConnectorDeclaration(ctx, db, input.Package, input.Connector); err != nil {
			return false, err
		}
		if input.Connection != nil {
			record, err := ReadConnectorRecord(ctx, db, *input.Connection)
			if err != nil {
				return false, err
			}
			if record.Plugin != input.Package.Name || record.Connector != input.Connector {
				return false, denied()
			}
		}
		return true, nil
	case "inspect_connector_cli", "update_connection", "set_connection_enabled", "disconnect_connection", "remove_connection", "confirm_connector_consent", "probe_connection":
		return false, denied()
	}
	return false, nil
}
