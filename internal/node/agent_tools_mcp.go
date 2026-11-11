package node

import (
	"context"
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const mcpInventoryLost = "MCP inventory response is unavailable"

// mcpSlots lists the credential slot names of a standalone MCP definition.
func mcpSlots(definition protocol.PluginMcpDefinition) []string {
	if value, stdio := dispatchPayload[protocol.PluginMcpDefinitionStdioPayload](definition.Data); stdio {
		return value.Env
	}
	if value, http := dispatchPayload[protocol.PluginMcpDefinitionStreamableHttpPayload](definition.Data); http {
		return value.Headers
	}
	if value, sse := dispatchPayload[protocol.PluginMcpDefinitionSsePayload](definition.Data); sse {
		return value.Headers
	}
	return nil
}

func mcpManagerSummary(info protocol.PluginInfo) map[string]any {
	servers := []any{}
	for _, server := range info.Mcp {
		servers = append(servers, map[string]any{"name": server.Name, "transport": server.Transport})
	}
	slots := []string{}
	if info.McpSource != nil {
		slots = append(slots, mcpSlots(*info.McpSource)...)
	}
	return map[string]any{"name": info.Summary.Name, "revision": info.Summary.Revision, "enabled": info.Summary.Enabled,
		"standalone": info.McpSource != nil, "servers": servers, "credential_slots": slots}
}

// mcpManager declares manage_mcp over the installed inventory and the durable
// standalone MCP installer. Arguments carry credential slot names only.
func (node *Node) mcpManager(inv *store.AgentInvocation) agentTool {
	plan := inv.Turn.Config.Mode == protocol.WorkModePlan
	read := func(ctx context.Context, name string) (protocol.PluginInfo, error) {
		info, found, err := node.agentPlugin(ctx, inv, name, mcpInventoryLost)
		if err == nil && !found {
			err = connectionFault(protocol.ErrorCodeInvalidRequest, "expected an MCP package")
		}
		return info, err
	}
	standalone := func(info protocol.PluginInfo, revision uint64) (protocol.PluginMcpDefinition, error) {
		if info.McpSource == nil {
			return protocol.PluginMcpDefinition{}, connectionFault(protocol.ErrorCodePermissionDenied, "only standalone MCP packages can be managed by this tool")
		}
		if revision == 0 || info.Summary.Revision != revision {
			return protocol.PluginMcpDefinition{}, connectionFault(protocol.ErrorCodeRevisionConflict, "MCP server changed; reload its revision")
		}
		return *info.McpSource, nil
	}
	prepare := func(ctx context.Context, arguments json.RawMessage) (protocol.Command, error) {
		var input struct {
			Action           *string                       `json:"action"`
			Name             string                        `json:"name"`
			ExpectedRevision uint64                        `json:"expected_revision"`
			Definition       *protocol.PluginMcpDefinition `json:"definition"`
			ClearCredentials []string                      `json:"clear_credentials"`
		}
		if err := agentArguments(arguments, &input); err != nil {
			return protocol.Command{}, err
		}
		if input.Action == nil {
			return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "MCP action is required")
		}
		if !slices.Contains([]string{"list", "install", "update", "uninstall"}, *input.Action) {
			return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "unknown MCP action")
		}
		if plan && *input.Action != "list" {
			return protocol.Command{}, connectionFault(protocol.ErrorCodePermissionDenied, "MCP mutations are unavailable in Plan mode")
		}
		switch *input.Action {
		case "install":
			if input.Definition == nil {
				return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "invalid tool arguments: missing field `definition`")
			}
			secrets := map[string]protocol.PluginSettingsSecretUpdate{}
			for _, slot := range mcpSlots(*input.Definition) {
				secrets[slot] = protocol.PluginSettingsSecretUpdate{Kind: "clear"}
			}
			return protocol.Command{Kind: "install_mcp", Data: &protocol.CommandInstallMcpPayload{Name: input.Name, Definition: *input.Definition, Secrets: secrets}}, nil
		case "update":
			info, err := read(ctx, input.Name)
			if err != nil {
				return protocol.Command{}, err
			}
			previous, err := standalone(info, input.ExpectedRevision)
			if err != nil {
				return protocol.Command{}, err
			}
			definition := previous
			if input.Definition != nil {
				definition = *input.Definition
			}
			// Existing credential bindings are retained. Adding or changing slots
			// belongs in the protected configuration form, never model arguments.
			slots, retained := mcpSlots(definition), mcpSlots(previous)
			preserved := definition.Kind == previous.Kind && len(slots) == len(retained)
			seen := map[string]bool{}
			for _, slot := range slots {
				preserved = preserved && !seen[slot] && slices.Contains(retained, slot)
				seen[slot] = true
			}
			if !preserved {
				return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "MCP updates must preserve transport and credential slots")
			}
			secrets := map[string]protocol.PluginSettingsSecretUpdate{}
			for _, slot := range slots {
				secrets[slot] = protocol.PluginSettingsSecretUpdate{Kind: "keep"}
			}
			for _, slot := range input.ClearCredentials {
				if !seen[slot] || secrets[slot].Kind == "clear" {
					return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "unknown or repeated MCP credential slot")
				}
				secrets[slot] = protocol.PluginSettingsSecretUpdate{Kind: "clear"}
			}
			return protocol.Command{Kind: "install_mcp", Data: &protocol.CommandInstallMcpPayload{Name: input.Name, ExpectedRevision: input.ExpectedRevision, Definition: definition, Secrets: secrets}}, nil
		case "uninstall":
			info, err := read(ctx, input.Name)
			if err != nil {
				return protocol.Command{}, err
			}
			if _, err := standalone(info, input.ExpectedRevision); err != nil {
				return protocol.Command{}, err
			}
			return protocol.Command{Kind: "remove_plugin", Data: &protocol.CommandRemovePluginPayload{Name: input.Name, ExpectedRevision: input.ExpectedRevision}}, nil
		}
		return protocol.Command{Kind: "list_plugins"}, nil
	}
	project := func(ctx context.Context, output protocol.Output) (any, error) {
		if entries, listed := dispatchPayload[[]protocol.PluginSummary](output.Data); listed && output.Kind == "plugins" {
			servers := []any{}
			for _, entry := range entries {
				info, err := read(ctx, entry.Name)
				if err != nil {
					return nil, err
				}
				if len(info.Mcp) != 0 {
					servers = append(servers, mcpManagerSummary(info))
				}
			}
			return map[string]any{"servers": servers}, nil
		}
		if info, found := dispatchPayload[protocol.PluginInfo](output.Data); found && output.Kind == "plugin" {
			return mcpManagerSummary(info), nil
		}
		return nil, connectionFault(protocol.ErrorCodeInvalidRequest, "unexpected MCP management result")
	}
	text := map[string]any{"type": "string"}
	slots := map[string]any{"type": "array", "items": text, "uniqueItems": true, "description": "Credential names only, never values"}
	definition := map[string]any{"oneOf": []any{
		objectSchema(map[string]any{"type": map[string]any{"const": "stdio"}, "command": text, "args": map[string]any{"type": "array", "items": text}, "env": slots}, "type", "command", "args", "env"),
		objectSchema(map[string]any{"type": map[string]any{"enum": []any{"streamable-http", "sse"}}, "url": text, "headers": slots}, "type", "url", "headers"),
	}}
	name := map[string]any{"type": "string", "pattern": "^mcp-.+"}
	revision := map[string]any{"type": "integer", "minimum": 1}
	parameters := actionSchema([]string{"list", "install", "update", "uninstall"}, []map[string]any{
		objectSchema(map[string]any{}),
		objectSchema(map[string]any{"name": name, "definition": definition}, "name", "definition"),
		objectSchema(map[string]any{"name": name, "expected_revision": revision, "definition": definition, "clear_credentials": slots}, "name", "expected_revision"),
		objectSchema(map[string]any{"name": name, "expected_revision": revision}, "name", "expected_revision"),
	})
	return agentTool{
		name:        "manage_mcp",
		description: "Manage MCP servers on this execution Node: list declarations without starting servers, or install/update/uninstall standalone entries using their current revision. Definition env/headers contain credential slot names only. Never include private values in tool arguments, URLs or command arguments; use the protected configuration form. Existing secrets and enabled state are retained on update unless clear_credentials explicitly names a slot. OAuth still requires interactive authorization. Plan permits only list. Changes affect future captures, not this turn's tools. Do not replay uncertain mutations.",
		parameters:  parameters, readOnly: plan, lost: mcpInventoryLost, prepare: prepare, project: project,
	}
}
