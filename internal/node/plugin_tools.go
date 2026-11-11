package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"runtime"
	"slices"
	"strings"

	"charm.land/fantasy"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) pluginTools(ctx context.Context, invocation *store.AgentInvocation, native ...[]engine.Tool) (tools []engine.Tool, resultErr error) {
	byName := map[string]engine.Tool{}
	var candidates []pluginMCPBinding
	if len(native) != 0 {
		tools = slices.Clone(native[0])
		for _, tool := range native[0] {
			byName[tool.Definition.Name] = tool
		}
	}
	defer func() {
		if resultErr != nil {
			for _, tool := range tools {
				if tool.Close != nil {
					resultErr = errors.Join(resultErr, tool.Close())
				}
			}
		}
	}()
	for _, reference := range invocation.Turn.Plugins {
		var info protocol.PluginInfo
		var settings store.ResolvedPluginSettings
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			info, err = store.ResolvePluginInfo(ctx, db, reference)
			if err == nil {
				settings, err = store.ResolvePluginSettings(ctx, db, reference)
			}
			return err
		})
		if err != nil {
			return tools, err
		}
		if info.Extension != nil && info.Extension.Host != nil {
			for _, declaration := range info.Extension.Tools {
				if declaration.Handler != nil {
					alias := operations.Alias(info.Summary.Name, declaration.Name)
					tools = slices.DeleteFunc(tools, func(tool engine.Tool) bool { return tool.Definition.Name == alias })
				}
			}
			scripts, err := node.scriptTools(ctx, invocation, info, byName)
			tools = append(tools, scripts...)
			if err != nil {
				return tools, err
			}
		}
		if len(info.Mcp) == 0 {
			continue
		}
		declared, err := node.declaredPluginMCP(ctx, reference)
		if err != nil {
			for _, entry := range info.Mcp {
				candidates = append(candidates, pluginMCPBinding{info: info, server: entry.Name, failure: err})
			}
			continue
		}
		configuration, err := store.EffectivePluginMCP(settings, declared)
		if err != nil {
			for _, entry := range info.Mcp {
				candidates = append(candidates, pluginMCPBinding{info: info, server: entry.Name, failure: err})
			}
			continue
		}
		for _, entry := range info.Mcp {
			if allowed, exists := settings.MCPAllowed[entry.Name]; exists && !allowed {
				continue
			}
			if !selectedMCPServer(invocation, info, entry.Name) {
				continue
			}
			server, exists := configuration.Servers[entry.Name]
			if !exists {
				continue
			}
			candidates = append(candidates, pluginMCPBinding{info: info, settings: settings, server: entry.Name, configuration: server})
		}
	}
	mcpTools, err := node.bindMCPTools(ctx, invocation, candidates)
	return append(tools, mcpTools...), err
}

// selectedMCPServer keeps only the owner's servers an assistant declares.
func selectedMCPServer(invocation *store.AgentInvocation, info protocol.PluginInfo, server string) bool {
	binding := invocation.Turn.Config.Assistant
	if binding == nil {
		return true
	}
	return binding.Package.Name == info.Summary.Name && invocation.Assistant != nil &&
		slices.ContainsFunc(invocation.Assistant.Tools, func(selection protocol.PluginConversationTool) bool {
			value, valid := selection.Data.(*protocol.PluginConversationToolPluginPayload)
			return valid && value.Server != nil && *value.Server == server
		})
}

func selectedMCPTool(invocation *store.AgentInvocation, info protocol.PluginInfo, server, name string) (*protocol.ToolDeclaration, bool) {
	var declaration *protocol.ToolDeclaration
	if info.Extension != nil {
		for index := range info.Extension.Tools {
			candidate := &info.Extension.Tools[index]
			if candidate.Server != nil && *candidate.Server == server && candidate.Name == name {
				declaration = candidate
				break
			}
		}
	}
	if declaration != nil && !availablePluginTool(invocation, *declaration) {
		return nil, false
	}
	return declaration, selectedPluginTool(invocation, info, server, name)
}

func availablePluginTool(invocation *store.AgentInvocation, tool protocol.ToolDeclaration) bool {
	if len(tool.Platforms) > 0 && !slices.Contains(tool.Platforms, runtime.GOOS) {
		return false
	}
	if len(tool.Contexts) == 0 {
		return true
	}
	resource := protocol.PluginConversationResourcePlugin
	if invocation.Worktree.Project != nil {
		resource = protocol.PluginConversationResourceWorkspace
	}
	if invocation.Turn.Config.Resource != nil {
		switch invocation.Turn.Config.Resource.Kind {
		case "database":
			resource = protocol.PluginConversationResourceDatabase
		case "ssh":
			resource = protocol.PluginConversationResourceSsh
		}
	}
	return slices.Contains(tool.Contexts, resource)
}

