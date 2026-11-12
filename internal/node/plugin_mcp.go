package node

import (
	"context"
	"database/sql"
	"errors"
	"path/filepath"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) declaredPluginMCP(ctx context.Context, reference protocol.PluginReference) (protocol.PluginMcpConfiguration, error) {
	body, err := node.plugins.manager.ReadResource(ctx, reference, "mcp.json", 256*1024)
	if err != nil {
		return protocol.PluginMcpConfiguration{}, err
	}
	entries, servers, issues := plugins.ReadMCP(body)
	if len(issues) != 0 || len(entries) == 0 {
		return protocol.PluginMcpConfiguration{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin has no valid MCP servers"}
	}
	return protocol.PluginMcpConfiguration{Schema: "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json", Servers: servers}, nil
}

func (node *Node) capturedPluginMCP(ctx context.Context, reference protocol.PluginReference, current bool) (protocol.PluginInfo, store.ResolvedPluginSettings, protocol.PluginMcpConfiguration, error) {
	var info protocol.PluginInfo
	var settings store.ResolvedPluginSettings
	read := func(db *sql.Conn) error {
		var err error
		if current {
			stored, err := store.ReadPlugin(ctx, db, reference.Name)
			if err != nil {
				return err
			}
			if stored == nil {
				return &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "plugin is not installed"}
			}
			if plugins.PackageReference(*stored) != reference {
				return &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin configuration changed"}
			}
			info = *stored
		} else {
			info, err = store.ResolvePluginInfo(ctx, db, reference)
			if err != nil {
				return err
			}
		}
		settings, err = store.ResolvePluginSettings(ctx, db, reference)
		return err
	}
	if err := node.profile.Store.Node.Read(ctx, read); err != nil {
		return info, settings, protocol.PluginMcpConfiguration{}, err
	}
	declared, err := node.declaredPluginMCP(ctx, reference)
	if err != nil {
		return info, settings, declared, err
	}
	configuration, err := store.EffectivePluginMCP(settings, declared)
	if err == nil && current {
		err = node.profile.Store.Node.Read(ctx, read)
	}
	return info, settings, configuration, err
}

func (node *Node) readPluginMCP(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	if request.Plugin != nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "MCP configuration is not exposed to plugin actions"}
	}
	input := request.Command.Data.(*protocol.CommandReadPluginMcpPayload)
	_, _, configuration, err := node.capturedPluginMCP(ctx, input.Package, true)
	return protocol.Output{Kind: "plugin_mcp", Data: protocol.PluginMcpState{Package: input.Package, Configuration: configuration}}, err
}

func (node *Node) checkPluginMCP(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	if request.Plugin != nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "MCP checks are not exposed to plugin actions"}
	}
	input := request.Command.Data.(*protocol.CommandCheckPluginMcpPayload)
	info, settings, configuration, err := node.capturedPluginMCP(ctx, input.Package, true)
	if err != nil {
		return protocol.Output{}, err
	}
	if !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "MCP package is disabled or incompatible"}
	}
	server, exists := configuration.Servers[input.Server]
	if !exists {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "MCP server is unavailable"}
	}
	prepared, err := node.prepareMCPLaunch(ctx, info, settings, input.Server, server)
	if err != nil {
		return protocol.Output{}, err
	}
	node.startComputer(info, server, &prepared)
	lease, err := node.plugins.mcp.Acquire(ctx, prepared)
	if err != nil {
		return protocol.Output{}, err
	}
	tools, err := lease.List(ctx)
	err = errors.Join(err, lease.Close())
	if err != nil {
		return protocol.Output{}, err
	}
	value := protocol.PluginMcpCheck{Package: input.Package, Server: input.Server, Tools: []protocol.PluginMcpTool{}}
	for _, tool := range tools {
		var description *string
		if tool.Description != "" {
			copy := tool.Description
			description = &copy
		}
		value.Tools = append(value.Tools, protocol.PluginMcpTool{Name: tool.Name, Description: description})
	}
	return protocol.Output{Kind: "plugin_mcp_checked", Data: value}, nil
}

