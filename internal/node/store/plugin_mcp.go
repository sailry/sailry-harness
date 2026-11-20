package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const pluginMCPSchema = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json"

func currentMCPPlugin(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) (protocol.PluginInfo, error) {
	info, err := ReadPlugin(ctx, db, reference.Name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if info == nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotFound, "plugin is not installed")
	}
	if plugins.PackageReference(*info) != reference {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeRevisionConflict, "plugin configuration changed")
	}
	return *info, nil
}

func EffectivePluginMCP(settings ResolvedPluginSettings, declared protocol.PluginMcpConfiguration) (protocol.PluginMcpConfiguration, error) {
	if len(settings.MCP) == 0 || string(settings.MCP) == "null" {
		return declared, nil
	}
	var configuration protocol.PluginMcpConfiguration
	if err := json.Unmarshal(settings.MCP, &configuration); err != nil {
		return configuration, invalid("stored MCP configuration is invalid")
	}
	return configuration, nil
}

func validateMCPConfiguration(info protocol.PluginInfo, configuration protocol.PluginMcpConfiguration) error {
	body, err := json.Marshal(configuration)
	if err != nil || len(body) > 256*1024 || configuration.Schema != pluginMCPSchema || len(configuration.Servers) != len(info.Mcp) || len(configuration.Servers) > 16 {
		return invalid("MCP configuration does not match its package declaration")
	}
	for _, entry := range info.Mcp {
		server, exists := configuration.Servers[entry.Name]
		if !exists || server.Kind != string(entry.Transport) {
			return invalid("MCP configuration cannot change declared servers or transports")
		}
		if err := plugins.ValidateMCPServer(server); err != nil {
			return err
		}
	}
	return nil
}

func persistMCPSettings(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, settings storedPluginSettings) (protocol.PluginInfo, error) {
	user, err := pluginUserID(ctx, db)
	if err != nil {
		return info, err
	}
	info.Summary.SettingsRevision, err = nextRevision(info.Summary.SettingsRevision)
	if err != nil {
		return info, err
	}
	info.Summary.Revision, err = nextRevision(info.Summary.Revision)
	if err != nil {
		return info, err
	}
	body, err := encode(settings)
	if err != nil {
		return info, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO plugin_user_settings(user,plugin,revision,body) VALUES(?4,?1,?2,?3)", info.Summary.Name, info.Summary.SettingsRevision, body, user); err != nil {
		return info, err
	}
	if err = savePluginInventory(ctx, db, info, true); err != nil {
		return info, err
	}
	return info, nil
}

func SavePluginMCP(ctx context.Context, db *sql.Conn, input protocol.CommandSavePluginMcpPayload, declared protocol.PluginMcpConfiguration) (protocol.PluginInfo, error) {
	info, err := currentMCPPlugin(ctx, db, input.Package)
	if err != nil {
		return info, err
	}
	if info.McpSource != nil {
		return info, invalid("standalone MCP settings use their declared form")
	}
	if err = validateMCPConfiguration(info, input.Configuration); err != nil {
		return info, err
	}
	saved, err := readStoredPluginSettings(ctx, db, info)
	if err != nil {
		return info, err
	}
	current, err := EffectivePluginMCP(ResolvedPluginSettings{MCP: saved.Mcp}, declared)
	if err != nil {
		return info, err
	}
	if info.Settings != nil {
		for _, field := range info.Settings.Properties {
			if binding := field.Secret; binding != nil && binding.Server != nil && (binding.Header != nil || binding.Env != nil) {
				previous, exists := current.Servers[*binding.Server]
				if !exists || !reflect.DeepEqual(previous, input.Configuration.Servers[*binding.Server]) {
					return info, invalid("MCP slots use the plugin settings form")
				}
			}
		}
	}
	saved.Mcp, err = json.Marshal(input.Configuration)
	if err != nil {
		return info, err
	}
	return persistMCPSettings(ctx, db, info, saved)
}

