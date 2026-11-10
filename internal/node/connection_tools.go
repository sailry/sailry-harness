package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"slices"
	"strings"
	"sync"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// connectionToolCall is one prepared connection tool call. trust is set when
// the SSH host key must be saved under the call's approval before it runs.
type connectionToolCall struct {
	request protocol.Request
	trust   *protocol.SshTrust
}

// connectionBound names the kind of connection resource a conversation is
// bound to, or nothing for an unbound conversation.
func connectionBound(inv *store.AgentInvocation) string {
	if inv.Turn.Config.Resource == nil {
		return ""
	}
	return inv.Turn.Config.Resource.Kind
}

// connectionTools binds the captured database and SSH operations of a turn.
// The connections a turn may reach are captured once here: the bound resource
// of a connection conversation, or the connections shared with a project
// conversation. The store repeats that decision for every call.
func (node *Node) connectionTools(ctx context.Context, inv *store.AgentInvocation, catalog *operations.Catalog) ([]engine.Tool, error) {
	var declarations []operations.Declaration
	for _, declaration := range catalog.Tools {
		if operations.ConnectionOperation(declaration.Operation) && slices.Contains(inv.Turn.Plugins, declaration.Owner) &&
			declaration.Available(inv.Worktree.Project != nil, connectionBound(inv)) &&
			(inv.Turn.Config.Resource == nil || inv.Turn.Config.Assistant != nil || len(declaration.Contexts) != 0) {
			declarations = append(declarations, declaration)
		}
	}
	if len(declarations) == 0 {
		return nil, nil
	}
	var captured []protocol.ConnectionResource
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		reachable, err := store.ReadConnectionCatalog(ctx, db, inv.Session.ID)
		captured = reachable.Resources()
		return err
	})
	if err != nil {
		return nil, err
	}
	var tools []engine.Tool
	for _, declaration := range declarations {
		var schema map[string]any
		if err := json.Unmarshal(declaration.Parameters, &schema); err != nil {
			return nil, err
		}
		scope := protocol.PluginContext{Turn: &inv.Run.Turn, Package: declaration.Owner, Worktree: &inv.Run.Worktree, Session: &inv.Session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
		var mu sync.Mutex
		calls := map[string]*connectionToolCall{}
		tool := engine.Tool{Definition: fantasy.FunctionTool{Name: declaration.Name, Description: declaration.Description, InputSchema: schema}, SchemaJSON: declaration.Parameters, ReadOnly: declaration.ReadOnly, Concurrent: declaration.Concurrent, Deferred: !declaration.Initial, Approval: !declaration.ReadOnly, Binding: &store.ApprovalBinding{Owner: declaration.Owner, Tool: declaration.Name, Operation: &declaration.Operation}}
		tool.Prepare = func(ctx context.Context, call canonical.ToolRequest) error {
			command, err := node.connectionCommand(ctx, inv, declaration, call.Arguments, captured)
			if err != nil {
				return err
			}
			current := &connectionToolCall{request: protocol.NewRequest(node.ID(), command)}
			current.request.Plugin = &scope
			if current.trust, err = node.connectionPreflight(ctx, inv, declaration.Owner, scope, command, captured); err != nil {
				return err
			}
			mu.Lock()
			calls[call.ID] = current
			mu.Unlock()
			return nil
		}
		tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
			mu.Lock()
			current := calls[call.ID]
			mu.Unlock()
			if current.trust != nil {
				// Host-key consent is always an explicit decision of this call.
				if err := store.ValidateConnectionTrust(ctx, db, inv.Run.Turn, tool.Binding, *current.trust); err != nil {
					return value, err
				}
				value.Ssh = current.trust
				value.State, value.Source = protocol.ConversationApprovalStatePending, protocol.ConversationApprovalSourceUser
				value.Choice, value.Prefix, value.CanRemember = nil, nil, false
			}
			return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, declaration.Operation, current.request.Command, nil)
		}
		tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
			mu.Lock()
			defer mu.Unlock()
			calls[call.ID].request.ID = request
		}
		tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
			mu.Lock()
			current := calls[call.ID]
			delete(calls, call.ID)
			mu.Unlock()
			if current == nil {
				return workspaceToolFault(errors.New("native tool was not prepared"))
			}
			if current.trust != nil {
				revision, err := node.connectionTrust(ctx, scope, *current.trust)
				if err != nil {
					return workspaceToolFault(err)
				}
				switch input := current.request.Command.Data.(type) {
				case *protocol.CommandRunSshPayload:
					input.ExpectedRevision = revision
				case *protocol.CommandTransferSshPayload:
					input.ExpectedRevision = revision
				}
			}
			stopped := connectionFault(protocol.ErrorCodeCancelled, "plugin tool was cancelled")
			if connectionCancel(current.request.Command.Kind) != "" {
				stopped = connectionFault(protocol.ErrorCodeOutcomeUnknown, "connection operation was cancelled; inspect its outcome before retrying")
			}
			output, err := node.connectionDispatch(ctx, current.request, stopped, connectionFault(protocol.ErrorCodeOutcomeUnknown, "connection operation outcome is unavailable"))
			if err != nil {
				return workspaceToolFault(err)
			}
			value, err := node.connectionResult(ctx, inv, declaration.Owner, output, captured)
			if err != nil {
				return workspaceToolFault(err)
			}
			return engine.ToolResult{Value: value}
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func connectionFault(code protocol.ErrorCode, message string) *protocol.Fault {
	return &protocol.Fault{Code: code, Message: message}
}

