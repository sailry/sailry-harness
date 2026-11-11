package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"sync"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	browserReadOperation    protocol.ToolOperation = "browser.read"
	browserControlOperation protocol.ToolOperation = "browser.control"
)

var browserToolSchema = json.RawMessage(`{"type":"object","properties":{"action":{"type":"string"},"arguments":{"type":"object"}},"required":["action","arguments"],"additionalProperties":false}`)

// browserTools binds the browser operations a turn's packages declare to the
// initiating controller's browser. The package's host script prepares each
// call and formats its result around these native tools.
func (node *Node) browserTools(ctx context.Context, inv *store.AgentInvocation) ([]engine.Tool, error) {
	var infos []protocol.PluginInfo
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		for _, reference := range inv.Turn.Plugins {
			info, err := store.ResolvePluginInfo(ctx, db, reference)
			if err != nil {
				return err
			}
			infos = append(infos, info)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	var schema map[string]any
	if err := json.Unmarshal(browserToolSchema, &schema); err != nil {
		return nil, err
	}
	var tools []engine.Tool
	for _, info := range infos {
		if info.Extension == nil || info.Extension.Host == nil {
			continue
		}
		for _, declaration := range info.Extension.Tools {
			if declaration.Handler == nil || declaration.Handler.Operation == nil {
				continue
			}
			operation := *declaration.Handler.Operation
			if operation != browserReadOperation && operation != browserControlOperation {
				continue
			}
			name := plugins.ToolAlias(info.Summary.Name, declaration.Name)
			tools = append(tools, node.browserTool(inv, plugins.PackageReference(info), name, operation, schema))
		}
	}
	return tools, nil
}

func (node *Node) browserTool(inv *store.AgentInvocation, owner protocol.PluginReference, name string, operation protocol.ToolOperation, schema map[string]any) engine.Tool {
	readOnly := operation == browserReadOperation
	var mu sync.Mutex
	calls := map[string]*protocol.Request{}
	tool := engine.Tool{Definition: fantasy.FunctionTool{Name: name, InputSchema: schema}, SchemaJSON: browserToolSchema, ReadOnly: readOnly, Approval: !readOnly,
		Binding: &store.ApprovalBinding{Owner: owner, Tool: name, Operation: &operation}}
	// A capture is a read; saving it in the project asks first.
	tool.Confirm = func(call canonical.ToolRequest) bool {
		mu.Lock()
		defer mu.Unlock()
		current := calls[call.ID]
		return current != nil && browserRequiresApproval(current.Command.Data.(*protocol.CommandUseBrowserPayload).Action)
	}
	tool.Prepare = func(_ context.Context, call canonical.ToolRequest) error {
		var input struct {
			Action    string                     `json:"action"`
			Arguments map[string]json.RawMessage `json:"arguments"`
		}
		if json.Unmarshal(call.Arguments, &input) != nil || input.Arguments == nil {
			return browserFault(protocol.ErrorCodeInvalidRequest, "invalid browser tool arguments")
		}
		input.Arguments["action"], _ = json.Marshal(input.Action)
		encoded, err := json.Marshal(input.Arguments)
		if err != nil {
			return browserFault(protocol.ErrorCodeInvalidRequest, "invalid browser tool arguments")
		}
		var action protocol.BrowserAction
		if json.Unmarshal(encoded, &action) != nil {
			return browserFault(protocol.ErrorCodeInvalidRequest, "invalid browser tool arguments")
		}
		if action.Kind != "screenshot" && browserRequiresApproval(action) == readOnly {
			return browserFault(protocol.ErrorCodePermissionDenied, "browser action does not match the declared operation")
		}
		if err := validateBrowserAction(action); err != nil {
			return err
		}
		if readOnly && browserRequiresApproval(action) && inv.Turn.Config.Mode == protocol.WorkModePlan {
			return browserFault(protocol.ErrorCodePermissionDenied, "planning turns cannot save browser captures")
		}
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "use_browser", Data: &protocol.CommandUseBrowserPayload{Session: inv.Session.ID, Worktree: inv.Run.Worktree, Action: action}})
		mu.Lock()
		calls[call.ID] = &request
		mu.Unlock()
		return nil
	}
	tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
		mu.Lock()
		current := calls[call.ID]
		mu.Unlock()
		if current == nil {
			return value, errors.New("native tool was not prepared")
		}
		return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, operation, current.Command, nil)
	}
	tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
		mu.Lock()
		defer mu.Unlock()
		if current := calls[call.ID]; current != nil {
			current.ID = request
		}
	}
	tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		mu.Lock()
		current := calls[call.ID]
		delete(calls, call.ID)
		mu.Unlock()
		if current == nil {
			return workspaceToolFault(errors.New("native tool was not prepared"))
		}
		// The call goes to the controller that started the turn, never through
		// this Node's own dispatch identity.
		output, err := node.useBrowser(ctx, inv.Caller, current.ID, *current.Command.Data.(*protocol.CommandUseBrowserPayload))
		if err != nil {
			return workspaceToolFault(err)
		}
		return engine.ToolResult{Value: map[string]any{"kind": output.Kind, "data": output.Data}}
	}
	return tool
}
