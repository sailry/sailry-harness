package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"slices"
	"strings"
	"sync"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginScriptTurn struct {
	node       *Node
	invocation *store.AgentInvocation
	info       protocol.PluginInfo
	bundle     plugins.ScriptBundle
	scope      protocol.PluginContext
	mu         sync.Mutex
	state      json.RawMessage
}

func (owner *pluginScriptTurn) evaluate(ctx context.Context, handler, tool, call string, input any, readOnly bool) (json.RawMessage, error) {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	identityTool := tool
	if !strings.HasPrefix(tool, "turn.") {
		identityTool = plugins.ToolAlias(owner.info.Summary.Name, tool)
	}
	identity := plugins.ToolCallIdentity(owner.invocation.Run.Turn, owner.scope.Package, identityTool, call)
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	scope := owner.scope
	execution := &plugins.ScriptExecution{CallID: &identity, Mode: owner.invocation.Turn.Config.Mode, ReadOnly: readOnly, Turn: owner.state}
	result, err := plugins.RunTool(operation, owner.bundle, handler, input, plugins.ScriptEnvironment{
		Context: scope, Target: owner.node.ID(), Deadline: time.Now().Add(plugins.ScriptCallTimeout),
	}, owner.node.dispatchCallbackResult, execution)
	if err == nil && execution.StagedTurn != nil {
		owner.state = slices.Clone(execution.StagedTurn)
	}
	if err == nil {
		result = plugins.StampCards(result, owner.node.ID(), scope, owner.info, identity)
	}
	return result, err
}