// connectionCommand translates model arguments into the protocol command the
// Node workers execute, after the store checks the turn may reach its target.
func (node *Node) connectionCommand(ctx context.Context, inv *store.AgentInvocation, declaration operations.Declaration, arguments json.RawMessage, captured []protocol.ConnectionResource) (protocol.Command, error) {
	input, err := operations.ConnectionCall(declaration.Operation, arguments)
	if err != nil {
		return protocol.Command{}, err
	}
	action := store.ConnectionAction{Database: input.Database, ReadOnly: input.ReadOnly, Sql: input.Sql, RowLimit: input.RowLimit, Command: input.Command,
		TimeoutMs: &input.TimeoutMs, Path: input.Path, RemotePath: input.RemotePath, Direction: input.Direction}
	switch declaration.Operation {
	case protocol.ToolOperationListDatabaseConnections:
		action.Kind = store.ConnectionListDatabases
	case protocol.ToolOperationListSshConnections:
		action.Kind = store.ConnectionListSsh
	case protocol.ToolOperationSaveDatabaseConnection:
		action.Kind = store.ConnectionSaveDatabase
		if err := json.Unmarshal(input.Save, &action.DatabaseInput); err != nil {
			return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "invalid connection operation: "+err.Error())
		}
	case protocol.ToolOperationSaveSshConnection:
		action.Kind = store.ConnectionSaveSsh
		if err := json.Unmarshal(input.Save, &action.SshInput); err != nil {
			return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "invalid SSH connection operation")
		}
	case protocol.ToolOperationBrowseDatabase:
		action.Kind = store.ConnectionBrowse
	case protocol.ToolOperationQueryDatabase, protocol.ToolOperationExecuteDatabase:
		action.Kind = store.ConnectionQuery
	case protocol.ToolOperationRunSsh:
		action.Kind = store.ConnectionRun
	case protocol.ToolOperationTransferSsh:
		action.Kind = store.ConnectionTransfer
	}
	var command protocol.Command
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		command, err = store.ConnectionCommand(ctx, db, inv.Run.Turn, declaration.Owner, input.Target, action, captured)
		return err
	})
	return command, err
}

// connectionCancel names the command that stops a running connection effect.
func connectionCancel(kind string) string {
	switch kind {
	case "browse_database", "query_database":
		return "cancel_database"
	case "check_ssh", "run_ssh", "transfer_ssh":
		return "cancel_ssh"
	}
	return ""
}

// connectionDispatch runs one captured request on this Node. A stopped turn
// cancels the running effect and reports stopped; a completion that never
// arrives reports lost.
func (node *Node) connectionDispatch(ctx context.Context, request protocol.Request, stopped, lost *protocol.Fault) (protocol.Output, error) {
	// A turn-scoped plugin request is admitted only from the execution Node.
	accepted, err := node.Dispatch(ctx, node.ID(), request)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case response, delivered := <-accepted.Completion:
		if !delivered {
			return protocol.Output{}, lost
		}
		if response.Fault != nil {
			return protocol.Output{}, response.Fault
		}
		return response.Output, nil
	case <-ctx.Done():
		if kind := connectionCancel(request.Command.Kind); kind != "" {
			// The effect is tracked by request alone; a cancel carries no scope.
			var input any = &protocol.CommandCancelSshPayload{Request: request.ID}
			if kind == "cancel_database" {
				input = &protocol.CommandCancelDatabasePayload{Request: request.ID}
			}
			if cancelled, err := node.Dispatch(context.WithoutCancel(ctx), node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: kind, Data: input})); err == nil {
				<-cancelled.Completion
			}
		}
		return protocol.Output{}, stopped
	}
}

