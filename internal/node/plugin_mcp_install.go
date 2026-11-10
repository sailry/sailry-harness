package node

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func standaloneMCPDocuments(name string, definition protocol.PluginMcpDefinition) (map[string]any, error) {
	if !strings.HasPrefix(name, "mcp-") || name == "mcp-" {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "standalone MCP names must start with mcp-"}
	}
	server := map[string]any{"type": definition.Kind}
	var slots []string
	var key string
	switch value := definition.Data.(type) {
	case *protocol.PluginMcpDefinitionStdioPayload:
		server["command"], server["args"], key, slots = value.Command, value.Args, "env", value.Env
	case *protocol.PluginMcpDefinitionStreamableHttpPayload:
		server["url"], key, slots = value.Url, "headers", value.Headers
	case *protocol.PluginMcpDefinitionSsePayload:
		server["url"], key, slots = value.Url, "headers", value.Headers
	default:
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid standalone MCP definition"}
	}
	empty := map[string]string{}
	properties := map[string]any{}
	seen := map[string]bool{}
	if len(slots) > 64 {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "too many MCP credential slots"}
	}
	for _, slot := range slots {
		folded := strings.ToLower(slot)
		if slot == "" || seen[folded] {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid MCP credential slots"}
		}
		seen[folded] = true
		empty[slot] = ""
		sum := blake3.Sum256([]byte(slot))
		field := "slot_" + hex.EncodeToString(sum[:16])
		binding := map[string]any{"server": "server", "env": nil, "header": nil}
		if key == "env" {
			binding["env"] = slot
		} else {
			binding["header"] = slot
		}
		properties[field] = map[string]any{"type": "string", "title": slot, "maxLength": 4096, "x-sailry-secret": binding}
	}
	server[key] = empty
	configuration := map[string]any{"$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json", "mcpServers": map[string]any{"server": server}}
	body, _ := json.Marshal(configuration)
	servers, _, issues := plugins.ReadMCP(body)
	if len(issues) != 0 || len(servers) != 1 {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid standalone MCP configuration"}
	}
	return map[string]any{
		"plugin.json": map[string]any{"$schema": plugins.ManifestSchema, "name": name, "version": "0.1.0",
			"extensions": map[string]any{"dev.sailry.platform": map[string]any{"api_version": "v1", "actions": []any{},
				"desktop": nil, "settings_schema": "dev.sailry.platform/settings.json", "settings_page": nil}}},
		"mcp.json": configuration,
		"dev.sailry.platform/settings.json": map[string]any{"$schema": plugins.SettingsSchema, "type": "object",
			"properties": properties, "additionalProperties": false},
	}, nil
}

func (node *Node) prepareStandaloneMCP(ctx context.Context, input protocol.CommandInstallMcpPayload) (protocol.PluginInfo, error) {
	documents, err := standaloneMCPDocuments(input.Name, input.Definition)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	var archive bytes.Buffer
	writer := zip.NewWriter(&archive)
	for _, name := range []string{"plugin.json", "mcp.json", "dev.sailry.platform/settings.json"} {
		file, err := writer.Create(name)
		if err != nil {
			return protocol.PluginInfo{}, err
		}
		body, err := json.Marshal(documents[name])
		if err != nil {
			return protocol.PluginInfo{}, err
		}
		if _, err = file.Write(body); err != nil {
			return protocol.PluginInfo{}, err
		}
	}
	if err = writer.Close(); err != nil {
		return protocol.PluginInfo{}, err
	}
	info, err := node.plugins.manager.InstallArchive(ctx, bytes.NewReader(archive.Bytes()), int64(archive.Len()), input.Name)
	if err != nil {
		return info, err
	}
	if len(info.Issues) != 0 || len(info.Mcp) != 1 || info.Settings == nil {
		return info, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid standalone MCP package"}
	}
	info.McpSource = &input.Definition
	return info, nil
}
