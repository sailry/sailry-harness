package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func CheckPluginCardRequest(ctx context.Context, db *sql.Conn, node, _ protocol.NodeID, request protocol.Request) error {
	denied := func() error { return fault(protocol.ErrorCodePermissionDenied, "card is no longer available") }
	scope := request.Plugin
	if scope == nil || scope.Surface.Kind != "card" || scope.Turn != nil || scope.Invocation != nil {
		return denied()
	}
	receipt, valid := pluginViewCardReceipt(scope.Surface.Data)
	if !valid || scope.Session == nil || *scope.Session != receipt.Session {
		return denied()
	}
	info, summary, err := CapturedPluginCard(ctx, db, receipt)
	if err != nil {
		return err
	}
	if summary.Node != node || scope.Worktree == nil || *scope.Worktree != summary.Worktree ||
		scope.Package != plugins.PackageReference(info) || info.Extension == nil {
		return denied()
	}
	var declaration *protocol.PluginCardDeclaration
	for index := range info.Extension.Cards {
		if info.Extension.Cards[index].Key == summary.Key {
			declaration = &info.Extension.Cards[index]
			break
		}
	}
	if declaration == nil || declaration.McpServer == nil {
		return denied()
	}
	server := *declaration.McpServer
	if !slices.ContainsFunc(info.Mcp, func(entry protocol.PluginMcp) bool {
		return entry.Name == server && entry.Transport != protocol.PluginMcpTransportStdio
	}) {
		return denied()
	}
	switch request.Command.Kind {
	case "read_plugin_connection":
		input, err := providerPayload[protocol.CommandReadPluginConnectionPayload](request.Command)
		if err == nil && input.Package == scope.Package && input.Card == declaration.Key {
			return nil
		}
	case "list_connections", "list_connector_types", "submit_connector_input", "cancel_connector_login":
		return nil
	case "start_connector_login":
		input, err := providerPayload[protocol.CommandStartConnectorLoginPayload](request.Command)
		if err != nil || input.Package != scope.Package || input.Method != protocol.PluginConnectorKindMcpOauth ||
			len(input.Input.Secrets) != 0 || len(input.Input.Fields) != 0 || input.Input.Registration ||
			!slices.ContainsFunc(declaration.Capabilities.Browser, func(target protocol.PluginCardTarget) bool {
				return target.Source.Kind == "mcp_authorization"
			}) {
			return denied()
		}
		declared := slices.ContainsFunc(ConnectorDeclarations(info), func(connector protocol.PluginConnectorDeclaration) bool {
			return connector.ID == input.Connector && slices.ContainsFunc(connector.Auth, func(auth protocol.PluginConnectorAuth) bool {
				if auth.Kind != "mcp_oauth" {
					return false
				}
				body, err := json.Marshal(auth.Data)
				var method protocol.PluginConnectorAuthMcpOauthPayload
				return err == nil && json.Unmarshal(body, &method) == nil && method.Server == server
			})
		})
		if !declared {
			return denied()
		}
		if input.Connection != nil {
			record, err := ReadConnectorRecord(ctx, db, *input.Connection)
			if err != nil {
				return err
			}
			if record.Plugin != info.Summary.Name || record.Connector != input.Connector {
				return denied()
			}
		}
		return nil
	}
	return denied()
}