// connectionPreflight inspects the host key before an SSH run or transfer asks
// for approval. It returns the consent the approval must present, or nil when
// the saved key already matches.
func (node *Node) connectionPreflight(ctx context.Context, inv *store.AgentInvocation, owner protocol.PluginReference, scope protocol.PluginContext, command protocol.Command, captured []protocol.ConnectionResource) (*protocol.SshTrust, error) {
	inspection := store.ConnectionInspection{}
	switch input := command.Data.(type) {
	case *protocol.CommandRunSshPayload:
		inspection.Profile, inspection.ExpectedRevision = input.Profile, input.ExpectedRevision
	case *protocol.CommandTransferSshPayload:
		inspection.Profile, inspection.ExpectedRevision = input.Profile, input.ExpectedRevision
	default:
		return nil, nil
	}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "check_ssh", Data: &protocol.CommandCheckSshPayload{Profile: inspection.Profile, ExpectedRevision: inspection.ExpectedRevision}})
	request.Plugin = &scope
	unexpected := connectionFault(protocol.ErrorCodeInternal, "SSH host key inspection returned an unexpected result")
	output, err := node.connectionDispatch(ctx, request, connectionFault(protocol.ErrorCodeCancelled, "SSH host key inspection was cancelled"), unexpected)
	if err != nil {
		return nil, err
	}
	outcome, valid := dispatchPayload[protocol.SshOutcome](output.Data)
	if !valid || output.Kind != "ssh_outcome" {
		return nil, unexpected
	}
	if outcome.Kind == "connected" {
		return nil, nil
	}
	required, valid := dispatchPayload[protocol.SshOutcomeHostKeyRequiredPayload](outcome.Data)
	if !valid || outcome.Kind != "host_key_required" {
		return nil, unexpected
	}
	inspection.Key, inspection.Changed = required.Key, required.Changed
	var trust protocol.SshTrust
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		trust, err = store.ConnectionTrust(ctx, db, inv.Run.Turn, owner, inspection, captured)
		return err
	})
	if err != nil {
		return nil, err
	}
	return &trust, nil
}

// connectionTrust saves the approved host key and returns the profile revision
// the approved operation must then expect.
func (node *Node) connectionTrust(ctx context.Context, scope protocol.PluginContext, trust protocol.SshTrust) (uint64, error) {
	if trust.ExpectedRevision == math.MaxUint64 {
		return 0, connectionFault(protocol.ErrorCodeInvalidRequest, "SSH revision exhausted")
	}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "trust_ssh", Data: &protocol.CommandTrustSshPayload{Profile: trust.Profile, ExpectedRevision: trust.ExpectedRevision, Key: trust.Key}})
	request.Plugin = &scope
	lost := connectionFault(protocol.ErrorCodeOutcomeUnknown, "SSH host key save outcome is unavailable")
	output, err := node.connectionDispatch(ctx, request, lost, lost)
	if err != nil {
		return 0, err
	}
	profile, valid := dispatchPayload[protocol.SshProfile](output.Data)
	if !valid || output.Kind != "ssh_profile" {
		return 0, connectionFault(protocol.ErrorCodeInternal, "SSH host key save returned an unexpected result")
	}
	if profile.ID != trust.Profile || profile.Revision != trust.ExpectedRevision+1 || profile.Host != trust.Host || profile.Port != trust.Port ||
		profile.HostKey == nil || *profile.HostKey != trust.Key {
		return 0, connectionFault(protocol.ErrorCodeConflict, "SSH host key save differs from approval")
	}
	return profile.Revision, nil
}

