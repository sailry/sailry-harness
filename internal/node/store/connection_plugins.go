package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func connectionDenied(message string) error {
	return fault(protocol.ErrorCodePermissionDenied, message)
}

// PluginConnectionAction maps a database or SSH command to the software
// action its package must declare and checks that the captured plugin scope
// may reach the command's connection. It reports false for every command
// outside the two families, including terminals that are not SSH terminals.
func PluginConnectionAction(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.PluginAction, bool, error) {
	scope := request.Plugin
	if scope == nil {
		return "", false, nil
	}
	var input struct {
		Profile  json.RawMessage       `json:"profile"`
		Terminal *protocol.TerminalID  `json:"terminal"`
		ReadOnly bool                  `json:"read_only"`
		Transfer *protocol.SshTransfer `json:"transfer"`
	}
	if request.Command.Data != nil {
		body, err := json.Marshal(request.Command.Data)
		if err != nil {
			return "", false, err
		}
		if len(body) > 0 && body[0] == '{' {
			if err := json.Unmarshal(body, &input); err != nil {
				return "", false, err
			}
		}
	}
	// The saved profile is an identifier, or the whole profile for a save or test.
	identity := func() (string, error) {
		var id string
		if json.Unmarshal(input.Profile, &id) == nil {
			return id, nil
		}
		var profile struct {
			ID string `json:"id"`
		}
		if err := json.Unmarshal(input.Profile, &profile); err != nil {
			return "", invalid("connection profile is required")
		}
		return profile.ID, nil
	}
	database := func() (*protocol.ConnectionResource, error) {
		text, err := identity()
		if err != nil {
			return nil, err
		}
		id, err := protocol.ParseDatabaseID(text)
		if err != nil {
			return nil, invalid("connection profile is required")
		}
		resource := databaseResource(id)
		return &resource, nil
	}
	ssh := func() (*protocol.ConnectionResource, error) {
		text, err := identity()
		if err != nil {
			return nil, err
		}
		id, err := protocol.ParseSshID(text)
		if err != nil {
			return nil, invalid("connection profile is required")
		}
		resource := sshResource(id)
		return &resource, nil
	}
	var target *protocol.ConnectionResource
	var action protocol.PluginAction
	var err error
	management := false
	switch request.Command.Kind {
	case "list_databases":
		action = protocol.PluginActionReadDatabases
	case "list_ssh":
		action = protocol.PluginActionReadSsh
	case "save_database", "test_database", "remove_database":
		management, action = true, protocol.PluginActionManageDatabases
		target, err = database()
	case "browse_database", "check_database":
		action = protocol.PluginActionReadDatabases
		target, err = database()
	case "query_database":
		action = protocol.PluginActionControlDatabases
		if input.ReadOnly {
			action = protocol.PluginActionReadDatabases
		}
		target, err = database()
	case "cancel_database":
		// The worker checks the original active request's package scope before cancelling.
		action = protocol.PluginActionReadDatabases
	case "save_ssh", "remove_ssh", "trust_ssh", "install_host", "update_ssh_host":
		management, action = true, protocol.PluginActionManageSsh
		target, err = ssh()
	case "read_host_install":
		management, action = true, protocol.PluginActionManageSsh
	case "check_ssh", "list_ssh_terminals", "browse_ssh_directory", "download_ssh_file":
		action = protocol.PluginActionReadSsh
		target, err = ssh()
	case "run_ssh", "open_ssh_terminal", "modify_ssh_file", "finish_ssh_upload", "stage_ssh_upload":
		action = protocol.PluginActionControlSsh
		target, err = ssh()
	case "transfer_ssh":
		if input.Transfer == nil {
			return "", false, invalid("SSH transfer is required")
		}
		if scope.Worktree == nil && scope.Session == nil && scope.Turn == nil && scope.Invocation == nil {
			var registered bool
			if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM worktrees WHERE id=?1 AND project IS NOT NULL AND id NOT IN (SELECT worktree FROM removed_worktrees))", input.Transfer.Worktree.String()).Scan(&registered); err != nil {
				return "", false, err
			}
			if !registered {
				return "", false, connectionDenied("SSH transfer requires a registered project worktree")
			}
			return protocol.PluginActionManageSsh, true, nil
		}
		if scope.Worktree == nil || *scope.Worktree != input.Transfer.Worktree {
			return "", false, connectionDenied("SSH transfer is outside the captured worktree")
		}
		action = protocol.PluginActionControlSsh
		target, err = ssh()
	case "cancel_ssh":
		action = protocol.PluginActionControlSsh
	case "close_ssh_terminal", "open_terminal", "inspect_terminal", "claim_terminal", "input_terminal", "resize_terminal", "set_terminal_appearance":
		if input.Terminal == nil {
			return "", false, invalid("terminal is required")
		}
		terminal, err := ReadTerminal(ctx, db, *input.Terminal)
		if err != nil {
			return "", false, err
		}
		if terminal.Ssh == nil {
			if request.Command.Kind == "close_ssh_terminal" {
				return "", false, connectionDenied("terminal is not an SSH terminal")
			}
			return "", false, nil
		}
		resource := sshResource(*terminal.Ssh)
		target, action = &resource, protocol.PluginActionControlSsh
		if request.Command.Kind == "inspect_terminal" {
			action = protocol.PluginActionReadSsh
		}
	default:
		return "", false, nil
	}
	if err != nil {
		return "", false, err
	}
	creation := false
	if management && (scope.Turn != nil || scope.Invocation != nil) {
		if creation, err = authorizeConnectionManagement(ctx, db, request); err != nil {
			return "", false, err
		}
	}
	switch {
	case creation:
		bound, err := ConnectionBound(ctx, db, *scope.Turn)
		if err != nil {
			return "", false, err
		}
		if bound != nil {
			return "", false, connectionDenied("bound conversations cannot create another connection")
		}
	case target != nil:
		if err := CheckPluginConnectionTarget(ctx, db, *scope, *target); err != nil {
			return "", false, err
		}
	case scope.Turn != nil && scope.Session == nil:
		return "", false, connectionDenied("connection tool requires a captured session")
	}
	return action, true, nil
}

