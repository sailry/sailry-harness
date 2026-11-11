package node

import (
	"context"
	"encoding/json"
	"errors"
	"sync"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// agentTool declares one Node-owned native tool. Its calls run as the
// controller that submitted the turn, without a plugin scope, through the
// commands the settings pages dispatch.
type agentTool struct {
	name        string
	description string
	parameters  map[string]any
	readOnly    bool
	// lost is reported when the Node drops the response.
	lost string
	// prepare validates the arguments and may read the current inventory.
	prepare func(ctx context.Context, arguments json.RawMessage) (protocol.Command, error)
	project func(ctx context.Context, output protocol.Output) (any, error)
}

// agentTools binds the plugin, MCP and skill managers and the role and team
// catalog tools of a turn. They are discovered through tool_search unless an
// assistant declaration names them.
func (node *Node) agentTools(inv *store.AgentInvocation) ([]engine.Tool, error) {
	var declared []agentTool
	restricted := inv.Turn.Config.Resource != nil
	if !restricted || inv.Turn.Config.Assistant != nil {
		declared = append(declared, node.skillManager(inv), node.mcpManager(inv), node.pluginManager(inv))
	}
	if !restricted && (inv.Session.Delegation == nil || *inv.Session.Delegation == nil) && inv.Worktree.Project != nil {
		declared = append(declared, node.catalogTools(inv)...)
	}
	tools := make([]engine.Tool, 0, len(declared))
	for _, declaration := range declared {
		schema, err := json.Marshal(declaration.parameters)
		if err != nil {
			return nil, err
		}
		tool := engine.Tool{Definition: fantasy.FunctionTool{Name: declaration.name, Description: declaration.description, InputSchema: declaration.parameters},
			SchemaJSON: schema, ReadOnly: declaration.readOnly, Deferred: true, Approval: !declaration.readOnly}
		var mu sync.Mutex
		calls := map[string]*protocol.Request{}
		tool.Prepare = func(ctx context.Context, call canonical.ToolRequest) error {
			command, err := declaration.prepare(ctx, call.Arguments)
			if err != nil {
				return err
			}
			request := protocol.NewRequest(node.ID(), command)
			mu.Lock()
			calls[call.ID] = &request
			mu.Unlock()
			return nil
		}
		// The approved call becomes the request, so a replayed effect is refused.
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
			output, err := node.agentRequest(ctx, inv, *request, declaration.lost)
			if err != nil {
				return workspaceToolFault(err)
			}
			value, err := declaration.project(ctx, output)
			if err != nil {
				return workspaceToolFault(err)
			}
			return engine.ToolResult{Value: value}
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

// agentRequest dispatches one native tool request as the turn's controller.
func (node *Node) agentRequest(ctx context.Context, inv *store.AgentInvocation, request protocol.Request, lost string) (protocol.Output, error) {
	accepted, err := node.Dispatch(ctx, inv.Caller, request)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case response, delivered := <-accepted.Completion:
		if !delivered {
			return protocol.Output{}, connectionFault(protocol.ErrorCodeUnavailable, lost)
		}
		if response.Fault != nil {
			return protocol.Output{}, response.Fault
		}
		return response.Output, nil
	case <-ctx.Done():
		return protocol.Output{}, connectionFault(protocol.ErrorCodeCancelled, "tool invocation was cancelled")
	}
}

// agentCommand dispatches a command a native tool issues while preparing or
// projecting a call.
func (node *Node) agentCommand(ctx context.Context, inv *store.AgentInvocation, command protocol.Command, lost string) (protocol.Output, error) {
	return node.agentRequest(ctx, inv, protocol.NewRequest(node.ID(), command), lost)
}

// agentPlugin reads one installed package, or reports false for another outcome.
func (node *Node) agentPlugin(ctx context.Context, inv *store.AgentInvocation, name, lost string) (protocol.PluginInfo, bool, error) {
	output, err := node.agentCommand(ctx, inv, protocol.Command{Kind: "read_plugin", Data: &protocol.CommandReadPluginPayload{Name: name}}, lost)
	if err != nil {
		return protocol.PluginInfo{}, false, err
	}
	info, found := dispatchPayload[protocol.PluginInfo](output.Data)
	return info, found, nil
}

// agentArguments decodes validated model arguments into a typed input.
func agentArguments(arguments json.RawMessage, target any) error {
	if err := json.Unmarshal(arguments, target); err != nil {
		return connectionFault(protocol.ErrorCodeInvalidRequest, "invalid tool arguments: "+err.Error())
	}
	return nil
}

// actionSchema merges per-action argument schemas into one object schema whose
// oneOf selects the fields each action accepts.
func actionSchema(actions []string, variants []map[string]any) map[string]any {
	properties := map[string]any{}
	names := make([]any, 0, len(actions))
	options := make([]any, 0, len(actions))
	for index, action := range actions {
		variant := variants[index]
		fields := variant["properties"].(map[string]any)
		for name, schema := range fields {
			properties[name] = schema
		}
		fields["action"] = map[string]any{"const": action}
		required, _ := variant["required"].([]any)
		variant["required"] = append(required, "action")
		names = append(names, action)
		options = append(options, variant)
	}
	properties["action"] = map[string]any{"type": "string", "enum": names}
	return map[string]any{"type": "object", "properties": properties, "required": []any{"action"}, "additionalProperties": false, "oneOf": options}
}

// objectSchema is a closed object schema.
func objectSchema(properties map[string]any, required ...any) map[string]any {
	schema := map[string]any{"type": "object", "additionalProperties": false, "properties": properties}
	if len(required) != 0 {
		schema["required"] = required
	}
	return schema
}