// connectionResult shapes a completed connection command for the model.
// Catalog, query, run and transfer outcomes are returned as the Node produced
// them. Profile lists keep only the connections the turn may reach, and every
// database profile reports whether its password is still missing. Profiles
// never carry a password, key or passphrase.
func (node *Node) connectionResult(ctx context.Context, inv *store.AgentInvocation, owner protocol.PluginReference, output protocol.Output, captured []protocol.ConnectionResource) (any, error) {
	var targets []protocol.ConnectionResource
	var profiles []any
	list := false
	switch output.Kind {
	case "database_profiles":
		list = true
		values, _ := dispatchPayload[[]protocol.DatabaseProfile](output.Data)
		for _, profile := range values {
			targets, profiles = append(targets, protocol.ConnectionResource{Kind: "database", Data: profile.ID}), append(profiles, profile)
		}
	case "ssh_profiles":
		list = true
		values, _ := dispatchPayload[[]protocol.SshProfile](output.Data)
		for _, profile := range values {
			targets, profiles = append(targets, protocol.ConnectionResource{Kind: "ssh", Data: profile.ID}), append(profiles, profile)
		}
	case "database_profile":
		profile, valid := dispatchPayload[protocol.DatabaseProfile](output.Data)
		if !valid {
			return nil, connectionFault(protocol.ErrorCodeInvalidRequest, "connection result has no profile")
		}
		targets, profiles = append(targets, protocol.ConnectionResource{Kind: "database", Data: profile.ID}), append(profiles, profile)
	default:
		return output, nil
	}
	var statuses []store.ConnectionStatus
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		statuses, err = store.ConnectionStatuses(ctx, db, inv.Run.Turn, owner, targets, captured)
		return err
	})
	if err != nil {
		return nil, err
	}
	reachable := []any{}
	for index, profile := range profiles {
		// Statuses keep request order and omit targets outside the turn.
		position := slices.IndexFunc(statuses, func(status store.ConnectionStatus) bool {
			return status.Target.Kind == targets[index].Kind && status.Target.Data == targets[index].Data
		})
		if position < 0 {
			continue
		}
		if targets[index].Kind != "database" {
			reachable = append(reachable, profile)
			continue
		}
		body, err := json.Marshal(profile)
		if err != nil {
			return nil, err
		}
		var fields map[string]any
		if err := json.Unmarshal(body, &fields); err != nil {
			return nil, err
		}
		fields["credentials_required"] = statuses[position].MissingPassword
		reachable = append(reachable, fields)
	}
	if list {
		return map[string]any{"kind": output.Kind, "data": reachable}, nil
	}
	if len(reachable) == 0 {
		return nil, connectionFault(protocol.ErrorCodePermissionDenied, "connection result is outside the captured turn")
	}
	return map[string]any{"kind": output.Kind, "data": reachable[0]}, nil
}

// connectionTurnInput is the `connections` value a package's turn initializer
// receives: the bound resource and the public summaries of the connections the
// turn may reach, limited to the kinds the package's selected operations use.
func (node *Node) connectionTurnInput(ctx context.Context, inv *store.AgentInvocation, selected []protocol.ToolOperation) (map[string]any, error) {
	databases, ssh := []any{}, []any{}
	var reachable store.ConnectionCatalog
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		reachable, err = store.ReadConnectionCatalog(ctx, db, inv.Session.ID)
		return err
	})
	if err != nil {
		return nil, err
	}
	uses := func(prefix string) bool {
		return slices.ContainsFunc(selected, func(operation protocol.ToolOperation) bool {
			return operations.ConnectionOperation(operation) && strings.HasPrefix(string(operation), prefix)
		})
	}
	if uses("databases.") {
		for _, profile := range reachable.Databases {
			body, err := json.Marshal(profile.Connection)
			if err != nil {
				return nil, err
			}
			target := struct {
				Engine   protocol.DatabaseEngine `json:"engine"`
				Database string                  `json:"database"`
			}{}
			if err := json.Unmarshal(body, &target); err != nil {
				return nil, err
			}
			if profile.Connection.Kind == "sqlite" {
				target.Engine, target.Database = protocol.DatabaseEngineSqlite, "main"
			}
			databases = append(databases, map[string]any{"id": profile.ID, "name": profile.Name, "engine": target.Engine, "database": target.Database, "read_only": profile.ReadOnly})
		}
	}
	if uses("ssh.") {
		for _, profile := range reachable.Ssh {
			ssh = append(ssh, map[string]any{"id": profile.ID, "name": profile.Name})
		}
	}
	return map[string]any{"bound": inv.Turn.Config.Resource, "databases": databases, "ssh": ssh}, nil
}