func (node *Node) pluginMCPTargets(ctx context.Context, reference protocol.PluginReference) (map[string]string, error) {
	var declared bool
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		info, err := store.ResolvePluginInfo(ctx, db, reference)
		if err != nil {
			return err
		}
		declared = len(info.Mcp) != 0
		return nil
	}); err != nil {
		return nil, err
	}
	if !declared {
		return map[string]string{}, nil
	}
	info, _, configuration, err := node.capturedPluginMCP(ctx, reference, false)
	if err != nil {
		return nil, err
	}
	return store.MCPSettingsTargets(info.Settings, configuration), nil
}

func expandMCP(value, packageRoot, dataRoot string) string {
	return strings.NewReplacer("${PLUGIN_ROOT}", packageRoot, "${PLUGIN_DATA}", dataRoot).Replace(value)
}

func mcpDirectory(value, packageRoot, dataRoot string) (string, string, error) {
	root, prefix := packageRoot, "${PLUGIN_ROOT}"
	if value == "${PLUGIN_DATA}" || strings.HasPrefix(value, "${PLUGIN_DATA}/") {
		root, prefix = dataRoot, "${PLUGIN_DATA}"
	} else if value == "." || strings.HasPrefix(value, "./") {
		prefix = "."
	}
	if value != prefix && !strings.HasPrefix(value, prefix+"/") {
		return "", "", &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "MCP directory must belong to package or data"}
	}
	relative := strings.TrimPrefix(strings.TrimPrefix(value, prefix), "/")
	if relative == "" {
		relative = "."
	}
	if !filepath.IsLocal(filepath.FromSlash(relative)) {
		return "", "", &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "MCP directory escapes its root"}
	}
	return root, relative, nil
}

// projectPluginMCP fills each server's permission, availability and last
// runtime status. It reads configuration and authorization and starts nothing.
func (node *Node) projectPluginMCP(ctx context.Context, info *protocol.PluginInfo) error {
	if len(info.Mcp) == 0 {
		return nil
	}
	reference := plugins.PackageReference(*info)
	var settings store.ResolvedPluginSettings
	var connections []protocol.PluginConnectorConnection
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		if settings, err = store.ResolvePluginSettings(ctx, db, reference); err != nil {
			return err
		}
		connections, err = store.Connections(ctx, db)
		return err
	})
	if err != nil {
		return err
	}
	declarations := store.ConnectorDeclarations(*info)
	launched := false
	for index := range info.Mcp {
		entry := &info.Mcp[index]
		entry.Allowed = true
		if allowed, exists := settings.MCPAllowed[entry.Name]; exists {
			entry.Allowed = allowed
		}
		entry.Availability = protocol.PluginMcpAvailabilityAvailable
		if info.Summary.Compatibility.Kind != "compatible" {
			entry.Availability = protocol.PluginMcpAvailabilityUnavailable
		} else if !info.Summary.Enabled {
			entry.Availability = protocol.PluginMcpAvailabilityPackageDisabled
		} else if !entry.Allowed {
			entry.Availability = protocol.PluginMcpAvailabilityServerDisabled
		} else {
			if info.Settings != nil {
				for _, name := range info.Settings.Required {
					field := info.Settings.Properties[name]
					if field.Secret == nil || field.Secret.Server == nil || *field.Secret.Server != entry.Name {
						continue
					}
					if _, configured := settings.Secrets[name]; !configured {
						entry.Availability = protocol.PluginMcpAvailabilityConfigurationRequired
					}
				}
			}
			if entry.Availability == protocol.PluginMcpAvailabilityAvailable {
				entry.Availability = connectorAvailability(*info, declarations, connections, entry.Name)
			}
		}
		entry.Runtime = node.plugins.mcp.Status(reference, entry.Name)
		launched = launched || entry.Transport == protocol.PluginMcpTransportStdio && entry.Availability == protocol.PluginMcpAvailabilityAvailable
	}
	if info.Settings == nil && !launched {
		return nil
	}
	declared, err := node.declaredPluginMCP(ctx, reference)
	if err != nil {
		return err
	}
	configuration, err := store.EffectivePluginMCP(settings, declared)
	if err != nil {
		return err
	}
	if info.Settings != nil {
		targets := store.MCPSettingsTargets(info.Settings, configuration)
		for name, field := range info.Settings.Properties {
			if field.Secret == nil || field.Secret.Server == nil {
				continue
			}
			if _, present := settings.Secrets[name]; present && settings.MCPTargets[name] != targets[name] {
				for index := range info.Mcp {
					entry := &info.Mcp[index]
					if entry.Name == *field.Secret.Server && entry.Availability == protocol.PluginMcpAvailabilityAvailable {
						entry.Availability = protocol.PluginMcpAvailabilityConfigurationRequired
					}
				}
			}
		}
	}
	// A server launched from the bundled helper cannot start without it: the
	// entry says so with the reason instead of waiting for a failed check.
	for index := range info.Mcp {
		entry := &info.Mcp[index]
		if entry.Availability != protocol.PluginMcpAvailabilityAvailable || !computerServer(configuration.Servers[entry.Name]) {
			continue
		}
		if _, err := computer.Executable(); err != nil {
			entry.Availability = protocol.PluginMcpAvailabilityUnavailable
			fault := &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: err.Error()}
			errors.As(err, &fault)
			entry.Runtime = &protocol.PluginMcpRuntime{State: protocol.PluginMcpRuntimeStateFailed, Fault: fault, UpdatedAtMs: uint64(time.Now().UnixMilli())}
		}
	}
	return nil
}

