package node

import (
	"context"
	"encoding/json"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (owner *pluginScriptTurn) bindNative(tool, bound engine.Tool, declaration protocol.ToolDeclaration, handler protocol.ToolHandler) engine.Tool {
	tool.ReadOnly, tool.Concurrent, tool.Approval, tool.Binding = bound.ReadOnly, bound.Concurrent, bound.Approval, bound.Binding
	tool.Delegation = bound.Delegation
	tool.ConfigureApproval, tool.Approved, tool.Close = bound.ConfigureApproval, bound.Approved, bound.Close
	var mu sync.Mutex
	calls := map[string]canonical.ToolRequest{}
	tool.Prepare = func(ctx context.Context, call canonical.ToolRequest) error {
		var input any
		if json.Unmarshal(call.Arguments, &input) != nil {
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid plugin tool arguments"}
		}
		prepared, err := owner.evaluate(ctx, handler.Name, declaration.Name, call.ID, input, true)
		if err != nil {
			return err
		}
		var result any
		if json.Unmarshal(prepared, &result) != nil {
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin tool returned invalid preparation"}
		}
		if pluginResultFailed(result) {
			object := result.(map[string]any)
			body, _ := json.Marshal(object["error"])
			var failure protocol.Fault
			if json.Unmarshal(body, &failure) == nil && failure.Code != "" && failure.Message != "" {
				return &failure
			}
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin tool preparation failed"}
		}
		call.Arguments = prepared
		if bound.Prepare != nil {
			if err := bound.Prepare(ctx, call); err != nil {
				return err
			}
		}
		mu.Lock()
		calls[call.ID] = call
		mu.Unlock()
		return nil
	}
	tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		mu.Lock()
		prepared, exists := calls[call.ID]
		delete(calls, call.ID)
		mu.Unlock()
		if !exists {
			return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin tool was not prepared"})
		}
		call = prepared
		output := bound.Execute(ctx, call)
		if handler.Result == nil || output.Failure != nil {
			return output
		}
		var arguments any
		_ = json.Unmarshal(call.Arguments, &arguments)
		formatted, err := owner.evaluate(ctx, *handler.Result, declaration.Name, call.ID, map[string]any{"arguments": arguments, "output": output.Value}, true)
		if err != nil {
			return output
		}
		if !output.ReturnedError && !output.Error {
			_ = json.Unmarshal(formatted, &output.Value)
		}
		return output
	}
	return tool
}