// CheckPluginConnectionTarget applies the core sharing decision to the
// resource a plugin scope captured. A Node-wide controller surface manages
// every connection.
func CheckPluginConnectionTarget(ctx context.Context, db *sql.Conn, scope protocol.PluginContext, target protocol.ConnectionResource) error {
	var bound *protocol.ConnectionResource
	var project *protocol.ProjectID
	switch {
	case scope.Session != nil:
		session, err := ReadSession(ctx, db, *scope.Session)
		if err != nil {
			return err
		}
		if scope.Worktree == nil || session.Worktree != *scope.Worktree {
			return connectionDenied("connection session belongs to another worktree")
		}
		bound, project = session.Config.Resource, session.Project
	case scope.Worktree != nil:
		var owner sql.NullString
		err := db.QueryRowContext(ctx, "SELECT project FROM worktrees WHERE id=?1 AND id NOT IN (SELECT worktree FROM removed_worktrees)", scope.Worktree.String()).Scan(&owner)
		if errors.Is(err, sql.ErrNoRows) {
			return connectionDenied("captured worktree is unavailable")
		}
		if err != nil {
			return err
		}
		if owner.Valid {
			parsed, err := protocol.ParseProjectID(owner.String)
			if err != nil {
				return err
			}
			project = &parsed
		}
	case scope.Turn == nil && scope.Invocation == nil:
		// Global connection management is an explicit controller surface.
		return nil
	default:
		return connectionDenied("connection operation requires a captured resource")
	}
	if scope.Turn != nil {
		run, err := ConnectionOwner(ctx, db, *scope.Turn)
		if err != nil {
			return err
		}
		if scope.Session == nil || run.Session != *scope.Session || scope.Worktree == nil || run.Worktree != *scope.Worktree {
			return connectionDenied("connection turn belongs to another resource")
		}
		created, err := ConnectionCreated(ctx, db, *scope.Turn, scope.Package, target)
		if err != nil || created {
			return err
		}
	}
	sharing, _, err := connectionSharing(ctx, db, target)
	if err != nil {
		return err
	}
	if !ConnectionPermits(bound, project, target, sharing) {
		return connectionDenied("connection is not available to this captured resource")
	}
	return nil
}

// FilterPluginDatabases keeps the profiles a plugin scope may reach.
func FilterPluginDatabases(ctx context.Context, db *sql.Conn, scope protocol.PluginContext, profiles []protocol.DatabaseProfile) ([]protocol.DatabaseProfile, error) {
	permitted := []protocol.DatabaseProfile{}
	for _, profile := range profiles {
		err := CheckPluginConnectionTarget(ctx, db, scope, databaseResource(profile.ID))
		var refused *protocol.Fault
		if err != nil && !errors.As(err, &refused) {
			return nil, err
		}
		if err == nil {
			permitted = append(permitted, profile)
		}
	}
	return permitted, nil
}