// connectorAvailability is what the accounts bound to a server allow: a
// declared connector that injects into it, or authorizes it, needs a usable
// connection unless every credential it declares is optional.
func connectorAvailability(info protocol.PluginInfo, declarations []protocol.PluginConnectorDeclaration, connections []protocol.PluginConnectorConnection, server string) protocol.PluginMcpAvailability {
	for _, declaration := range declarations {
		bound := slices.ContainsFunc(declaration.Inject, func(item protocol.PluginConnectorInjection) bool {
			return item.Server != nil && *item.Server == server
		}) || slices.ContainsFunc(declaration.Auth, func(auth protocol.PluginConnectorAuth) bool {
			value, declared := dispatchPayload[protocol.PluginConnectorAuthMcpOauthPayload](auth.Data)
			return declared && auth.Kind == "mcp_oauth" && value.Server == server
		})
		if !bound {
			continue
		}
		index := slices.IndexFunc(connections, func(connection protocol.PluginConnectorConnection) bool {
			return connection.Package.Name == info.Summary.Name && connection.Connector == declaration.ID
		})
		if index < 0 {
			// Manual headers and anonymous custom servers need no account.
			if info.McpSource != nil || connectorOptional(declaration) {
				continue
			}
			return protocol.PluginMcpAvailabilityConnectionRequired
		}
		connection := connections[index]
		switch connection.State {
		case protocol.PluginConnectorStateConnected:
			if !connection.Enabled {
				return protocol.PluginMcpAvailabilityConnectionDisabled
			}
		case protocol.PluginConnectorStateExpired:
			return protocol.PluginMcpAvailabilityAuthorizationExpired
		case protocol.PluginConnectorStateNeedsConfirmation:
			return protocol.PluginMcpAvailabilityConsentRequired
		case protocol.PluginConnectorStateFailed:
			return protocol.PluginMcpAvailabilityUnavailable
		default:
			return protocol.PluginMcpAvailabilityConnectionRequired
		}
	}
	return protocol.PluginMcpAvailabilityAvailable
}

// connectorOptional reports whether every credential a connector declares is
// optional, so its servers start without an account.
func connectorOptional(declaration protocol.PluginConnectorDeclaration) bool {
	return !slices.ContainsFunc(declaration.Auth, func(auth protocol.PluginConnectorAuth) bool {
		value, declared := dispatchPayload[protocol.PluginConnectorAuthSecretPayload](auth.Data)
		return !declared || auth.Kind != "secret" || slices.ContainsFunc(value.Fields, func(name string) bool {
			return !slices.ContainsFunc(declaration.Fields, func(field protocol.PluginConnectorField) bool { return field.Name == name && field.Optional })
		})
	})
}
