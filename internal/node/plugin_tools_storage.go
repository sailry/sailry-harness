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

// storageOperation reports whether an operation reads or writes the declaring
// plugin's private values or records its progress. The Node binds these to the
// plugin scope of the turn instead of a workspace command.
func storageOperation(operation protocol.ToolOperation) bool {
	switch operation {
	case protocol.ToolOperationProgress, protocol.ToolOperationGetValue, protocol.ToolOperationListKeys,
		protocol.ToolOperationSetValue, protocol.ToolOperationDeleteValue:
		return true
	}
	return false
}

// storageTools binds the declared progress and private storage operations of a
// turn. Every storage call is dispatched under the declaring package, so the
// store confines it to that package's namespace, actions and quotas. Handler
// tools keep their script binding.
func (node *Node) storageTools(inv *store.AgentInvocation, catalog *operations.Catalog) ([]engine.Tool, error) {
	var tools []engine.Tool
	for _, declaration := range catalog.Tools {
		if !storageOperation(declaration.Operation) || declaration.Script || !slices.Contains(inv.Turn.Plugins, declaration.Owner) ||
			!declaration.Available(inv.Worktree.Project != nil, connectionBound(inv)) ||
			inv.Turn.Config.Resource != nil && inv.Turn.Config.Assistant == nil && len(declaration.Contexts) == 0 {
			continue
		}
		var schema map[string]any
		if err := json.Unmarshal(declaration.Parameters, &schema); err != nil {
			return nil, err
		}
		tool := engine.Tool{Definition: fantasy.FunctionTool{Name: declaration.Name, Description: declaration.Description, InputSchema: schema}, SchemaJSON: declaration.Parameters, ReadOnly: declaration.ReadOnly, Concurrent: declaration.Concurrent, Binding: &store.ApprovalBinding{Owner: declaration.Owner, Tool: declaration.Name, Operation: &declaration.Operation}}
		if declaration.Operation == protocol.ToolOperationProgress {
			// The manifest admits this operation only with the progress presentation.
			if declaration.Presentation != protocol.ToolPresentationProgress {
				continue
			}
			tool.Execute = func(_ context.Context, call canonical.ToolRequest) engine.ToolResult {
				var progress protocol.ConversationProgress
				if err := json.Unmarshal(call.Arguments, &progress); err != nil {
					return workspaceToolFault(connectionFault(protocol.ErrorCodeInvalidRequest, "invalid task progress arguments"))
				}
				if !store.ValidAgentProgress(progress) {
					return workspaceToolFault(connectionFault(protocol.ErrorCodeInvalidRequest, "invalid task progress"))
				}
				return engine.ToolResult{Value: map[string]any{"progress": progress}}
			}
			tools = append(tools, tool)
			continue
		}
		tool.Deferred, tool.Approval = !declaration.Initial, !declaration.ReadOnly
		scope := protocol.PluginContext{Turn: &inv.Run.Turn, Package: declaration.Owner, Worktree: &inv.Run.Worktree, Session: &inv.Session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
		var mu sync.Mutex
		calls := map[string]*protocol.Request{}
		tool.Prepare = func(_ context.Context, call canonical.ToolRequest) error {
			command, err := operations.Command(declaration.Operation, call.Arguments, inv.Run.Worktree, inv.Session.ID, inv.Run.Turn, inv.Worktree.Project, false)
			if err != nil {
				return err
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
			// The model receives the entry or page itself, without its envelope.
			return engine.ToolResult{Value: output.Data}
		}
		tools = append(tools, tool)
	}
	return tools, nil
}
