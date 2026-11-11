package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"sync"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// scopedTools binds the declared local model and role catalog operations of a
// turn. Each call is dispatched under the declaring package, so the store
// checks its declared software action, and a model removal is admitted only as
// the approved call that captured it. Handler tools wrap these bindings with
// their script preparation.
func (node *Node) scopedTools(inv *store.AgentInvocation, catalog *operations.Catalog) ([]engine.Tool, error) {
	var tools []engine.Tool
	for _, declaration := range catalog.Tools {
		if !operations.ScopedOperation(declaration.Operation) || !slices.Contains(inv.Turn.Plugins, declaration.Owner) ||
			!declaration.Available(inv.Worktree.Project != nil, connectionBound(inv)) ||
			inv.Turn.Config.Resource != nil && inv.Turn.Config.Assistant == nil && len(declaration.Contexts) == 0 {
			continue
		}
		var schema map[string]any
		if err := json.Unmarshal(declaration.Parameters, &schema); err != nil {
			return nil, err
		}
		tool := engine.Tool{Definition: fantasy.FunctionTool{Name: declaration.Name, Description: declaration.Description, InputSchema: schema}, SchemaJSON: declaration.Parameters,
			ReadOnly: declaration.ReadOnly, Concurrent: declaration.Concurrent, Deferred: !declaration.Initial, Approval: !declaration.ReadOnly,
			Binding: &store.ApprovalBinding{Owner: declaration.Owner, Tool: declaration.Name, Operation: &declaration.Operation}}
		scope := protocol.PluginContext{Turn: &inv.Run.Turn, Package: declaration.Owner, Worktree: &inv.Run.Worktree, Session: &inv.Session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
		var mu sync.Mutex
		calls := map[string]*protocol.Request{}
		tool.Prepare = func(_ context.Context, call canonical.ToolRequest) error {
			command, err := operations.ScopedCommand(declaration.Operation, call.Arguments, inv.Session.ID)
			if err != nil {
				return err
			}
			// An invalid definition is refused before its approval is requested.
			if save, saving := command.Data.(*protocol.CommandPutRolePayload); saving {
				if err := store.ValidateRole(save.Role); err != nil {
					return err
				}
			}
			request := protocol.NewRequest(node.ID(), command)
			request.Plugin = &scope
			mu.Lock()
			calls[call.ID] = &request
			mu.Unlock()
			return nil
		}
		tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
			mu.Lock()
			command := calls[call.ID].Command
			mu.Unlock()
			return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, declaration.Operation, command, nil)
		}
		tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
			mu.Lock()
			defer mu.Unlock()
			calls[call.ID].ID = request
		}
		tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
			mu.Lock()
			request := calls[call.ID]
			delete(calls, call.ID)
			mu.Unlock()
			if request == nil {
				return workspaceToolFault(errors.New("native tool was not prepared"))
			}
			output, err := node.connectionDispatch(ctx, *request, connectionFault(protocol.ErrorCodeCancelled, "plugin tool was cancelled"),
				connectionFault(protocol.ErrorCodeOutcomeUnknown, "plugin tool response is unavailable"))
			if err != nil {
				return workspaceToolFault(err)
			}
			value, err := operations.ScopedOutput(declaration.Operation, output)
			if err != nil {
				return workspaceToolFault(err)
			}
			return engine.ToolResult{Value: value}
		}
		tools = append(tools, tool)
	}
	return tools, nil
}
