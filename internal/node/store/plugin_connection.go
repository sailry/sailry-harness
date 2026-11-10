package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"maps"
	"net/url"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func CheckPluginConnectionRequest(request protocol.Request, info protocol.PluginInfo) error {
	scope := request.Plugin
	input, err := providerPayload[protocol.CommandReadPluginConnectionPayload](request.Command)
	if err != nil || scope == nil || input.Package != scope.Package || scope.Turn == nil && scope.Invocation == nil {
		return fault(protocol.ErrorCodePermissionDenied, "connection status requires an owned card")
	}
	if info.Extension == nil {
		return fault(protocol.ErrorCodePermissionDenied, "connection status requires an owned card")
	}
	for _, declaration := range info.Extension.Cards {
		if declaration.Key != input.Card {
			continue
		}
		if declaration.McpServer != nil && slices.ContainsFunc(info.Mcp, func(entry protocol.PluginMcp) bool {
			return entry.Name == *declaration.McpServer && entry.Transport != protocol.PluginMcpTransportStdio
		}) {
			return nil
		}
		return fault(protocol.ErrorCodePermissionDenied, "connection status requires an owned HTTP server")
	}
	return fault(protocol.ErrorCodePermissionDenied, "connection status requires an owned card")
}

func ReadPluginConnection(ctx context.Context, db *sql.Conn, profile string, reference protocol.PluginReference, card string) (protocol.PluginConnectionStatus, error) {
	info, err := currentMCPPlugin(ctx, db, reference)
	if err != nil {
		return protocol.PluginConnectionStatus{}, err
	}
	return ReadPluginConnectionAdmitted(ctx, db, profile, info, card)
}

func ReadPluginConnectionAdmitted(ctx context.Context, db *sql.Conn, profile string, info protocol.PluginInfo, card string) (protocol.PluginConnectionStatus, error) {
	var declaration *protocol.PluginCardDeclaration
	if info.Extension != nil {
		for index := range info.Extension.Cards {
			if info.Extension.Cards[index].Key == card {
				declaration = &info.Extension.Cards[index]
				break
			}
		}
	}
	if declaration == nil {
		return protocol.PluginConnectionStatus{}, fault(protocol.ErrorCodeNotFound, "plugin card is unavailable")
	}
	if declaration.McpServer == nil {
		return protocol.PluginConnectionStatus{}, fault(protocol.ErrorCodeNotConfigured, "plugin card has no MCP server")
	}
	server := *declaration.McpServer
	connector := ""
	for _, candidate := range ConnectorDeclarations(info) {
		if slices.ContainsFunc(candidate.Auth, func(auth protocol.PluginConnectorAuth) bool {
			if auth.Kind != "mcp_oauth" {
				return false
			}
			method, valid := dispatchValue[protocol.PluginConnectorAuthMcpOauthPayload](auth.Data)
			return valid && method.Server == server
		}) {
			connector = candidate.ID
			break
		}
	}
	if connector == "" {
		return protocol.PluginConnectionStatus{}, fault(protocol.ErrorCodeNotConfigured, "plugin card has no connector")
	}
	records, err := readBodies[ConnectorRecord](ctx, db, "SELECT body FROM plugin_connections ORDER BY plugin,connector,id")
	if err != nil {
		return protocol.PluginConnectionStatus{}, err
	}
	var row *protocol.PluginConnectorConnection
	for _, record := range records {
		if record.Plugin != info.Summary.Name || record.Connector != connector {
			continue
		}
		value, err := pluginConnectionRow(ctx, db, profile, record)
		if err != nil {
			return protocol.PluginConnectionStatus{}, err
		}
		row = &value
		break
	}
	result := protocol.PluginConnectionStatus{
		Package: plugins.PackageReference(info), Card: card, Server: server,
		Configuration: protocol.PluginConnectionConfigurationReady,
		Authorization: protocol.PluginConnectionAuthorizationMissing,
		Credential:    protocol.PluginConnectionCredentialNotDeclared,
	}
	method := protocol.PluginConnectorKindMcpOauth
	ready := false
	if row != nil {
		method = row.Method
		ready = row.Enabled && row.State == protocol.PluginConnectorStateConnected
		if row.State == protocol.PluginConnectorStateNeedsConfirmation {
			result.Configuration = protocol.PluginConnectionConfigurationChanged
		}
	}
	switch method {
	case protocol.PluginConnectorKindOauth2, protocol.PluginConnectorKindApplication, protocol.PluginConnectorKindMcpOauth:
		if ready {
			result.Authorization = protocol.PluginConnectionAuthorizationConfigured
		}
	default:
		result.Authorization = protocol.PluginConnectionAuthorizationNotDeclared
	}
	return result, nil
}