// selectedPluginTool applies the frozen assistant declaration: the owner's
// declared tools, and package tools the assistant imports explicitly. Imported
// packages contribute no other tool, skill or MCP server.
func selectedPluginTool(invocation *store.AgentInvocation, info protocol.PluginInfo, server, name string) bool {
	binding := invocation.Turn.Config.Assistant
	if binding == nil {
		return true
	}
	if invocation.Assistant == nil {
		return false
	}
	owner := binding.Package.Name == info.Summary.Name
	return slices.ContainsFunc(invocation.Assistant.Tools, func(selection protocol.PluginConversationTool) bool {
		switch value := selection.Data.(type) {
		case *protocol.PluginConversationToolPluginPayload:
			return owner && value.Name == name && (value.Server == nil && server == "" || value.Server != nil && *value.Server == server)
		case *protocol.PluginConversationToolPackagePayload:
			return server == "" && value.Package == info.Summary.Name && value.Name == name
		}
		return false
	})
}

func (node *Node) mcpTool(invocation *store.AgentInvocation, reference protocol.PluginReference, server string, lease *mcp.Lease, descriptor *sdk.Tool, declaration *protocol.ToolDeclaration, options protocol.PluginMcpOptions) (engine.Tool, error) {
	body, err := json.Marshal(descriptor.InputSchema)
	if err != nil {
		return engine.Tool{}, err
	}
	var schema map[string]any
	if json.Unmarshal(body, &schema) != nil || schema["type"] != "object" {
		return engine.Tool{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "MCP tool input schema must describe an object"}
	}
	readOnly := descriptor.Annotations != nil && descriptor.Annotations.ReadOnlyHint
	tool := engine.Tool{Definition: fantasy.FunctionTool{Name: mcp.Alias(reference.Name, server, descriptor.Name), Description: descriptor.Description, InputSchema: schema},
		SchemaJSON: body, ReadOnly: readOnly, Concurrent: readOnly, Deferred: declaration == nil || !declaration.Initial,
		Approval: !readOnly, Binding: &store.ApprovalBinding{Owner: reference, Tool: mcp.Alias(reference.Name, server, descriptor.Name)}}
	if declaration != nil && declaration.Description != nil {
		tool.Definition.Description = *declaration.Description
	}
	keepImages := options.KeepImages
	if declaration != nil && declaration.KeepImages != nil {
		keepImages = declaration.KeepImages
	}
	tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		var arguments map[string]any
		if json.Unmarshal(call.Arguments, &arguments) != nil {
			return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid MCP tool arguments"})
		}
		result, err := lease.Call(ctx, descriptor.Name, arguments, readOnly, func(ctx context.Context, request *sdk.ElicitRequest) (*sdk.ElicitResult, error) {
			return node.agent.Elicit(ctx, invocation.Run.Turn, call, request)
		})
		if err != nil {
			return workspaceToolFault(err)
		}
		boundToolImages(result)
		if keepImages != nil {
			remaining := *keepImages
			result.Content = slices.DeleteFunc(result.Content, func(content sdk.Content) bool {
				if _, image := content.(*sdk.ImageContent); !image {
					return false
				}
				if remaining == 0 {
					return true
				}
				remaining--
				return false
			})
		}
		return engine.ToolResult{Value: result, Format: canonical.MCP, Error: result.IsError}
	}
	return tool, nil
}

func pluginOperationReadOnly(operation protocol.ToolOperation) bool {
	switch string(operation) {
	case "storage.get", "storage.keys", "settings.read", "roles.list", "teams.list", "models.list":
		return true
	}
	readOnly, _ := operations.Policy(operation)
	return readOnly
}

func pluginHandlerReadOnly(handler protocol.ToolHandler, actions []protocol.PluginAction) bool {
	if handler.Operation != nil {
		return pluginOperationReadOnly(*handler.Operation)
	}
	if handler.Flow != nil {
		for _, operation := range handler.Flow.Operations {
			if !pluginOperationReadOnly(operation) {
				return false
			}
		}
		return true
	}
	if handler.ReadOnly {
		return true
	}
	for _, action := range actions {
		if !strings.HasSuffix(string(action), ".read") {
			return false
		}
	}
	return true
}