func (node *Node) scriptTools(ctx context.Context, invocation *store.AgentInvocation, info protocol.PluginInfo, native map[string]engine.Tool) ([]engine.Tool, error) {
	host := info.Extension.Host
	declarations := []protocol.ToolDeclaration{}
	names := []string{}
	for _, declaration := range info.Extension.Tools {
		if declaration.Handler != nil && availablePluginTool(invocation, declaration) && selectedPluginTool(invocation, info, "", declaration.Name) {
			if operation := declaration.Handler.Operation; operation != nil && (*operation == protocol.ToolOperationDelegateAgent || *operation == protocol.ToolOperationGuideAgent) &&
				(invocation.Session.Delegation != nil || invocation.Turn.Config.Resource != nil || invocation.Turn.Roles.Execution != protocol.ConversationExecutionTeam) {
				continue
			}
			// The media tools are the Node's own: a package's declaration of a
			// media operation binds nothing. A managed browser tool exists
			// only when this Node can run that browser.
			if operation := declaration.Handler.Operation; operation != nil {
				if _, media := mediaOperationKind(*operation); media {
					continue
				}
				if *operation == protocol.ToolOperationReadExternalBrowser || *operation == protocol.ToolOperationControlExternalBrowser {
					if _, bound := native[plugins.ToolAlias(info.Summary.Name, declaration.Name)]; !bound {
						continue
					}
				}
			}
			declarations = append(declarations, declaration)
			names = append(names, declaration.Name)
		}
	}
	if len(declarations) == 0 {
		return nil, nil
	}
	files, err := node.plugins.manager.ReadBundle(ctx, plugins.PackageReference(info), host.Entry, host.Resources)
	if err != nil {
		return nil, err
	}
	owner := &pluginScriptTurn{node: node, invocation: invocation, info: info, bundle: plugins.ScriptBundle{Entry: host.Entry, Files: files},
		scope: protocol.PluginContext{Turn: &invocation.Run.Turn, Package: plugins.PackageReference(info), Worktree: &invocation.Run.Worktree,
			Session: &invocation.Session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}, state: json.RawMessage("null")}
	instruction := ""
	if info.Extension.Instructions != nil {
		instruction = *info.Extension.Instructions
	}
	overrides := map[string]json.RawMessage{}
	var contextParts []string
	if host.Turn != nil {
		input := pluginTurnInput(invocation, info, names)
		var selected []protocol.ToolOperation
		for _, declaration := range info.Extension.Tools {
			if slices.Contains(names, declaration.Name) && declaration.Handler != nil && declaration.Handler.Operation != nil {
				selected = append(selected, *declaration.Handler.Operation)
			}
		}
		connections, err := node.connectionTurnInput(ctx, invocation, selected)
		if err != nil {
			return nil, err
		}
		input["connections"] = connections
		body, err := owner.evaluate(ctx, host.Turn.Initialize, "turn.initialize", host.Turn.Initialize, input, false)
		if err != nil {
			return nil, err
		}
		var selection struct {
			Instruction string                     `json:"instruction"`
			Parts       []string                   `json:"parts"`
			Tools       []string                   `json:"tools"`
			Parameters  map[string]json.RawMessage `json:"parameters"`
		}
		decoder := json.NewDecoder(bytes.NewReader(body))
		decoder.DisallowUnknownFields()
		if decoder.Decode(&selection) != nil || selection.Tools == nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid plugin turn selection"}
		}
		for _, name := range selection.Tools {
			if !slices.Contains(names, name) {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin turn selected an unavailable tool"}
			}
		}
		for name, schema := range selection.Parameters {
			var document map[string]any
			if !slices.Contains(selection.Tools, name) || len(schema) > plugins.ScriptMaxDataBytes || json.Unmarshal(schema, &document) != nil || document["type"] != "object" {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin turn selected an invalid tool schema"}
			}
		}
		combined := 0
		if len(selection.Parts) > 8 {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin turn returned too many context parts"}
		}
		for _, part := range selection.Parts {
			combined += len(part)
			if strings.TrimSpace(part) == "" || combined > 32*1024 {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin turn returned invalid context parts"}
			}
			if !slices.Contains(contextParts, part) {
				contextParts = append(contextParts, part)
			}
		}
		if selection.Instruction != "" {
			instruction += "\n\n" + selection.Instruction
		}
		declarations = slices.DeleteFunc(declarations, func(declaration protocol.ToolDeclaration) bool {
			return !slices.Contains(selection.Tools, declaration.Name)
		})
		overrides = selection.Parameters
	}
	var tools []engine.Tool
	for _, declaration := range declarations {
		handler := *declaration.Handler
		if schema, exists := overrides[declaration.Name]; exists {
			handler.Parameters = schema
		}
		var schema map[string]any
		if json.Unmarshal(handler.Parameters, &schema) != nil || schema["type"] != "object" {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin tool parameters are invalid"}
		}
		description := ""
		if declaration.Description != nil {
			description = *declaration.Description
		}
		readOnly := pluginHandlerReadOnly(handler, info.Extension.Actions)
		tool := engine.Tool{Definition: fantasy.FunctionTool{Name: plugins.ToolAlias(info.Summary.Name, declaration.Name), Description: description, InputSchema: schema},
			SchemaJSON: handler.Parameters, ReadOnly: readOnly, Deferred: !declaration.Initial, Approval: !readOnly,
			Binding:     &store.ApprovalBinding{Owner: owner.scope.Package, Tool: plugins.ToolAlias(info.Summary.Name, declaration.Name), Operation: handler.Operation},
			Instruction: strings.TrimSpace(instruction)}
		tool.Hosted = handler.Operation == nil && handler.Flow == nil
		if tool.Hosted {
			// A plain handler acts only through its package's own grants, each
			// checked when the script dispatches, so the call itself asks for
			// nothing. A planning turn still refuses one that is not read-only.
			tool.Approval = false
		} else if pluginStorageHandler(handler) {
			// Writes confined to the package's own values are authorized by
			// the storage scope.
			tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, _ canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
				return store.ConfigureWorkspaceApproval(ctx, db, invocation.Run.Turn, value, protocol.ToolOperationStorageTransaction, protocol.Command{}, nil)
			}
		}
		if handler.Flow != nil {
			tool.Flow = handler.Flow.Operations
		}
		if handler.Operation != nil {
			_, tool.Concurrent = operations.Policy(*handler.Operation)
		} else if handler.Flow != nil && len(handler.Flow.Operations) != 0 {
			tool.Concurrent = true
			for _, operation := range handler.Flow.Operations {
				_, concurrent := operations.Policy(operation)
				tool.Concurrent = tool.Concurrent && concurrent
			}
		}
		tool.ContextParts = contextParts
		if host.Turn != nil && host.Turn.BeforeModel != nil {
			tool.BeforeModel = owner.beforeModel(*host.Turn.BeforeModel)
		}
		if handler.Operation != nil {
			// A package tool bound to agents.delegate or agents.guide runs
			// the core tool with the arguments its script prepares.
			if *handler.Operation == protocol.ToolOperationDelegateAgent || *handler.Operation == protocol.ToolOperationGuideAgent {
				tool.Guidance = *handler.Operation == protocol.ToolOperationGuideAgent
				tool.BindDelegation = func(bound engine.Tool) engine.Tool { return owner.bindNative(tool, bound, declaration, handler) }
				tools = append(tools, tool)
				continue
			}
			if bound, exists := native[tool.Definition.Name]; exists {
				tools = append(tools, owner.bindNative(tool, bound, declaration, handler))
				continue
			}
		}
		tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
			var input any
			if json.Unmarshal(call.Arguments, &input) != nil {
				return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid plugin tool arguments"})
			}
			if handler.Flow != nil {
				return owner.flow(ctx, declaration.Name, handler, call, input, readOnly)
			}
			body, err := owner.evaluate(ctx, handler.Name, declaration.Name, call.ID, input, readOnly)
			if err != nil {
				return workspaceToolFault(err)
			}
			var prepared any
			if json.Unmarshal(body, &prepared) != nil {
				return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin tool returned invalid JSON"})
			}
			if pluginResultFailed(prepared) || handler.Operation == nil {
				return engine.ToolResult{Value: prepared, ReturnedError: pluginResultFailed(prepared)}
			}
			output := owner.operation(ctx, declaration.Name, call, 0, *handler.Operation, body)
			if handler.Result == nil || output.Failure != nil {
				return output
			}
			formatted, err := owner.evaluate(ctx, *handler.Result, declaration.Name, call.ID, map[string]any{"arguments": input, "output": output.Value}, readOnly)
			if err != nil {
				return output
			}
			if !output.ReturnedError {
				_ = json.Unmarshal(formatted, &output.Value)
			}
			return output
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func pluginTurnInput(invocation *store.AgentInvocation, info protocol.PluginInfo, names []string) map[string]any {
	input := map[string]any{"tools": names, "project": invocation.Worktree.Project, "resources": invocation.Turn.Config.Assistant == nil,
		"roles": []any{}, "options": invocation.Turn.Config.Options[info.Summary.Name], "session_revision": nil, "automatic": false,
		"mode": invocation.Turn.Config.Mode, "config": nil, "execution": nil, "team": nil, "models": nil, "message": nil,
		"connections": map[string]any{"bound": invocation.Turn.Config.Resource, "databases": []any{}, "ssh": []any{}}}
	aliases := []string{}
	for _, name := range names {
		aliases = append(aliases, plugins.ToolAlias(info.Summary.Name, name))
	}
	input["names"] = aliases
	if slices.Contains(info.Extension.Actions, protocol.PluginActionReadConversation) {
		input["config"], input["execution"], input["models"] = invocation.Turn.Config, invocation.Turn.Roles.Execution, invocation.Turn.Roles.Models
	}
	if slices.Contains(info.Extension.Actions, protocol.PluginActionReadMessage) && invocation.Session.Delegation == nil {
		message := invocation.Message.Text
		if len(message) > 4096 {
			for len(message) > 4096 {
				message = message[:len(message)-1]
			}
			message = strings.ToValidUTF8(message, "")
		}
		if strings.TrimSpace(message) != "" {
			input["message"] = message
		}
	}
	return input
}