func pluginConnectionRow(ctx context.Context, db *sql.Conn, profile string, record ConnectorRecord) (protocol.PluginConnectorConnection, error) {
	info, err := ReadPlugin(ctx, db, record.Plugin)
	if err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	if info == nil {
		return protocol.PluginConnectorConnection{}, fault(protocol.ErrorCodeNotFound, "connector package is unavailable")
	}
	var declaration *protocol.PluginConnectorDeclaration
	for _, candidate := range ConnectorDeclarations(*info) {
		if candidate.ID == record.Connector {
			declaration = &candidate
			break
		}
	}
	var bound *ConnectorBound
	if declaration != nil {
		bound = connectorBound(*info, *declaration, record.Method)
		if bound != nil {
			bound.Targets, err = pluginConnectionTargets(ctx, db, profile, *info, *declaration)
			if err != nil {
				return protocol.PluginConnectorConnection{}, err
			}
		}
	}
	state := record.State
	switch {
	case declaration == nil || !info.Summary.Enabled:
		state = protocol.PluginConnectorStateNotConnected
	case connectorConsent(record.Plugin, record.Bound, bound) != nil:
		state = protocol.PluginConnectorStateNeedsConfirmation
	case !slices.ContainsFunc(declaration.Auth, func(auth protocol.PluginConnectorAuth) bool { return auth.Kind == string(record.Method) }):
		state = protocol.PluginConnectorStateNotConnected
	case record.Method == protocol.PluginConnectorKindMcpOauth:
		switch {
		case record.Login != nil:
			state = protocol.PluginConnectorStateConnecting
		case record.State != protocol.PluginConnectorStateConnected:
		case record.Grant == nil || record.Grant.Access == "":
			state = protocol.PluginConnectorStateNotConnected
		case record.Grant.ExpiresAtMs != nil && *record.Grant.ExpiresAtMs <= uint64(time.Now().UnixMilli()):
			state = protocol.PluginConnectorStateExpired
		}
	}
	return protocol.PluginConnectorConnection{Enabled: record.Enabled, Method: record.Method, State: state}, nil
}

func pluginConnectionTargets(ctx context.Context, db *sql.Conn, profile string, info protocol.PluginInfo, declaration protocol.PluginConnectorDeclaration) (map[string]protocol.PluginMcpDefinition, error) {
	result := map[string]protocol.PluginMcpDefinition{}
	servers := map[string]bool{}
	for _, injection := range declaration.Inject {
		if injection.Server != nil {
			servers[*injection.Server] = true
		}
	}
	if len(servers) == 0 {
		return result, nil
	}
	settings, err := readStoredPluginSettings(ctx, db, info)
	if err != nil {
		return nil, err
	}
	var configuration protocol.PluginMcpConfiguration
	if string(settings.Mcp) != "null" {
		if err := json.Unmarshal(settings.Mcp, &configuration); err != nil {
			return nil, invalid("stored MCP configuration is invalid")
		}
	} else {
		body, err := plugins.NewManager(profile).ReadResource(ctx, plugins.PackageReference(info), "mcp.json", 256*1024)
		if err != nil {
			return nil, err
		}
		entries, declared, issues := plugins.ReadMCP(body)
		if len(entries) == 0 || len(issues) != 0 {
			return nil, fault(protocol.ErrorCodeNotConfigured, "plugin has no valid MCP servers")
		}
		configuration.Servers = declared
	}
	for _, name := range slices.Sorted(maps.Keys(servers)) {
		server, exists := configuration.Servers[name]
		if !exists {
			return nil, invalid("connector MCP server is unavailable")
		}
		switch server.Kind {
		case "stdio":
			value, valid := dispatchValue[protocol.PluginMcpServerStdioPayload](server.Data)
			if !valid {
				return nil, invalid("invalid connector MCP server")
			}
			result[name] = protocol.PluginMcpDefinition{Kind: "stdio", Data: protocol.PluginMcpDefinitionStdioPayload{Command: value.Command, Args: value.Args, Env: slices.Sorted(maps.Keys(value.Env))}}
		case "sse", "streamable-http":
			var address string
			var headers []string
			if server.Kind == "sse" {
				value, valid := dispatchValue[protocol.PluginMcpServerSsePayload](server.Data)
				if !valid {
					return nil, invalid("invalid connector MCP URL")
				}
				address, headers = value.Url, slices.Sorted(maps.Keys(value.Headers))
			} else {
				value, valid := dispatchValue[protocol.PluginMcpServerHttpPayload](server.Data)
				if !valid {
					return nil, invalid("invalid connector MCP URL")
				}
				address, headers = value.Url, slices.Sorted(maps.Keys(value.Headers))
			}
			target, err := url.Parse(address)
			if err != nil || target.Host == "" {
				return nil, invalid("invalid connector MCP URL")
			}
			expanded := address
			for _, field := range declaration.Fields {
				if !field.Secret {
					expanded = strings.ReplaceAll(expanded, "{"+field.Name+"}", "tenant")
				}
			}
			endpoint, endpointErr := url.Parse(expanded)
			allowed := endpointErr == nil && !strings.ContainsAny(expanded, "{}") &&
				endpoint.Scheme == "https" && endpoint.User == nil && endpoint.Fragment == "" &&
				slices.Contains(declaration.Origins, endpoint.Scheme+"://"+endpoint.Host)
			if !allowed && info.McpSource != nil && target.User == nil && target.Fragment == "" &&
				slices.Contains(declaration.Origins, target.Scheme+"://"+target.Host) {
				allowed = plugins.ValidateMCPServer(server) == nil
			}
			if !allowed {
				return nil, fault(protocol.ErrorCodePermissionDenied, "connector MCP server is outside its origins")
			}
			if server.Kind == "sse" {
				result[name] = protocol.PluginMcpDefinition{Kind: "sse", Data: protocol.PluginMcpDefinitionSsePayload{Url: target.String(), Headers: headers}}
			} else {
				result[name] = protocol.PluginMcpDefinition{Kind: "streamable-http", Data: protocol.PluginMcpDefinitionStreamableHttpPayload{Url: target.String(), Headers: headers}}
			}
		default:
			return nil, invalid("invalid connector MCP server")
		}
	}
	return result, nil
}
