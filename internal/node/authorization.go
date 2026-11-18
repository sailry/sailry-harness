package node

import (
	"context"
	"database/sql"
	"reflect"
	"strings"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type accessPolicy uint8

const (
	accessResource accessPolicy = iota + 1
	accessCollection
	accessPersonal
	accessMachine
	accessDevice
	accessCreateProject
	accessManageProject
)

// Policies are an explicit protocol allowlist. A new command must choose its
// authorization boundary before it can reach any specialized dispatcher.
var commandPolicies = func() map[string]accessPolicy {
	values := map[string]accessPolicy{}
	add := func(policy accessPolicy, kinds string) {
		for _, kind := range strings.Fields(kinds) {
			if _, ok := values[kind]; ok {
				panic("duplicate command policy: " + kind)
			}
			values[kind] = policy
		}
	}
	add(accessCreateProject, `create_project register_project`)
	add(accessManageProject, `update_project remove_project`)
	add(accessPersonal, `call_host_handler`)
	add(accessCollection, `snapshot read_project_catalog read_resource_catalog read_activity_catalog list_associated_sessions read_usage`)
	add(accessMachine, `browse_files manage_files read_power_settings save_power_settings read_telemetry_settings save_telemetry_settings reset_telemetry_identity send_diagnostics report_diagnostic read_network_settings save_network_settings probe_network list_network_presets read_sandbox_settings save_sandbox_settings read_hooks save_hooks save_code_index_settings read_python_settings save_python_settings read_local_models read_local_model_settings save_local_model_settings register_local_model install_local_model refresh_local_model delete_local_model cancel_local_model list_backups read_backup_settings save_backup_settings export_backup restore_backup restore_backup_file upload_backup download_backup inspect_host read_host_metrics stop_host_process install_host read_host_install install_wsl_host read_wsl_host_install cancel_wsl_host_install list_wsl_distributions read_computer_permissions request_computer_permission decide`)
	add(accessPersonal, `identity configuration_follow list_providers list_credentials read_provider_key put_provider save_provider remove_provider discover_models resolve_model request_provider_http execute_provider_http begin_provider_login complete_provider_login cancel_provider_login read_model_catalog refresh_model_catalog read_model_scores read_model_versions list_channel_presets search_models list_roles put_role remove_role list_teams put_team remove_team read_team_settings save_team_settings read_compaction_settings save_compaction_settings read_media_settings save_media_settings list_media_models use_media list_ssh save_ssh remove_ssh check_ssh trust_ssh run_ssh browse_ssh_directory download_ssh_file modify_ssh_file transfer_ssh stage_ssh_upload finish_ssh_upload update_ssh_host list_databases save_database remove_database check_database test_database browse_database query_database list_connections list_connector_types probe_connection read_plugin_connection update_connection set_connection_enabled remove_connection disconnect_connection start_connector_login submit_connector_input confirm_connector_consent cancel_connector_login inspect_connector_cli list_plugins read_plugin read_plugin_version read_plugin_view read_plugin_callback cancel_plugin_call call_plugin generate_plugin_text request_plugin_http read_plugin_settings read_plugin_secret save_plugin_settings read_plugin_mcp save_plugin_mcp check_plugin_mcp set_plugin_mcp_allowed set_plugin_enabled remove_plugin install_plugin install_plugin_source install_catalog_plugin set_plugin_pinned install_plugin_upload install_bundled_plugin install_mcp upload_plugin inspect_plugin_source inspect_plugin_upload search_plugin_catalog read_catalog_plugin read_catalog_plugin_info check_plugin_update check_plugin_updates discover_skills discover_local_skills discover_registry_skill discover_skill_upload install_skill install_registry_skill install_skill_upload search_skill_catalog read_skill_document read_plugin_value write_plugin_value write_indexed_plugin_value remove_plugin_value list_plugin_keys search_plugin_values read_plugin_conversation_value write_plugin_conversation_value remove_plugin_conversation_value plugin_transaction dispatch stop_dispatch_turn list_plugin_models resolve_plugin_model list_knowledge_collections create_knowledge_collection update_knowledge_collection delete_knowledge_collection pause_knowledge_collection rebuild_knowledge_collection query_knowledge read_knowledge_citation read_plugin_memory read_search_settings save_search_settings test_search_driver read_code_index_settings read_terminal_settings save_terminal_settings list_shells`)
	add(accessDevice, `inspect_request count_feature synthesize_speech complete_browser read_installations dismiss_installation cancel_database cancel_ssh cancel_port cancel_file_transfer finish_file_upload finish_attachment_upload cancel_web_fetch cancel_web_search cancel_code_index_query`)
	add(accessResource, `resume_controller_login apply_changes apply_git_hunk apply_turn_hunk archive_sessions claim_terminal close_ssh_terminal close_terminal compact_context continue_turn copy_entry copy_entry_to create_directory create_git_branch create_git_commit create_managed_worktree create_session create_session_at create_terminal create_worktree delete_file delete_git_branch deliver_background discard_attachment dismiss_notifications dismiss_read_notifications download_attachment download_file download_image edit_file edit_notebook edit_queued_turn fetch_web find_files fork_conversation fork_conversation_at graph_code guide_queued_turn guide_turn import_session input_terminal inspect_git inspect_terminal list_artifact_revisions list_artifacts list_commands list_conversation_assets list_directory list_file_checkpoints list_git_branches list_git_changes list_git_remote_tags list_ssh_terminals list_terminal_tools list_terminals list_worktrees mark_notification_read mark_notifications_read merge_git_branch move_conversation move_entry_to move_file move_queued_turn open_port open_ssh_terminal open_terminal open_tool_terminal outline_code publish_notification queue_turn read_activity read_artifact read_attachment read_command read_command_page read_conversation read_conversation_run read_conversation_runs read_file read_file_lines read_git_blame read_git_change read_git_commit read_git_context read_git_diff read_git_head read_git_log read_git_output read_git_show read_git_stash read_image read_notebook read_office read_queued_turn read_session read_tool_detail read_tool_output read_turn read_turn_capture read_turn_diff read_turn_hunks read_worktree_catalog register_worktree remove_queued_turn remove_session remove_sessions remove_worktree rename_entry rename_git_branch rename_session replace_turn resize_terminal resolve_approval resolve_git_revision resolve_question restore_file_checkpoint rewind_conversation run_command run_git_action search_code search_conversation search_files search_web send_queued_turn set_queue_paused set_session_archived set_session_config set_session_member_model set_session_order set_session_read set_session_roles set_session_team set_terminal_appearance start_queued_turn start_session stop_command stop_commands stop_turn submit_task submit_turn switch_git_branch trash_entry trash_file update_git_index upload_attachment upload_file withdraw_guidance write_artifact write_command write_file use_browser use_external_browser`)
	return values
}()

func permissionDenied() error {
	return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "resource access is unavailable"}
}