func pluginResultFailed(value any) bool {
	object, ok := value.(map[string]any)
	if !ok {
		return false
	}
	failed, _ := object["isError"].(bool)
	return failed
}

func (owner *pluginScriptTurn) beforeModel(handler string) func(context.Context, []canonical.Record) (bool, error) {
	lastCall := ""
	return func(ctx context.Context, records []canonical.Record) (bool, error) {
		calls := map[string]string{}
		results := []any{}
		newest := ""
		for _, record := range records {
			if message := record.Message(); message != nil {
				for _, block := range message.Content {
					switch value := block.Data.(type) {
					case *canonical.ToolRequest:
						calls[value.ID] = value.Name
					case *canonical.ToolResponse:
						newest = value.ID
						for _, declaration := range owner.info.Extension.Tools {
							if plugins.ToolAlias(owner.info.Summary.Name, declaration.Name) == calls[value.ID] {
								var response any
								_ = json.Unmarshal(value.Result.Value, &response)
								results = append(results, map[string]any{"name": declaration.Name, "response": response})
							}
						}
					}
				}
			}
		}
		if newest == lastCall && newest != "" {
			return false, nil
		}
		body, err := owner.evaluate(ctx, handler, "turn.before_model", newest, map[string]any{"results": results}, false)
		if err != nil {
			return false, err
		}
		lastCall = newest
		var response struct {
			Stop bool `json:"stop"`
		}
		_ = json.Unmarshal(body, &response)
		return response.Stop, nil
	}
}