func SetPluginMCPAllowed(ctx context.Context, db *sql.Conn, input protocol.CommandSetPluginMcpAllowedPayload) (protocol.PluginInfo, error) {
	info, err := currentMCPPlugin(ctx, db, input.Package)
	if err != nil {
		return info, err
	}
	found := false
	for _, entry := range info.Mcp {
		found = found || entry.Name == input.Server
	}
	if !found {
		return info, fault(protocol.ErrorCodeNotFound, "MCP server is unavailable")
	}
	saved, err := readStoredPluginSettings(ctx, db, info)
	if err != nil {
		return info, err
	}
	allowed, exists := saved.McpAllowed[input.Server]
	if !exists {
		allowed = true
	}
	if allowed == input.Allowed {
		return info, nil
	}
	saved.McpAllowed[input.Server] = input.Allowed
	return persistMCPSettings(ctx, db, info, saved)
}

// InstallStandaloneMCP saves the generated package and its credentials in the
// caller's transaction.
func InstallStandaloneMCP(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, input protocol.CommandInstallMcpPayload, configuration protocol.PluginMcpConfiguration, retainStdioEnv bool) (protocol.PluginInfo, error) {
	if info.McpSource == nil || info.Settings == nil {
		return info, invalid("standalone MCP package is unavailable")
	}
	var previousDestinations map[string]string
	if retainStdioEnv && input.ExpectedRevision != 0 && info.McpSource.Kind == "stdio" && len(input.Secrets) != 0 {
		previous, err := ReadPlugin(ctx, db, info.Summary.Name)
		if err != nil {
			return info, err
		}
		previousDestinations = standaloneMCPPreviousTargets(previous, configuration)
	}
	info, err := InstallPlugin(ctx, db, info, input.ExpectedRevision, true)
	if err != nil {
		return info, err
	}
	mapped := map[string]protocol.PluginSettingsSecretUpdate{}
	for name, field := range info.Settings.Properties {
		if field.Title == nil {
			return info, invalid("standalone MCP credential declaration is invalid")
		}
		update, exists := input.Secrets[*field.Title]
		if !exists {
			return info, invalid("every MCP slot requires an explicit credential update")
		}
		mapped[name] = update
	}
	if len(mapped) != len(input.Secrets) {
		return info, invalid("MCP credential update references an undeclared slot")
	}
	_, err = savePluginSettings(ctx, db, &info, protocol.CommandSavePluginSettingsPayload{
		Values: map[string]json.RawMessage{}, Secrets: mapped,
	}, MCPSettingsTargets(info.Settings, configuration), previousDestinations)
	return info, err
}

// An explicit controller edit may keep an environment slot across argument
// changes. Reconstruct the prior destination with the old arguments and the
// new command and directory so the settings save still checks the saved binding.
func standaloneMCPPreviousTargets(previous *protocol.PluginInfo, configuration protocol.PluginMcpConfiguration) map[string]string {
	if previous == nil || previous.McpSource == nil || len(previous.Mcp) != 1 {
		return nil
	}
	name := previous.Mcp[0].Name
	before, wasStdio := previous.McpSource.Data.(*protocol.PluginMcpDefinitionStdioPayload)
	server := configuration.Servers[name]
	after, isStdio := server.Data.(*protocol.PluginMcpServerStdioPayload)
	if !wasStdio || !isStdio || before.Command != after.Command {
		return nil
	}
	old := *after
	old.Args = before.Args
	server.Data = &old
	return MCPSettingsTargets(previous.Settings, protocol.PluginMcpConfiguration{Servers: map[string]protocol.PluginMcpServer{name: server}})
}

func MCPSettingsTargets(schema *protocol.PluginSettingsSchema, configuration protocol.PluginMcpConfiguration) map[string]string {
	targets := map[string]string{}
	if schema == nil {
		return targets
	}
	for name, field := range schema.Properties {
		if field.Secret == nil || field.Secret.Server == nil {
			continue
		}
		server, exists := configuration.Servers[*field.Secret.Server]
		if !exists {
			continue
		}
		var destination any
		switch value := server.Data.(type) {
		case *protocol.PluginMcpServerStdioPayload:
			destination = []any{"stdio", value.Command, value.Args, value.Cwd}
		case *protocol.PluginMcpServerHttpPayload:
			destination = []any{"streamable-http", value.Url}
		case *protocol.PluginMcpServerSsePayload:
			destination = []any{"sse", value.Url}
		}
		if destination != nil {
			body, _ := json.Marshal(destination)
			targets[name] = string(body)
		}
	}
	return targets
}