type capturedDispatchKey struct{}

// dispatchCaptured retains the verified actor of admitted in-process callbacks.
// The original device remains the ledger caller after its trust is revoked.
func (node *Node) dispatchCaptured(ctx context.Context, caller protocol.NodeID, request protocol.Request) (link.Admission, error) {
	if _, ok := store.UserFromContext(ctx); !ok {
		return link.Admission{}, permissionDenied()
	}
	return node.Dispatch(context.WithValue(ctx, capturedDispatchKey{}, true), caller, request)
}

func (node *Node) authorizeCommand(ctx context.Context, caller protocol.NodeID, request protocol.Request) (context.Context, error) {
	var verified context.Context
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var user protocol.User
		var err error
		// Only the private callback entry point can retain an admitted principal.
		// Public peers always resolve current membership, including revocation.
		captured, _ := ctx.Value(capturedDispatchKey{}).(bool)
		if caller == node.ID() || captured {
			user, err = store.CurrentUser(ctx, db)
		} else {
			user, err = store.CallerUser(ctx, db, node.ID(), caller)
		}
		if err != nil {
			return err
		}
		verified = store.WithUser(ctx, user)
		if request.Command.Durable() {
			storage, err := store.CapturedUser(verified, db)
			if err != nil {
				return err
			}
			verified = store.WithCapturedUser(verified, storage)
		}
		return node.authorizeCommandDB(verified, db, caller, request)
	})
	return verified, nodeFault(err)
}