// FilterPluginSsh keeps the profiles a plugin scope may reach.
func FilterPluginSsh(ctx context.Context, db *sql.Conn, scope protocol.PluginContext, profiles []protocol.SshProfile) ([]protocol.SshProfile, error) {
	permitted := []protocol.SshProfile{}
	for _, profile := range profiles {
		err := CheckPluginConnectionTarget(ctx, db, scope, sshResource(profile.ID))
		var refused *protocol.Fault
		if err != nil && !errors.As(err, &refused) {
			return nil, err
		}
		if err == nil {
			permitted = append(permitted, profile)
		}
	}
	return permitted, nil
}

// authorizeConnectionManagement admits a turn's connection save only through
// its own claimed approval, and a host-key trust only through explicit consent
// recorded on an approved SSH operation. It reports whether the command
// creates a connection.
func authorizeConnectionManagement(ctx context.Context, db *sql.Conn, request protocol.Request) (bool, error) {
	scope := request.Plugin
	if scope.Turn == nil {
		return false, nil
	}
	if scope.Invocation != nil {
		return false, connectionDenied("connection management requires a captured tool")
	}
	var operation protocol.ToolOperation
	creation := false
	switch request.Command.Kind {
	case "save_database":
		operation = protocol.ToolOperationSaveDatabaseConnection
		input, valid := dispatchValue[protocol.CommandSaveDatabasePayload](request.Command.Data)
		creation = valid && input.ExpectedRevision == 0
	case "save_ssh":
		operation = protocol.ToolOperationSaveSshConnection
		input, valid := dispatchValue[protocol.CommandSaveSshPayload](request.Command.Data)
		creation = valid && input.ExpectedRevision == 0
	case "trust_ssh":
		return false, authorizeConnectionTrust(ctx, db, request)
	default:
		return false, connectionDenied("connection management requires a controller view")
	}
	var body, bound []byte
	err := db.QueryRowContext(ctx, "SELECT body,binding FROM agent_approvals WHERE turn=?1 AND request=?2 AND claimed=1", scope.Turn.String(), request.ID.String()).Scan(&body, &bound)
	if errors.Is(err, sql.ErrNoRows) {
		return false, connectionDenied("connection save has no claimed approval")
	}
	if err != nil {
		return false, err
	}
	var approval protocol.ConversationApproval
	var binding *ApprovalBinding
	if err := json.Unmarshal(body, &approval); err != nil {
		return false, err
	}
	if err := json.Unmarshal(bound, &binding); err != nil {
		return false, err
	}
	if approval.State != protocol.ConversationApprovalStateApproved || scope.Session == nil || approval.Session != *scope.Session ||
		binding == nil || binding.Owner != scope.Package || binding.Operation == nil || *binding.Operation != operation {
		return false, connectionDenied("connection save differs from its claimed approval")
	}
	return creation, nil
}

func authorizeConnectionTrust(ctx context.Context, db *sql.Conn, request protocol.Request) error {
	scope := request.Plugin
	input, valid := dispatchValue[protocol.CommandTrustSshPayload](request.Command.Data)
	if !valid {
		return invalid("SSH configuration command expected")
	}
	rows, err := db.QueryContext(ctx, "SELECT body,binding FROM agent_approvals WHERE turn=?1 AND claimed=1 AND json_extract(body,'$.state')='approved' AND json_type(body,'$.ssh')='object'", scope.Turn.String())
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
		var body, bound []byte
		if err := rows.Scan(&body, &bound); err != nil {
			return err
		}
		var approval protocol.ConversationApproval
		var binding *ApprovalBinding
		if err := json.Unmarshal(body, &approval); err != nil {
			return err
		}
		if err := json.Unmarshal(bound, &binding); err != nil {
			return err
		}
		if scope.Session != nil && approval.Session == *scope.Session && binding != nil && binding.Owner == scope.Package &&
			binding.Operation != nil && (*binding.Operation == protocol.ToolOperationRunSsh || *binding.Operation == protocol.ToolOperationTransferSsh) &&
			approval.Ssh != nil && approval.Ssh.Profile == input.Profile && approval.Ssh.ExpectedRevision == input.ExpectedRevision && approval.Ssh.Key == input.Key {
			return nil
		}
	}
	if err := rows.Err(); err != nil {
		return err
	}
	return connectionDenied("SSH host key requires its exact explicit approval")
}