func (node *Node) authorizeCommandDB(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request) error {
	policy, ok := commandPolicies[request.Command.Kind]
	if !ok {
		return permissionDenied()
	}
	user, err := store.CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	if user.State != "active" {
		return permissionDenied()
	}
	switch policy {
	case accessMachine:
		if !user.Administrator {
			return permissionDenied()
		}
		// Hook scripts run across users on this execution Node.
		if request.Plugin != nil && (request.Command.Kind == "read_hooks" || request.Command.Kind == "save_hooks") {
			return permissionDenied()
		}
	case accessPersonal:
		// Personal stores apply their own canonical ownership checks. Resource
		// references inside a scoped plugin request remain an independent boundary.
	case accessCreateProject:
		var path string
		switch input := request.Command.Data.(type) {
		case *protocol.ProjectsDraft:
			path = input.Path
		case *protocol.CommandRegisterProjectPayload:
			path = input.Path
		}
		var id string
		err := db.QueryRowContext(ctx, "SELECT id FROM projects WHERE path=?1", path).Scan(&id)
		if err == nil {
			parsed, err := protocol.ParseProjectID(id)
			if err != nil {
				return err
			}
			if err = store.CheckProjectAccess(ctx, db, parsed, true); err != nil {
				return err
			}
		} else if err != sql.ErrNoRows {
			return err
		}
	case accessManageProject:
		switch input := request.Command.Data.(type) {
		case *protocol.CommandUpdateProjectPayload:
			return store.CheckProjectAccess(ctx, db, input.Expected.ID, true)
		case *protocol.CommandRemoveProjectPayload:
			return store.CheckProjectAccess(ctx, db, input.Expected.ID, true)
		default:
			return permissionDenied()
		}
	}
	if input, ok := request.Command.Data.(*protocol.CommandCreateSessionAtPayload); ok && input.Target != node.ID() {
		// Destination IDs belong to the remote host, which authenticates and checks
		// them. The forwarding service checks the local provider separately.
		if !user.Local {
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "only the local controller can forward configuration to another Node"}
		}
		return nil
	}
	if policy == accessResource {
		if err := authorizeResourceValue(ctx, db, reflect.ValueOf(request.Command.Data)); err != nil {
			return err
		}
	}
	if request.Plugin != nil {
		return authorizeResourceValue(ctx, db, reflect.ValueOf(request.Plugin))
	}
	return nil
}

// Walk typed resource identifiers, including optional and nested launches and
// every member of a batch. Text and numeric identifiers are never guessed.
func authorizeResourceValue(ctx context.Context, db *sql.Conn, v reflect.Value) error {
	if !v.IsValid() {
		return nil
	}
	for v.Kind() == reflect.Pointer || v.Kind() == reflect.Interface {
		if v.IsNil() {
			return nil
		}
		v = v.Elem()
	}
	if v.CanInterface() {
		switch id := v.Interface().(type) {
		case protocol.ProjectID:
			return store.CheckProjectAccess(ctx, db, id, false)
		case protocol.WorktreeID:
			return store.CheckWorktreeAccess(ctx, db, id)
		case protocol.SessionID:
			return store.CheckSessionAccess(ctx, db, id)
		case protocol.TerminalID:
			return store.CheckTerminalAccess(ctx, db, id)
		case protocol.TurnID:
			return store.CheckTurnAccess(ctx, db, id)
		case protocol.ArtifactID:
			return store.CheckArtifactAccess(ctx, db, id)
		case protocol.NotificationID:
			return store.CheckNotificationAccess(ctx, db, id)
		case protocol.ApprovalID:
			return store.CheckChildAccess(ctx, db, "approval", id.String())
		case protocol.QuestionID:
			return store.CheckChildAccess(ctx, db, "question", id.String())
		case protocol.CheckpointID:
			return store.CheckChildAccess(ctx, db, "checkpoint", id.String())
		case protocol.AttachmentID:
			return store.CheckChildAccess(ctx, db, "attachment", id.String())
		}
	}
	switch v.Kind() {
	case reflect.Struct:
		if v.Type() == reflect.TypeFor[protocol.ConversationInput]() {
			return nil
		}
		for i := 0; i < v.NumField(); i++ {
			if v.Type().Field(i).IsExported() {
				if err := authorizeResourceValue(ctx, db, v.Field(i)); err != nil {
					return err
				}
			}
		}
	case reflect.Slice, reflect.Array:
		for i := 0; i < v.Len(); i++ {
			if err := authorizeResourceValue(ctx, db, v.Index(i)); err != nil {
				return err
			}
		}
	case reflect.Map:
		iter := v.MapRange()
		for iter.Next() {
			if err := authorizeResourceValue(ctx, db, iter.Value()); err != nil {
				return err
			}
		}
	}
	return nil
}

func userFromContext(ctx context.Context) protocol.User {
	user, _ := store.UserFromContext(ctx)
	return user
}
