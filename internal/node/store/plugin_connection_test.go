package store

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginConnectionStatus(t *testing.T) {
	profile, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("connection-fixture", 'f')
		var extension protocol.PluginExtension
		if err := json.Unmarshal([]byte(`{
			"api_version":"v1","actions":[],
			"cards":[{"key":"connection","entry":"dev.sailry.platform/card.js","mcp_server":"input","props_schema":{"type":"object"}}],
			"connectors":[{"id":"account","display":{"label":"Account"},"accounts":"single","origins":["https://example.test"],
				"auth":[{"kind":"mcp_oauth","server":"input"},{"kind":"secret","fields":["token"]}],"inject":[]}]
		}`), &extension); err != nil {
			return err
		}
		info.Extension = &extension
		info.Mcp = []protocol.PluginMcp{{Name: "input", Transport: protocol.PluginMcpTransportStreamableHttp, Availability: protocol.PluginMcpAvailabilityAvailable}}
		info, err := InstallPlugin(ctx, db, info, 0, true)
		if err != nil {
			return err
		}
		reference := plugins.PackageReference(info)
		status, err := ReadPluginConnection(ctx, db, profile, reference, "connection")
		if err != nil || status.Configuration != protocol.PluginConnectionConfigurationReady ||
			status.Authorization != protocol.PluginConnectionAuthorizationMissing || status.Credential != protocol.PluginConnectionCredentialNotDeclared {
			return fmt.Errorf("missing connection classification differs: %v, %#v", err, status)
		}
		connection := protocol.PluginConnectorID{1}
		record := ConnectorRecord{
			ID: connection, Plugin: reference.Name, Connector: "account", Revision: 1, AuthRevision: 1, GrantRevision: 1,
			Method: protocol.PluginConnectorKindMcpOauth, Enabled: true, State: protocol.PluginConnectorStateConnected,
			Fields: map[string]string{"label": "private-label"}, Secrets: map[string]protocol.Secret{"token": protocol.NewSecret("private-token")},
			Grant: &authorization.Issued{Access: "private-access", Extra: map[string]string{"opaque": "private-extra"}},
		}
		for _, scenario := range []struct {
			name          string
			change        func(*ConnectorRecord)
			configuration protocol.PluginConnectionConfiguration
			authorization protocol.PluginConnectionAuthorization
		}{
			{"connected", func(record *ConnectorRecord) {}, protocol.PluginConnectionConfigurationReady, protocol.PluginConnectionAuthorizationConfigured},
			{"disabled connection", func(record *ConnectorRecord) { record.Enabled = false }, protocol.PluginConnectionConfigurationReady, protocol.PluginConnectionAuthorizationMissing},
			{"missing grant", func(record *ConnectorRecord) { record.Grant = nil }, protocol.PluginConnectionConfigurationReady, protocol.PluginConnectionAuthorizationMissing},
			{"empty grant", func(record *ConnectorRecord) { record.Grant.Access = "" }, protocol.PluginConnectionConfigurationReady, protocol.PluginConnectionAuthorizationMissing},
			{"expired grant", func(record *ConnectorRecord) { record.Grant.ExpiresAtMs = ptr(uint64(1)) }, protocol.PluginConnectionConfigurationReady, protocol.PluginConnectionAuthorizationMissing},
			{"pending login", func(record *ConnectorRecord) { attempt := protocol.NewRequestID(); record.Login = &attempt }, protocol.PluginConnectionConfigurationReady, protocol.PluginConnectionAuthorizationMissing},
			{"not connected", func(record *ConnectorRecord) { record.State = protocol.PluginConnectorStateNotConnected }, protocol.PluginConnectionConfigurationReady, protocol.PluginConnectionAuthorizationMissing},
			{"consent", func(record *ConnectorRecord) { record.Bound.Origins = []string{"https://previous.test"} }, protocol.PluginConnectionConfigurationChanged, protocol.PluginConnectionAuthorizationMissing},
			{"secret method", func(record *ConnectorRecord) { record.Method, record.Bound = protocol.PluginConnectorKindSecret, nil }, protocol.PluginConnectionConfigurationReady, protocol.PluginConnectionAuthorizationNotDeclared},
		} {
			candidate := record
			grant := *record.Grant
			candidate.Grant = &grant
			candidate.Bound = connectorBound(info, info.Extension.Connectors[0], candidate.Method)
			scenario.change(&candidate)
			if err := saveConnectorRecord(ctx, db, candidate); err != nil {
				return err
			}
			status, err := ReadPluginConnection(ctx, db, profile, reference, "connection")
			if err != nil || status.Package != reference || status.Card != "connection" || status.Server != "input" ||
				status.Configuration != scenario.configuration || status.Authorization != scenario.authorization ||
				status.Credential != protocol.PluginConnectionCredentialNotDeclared {
				return fmt.Errorf("%s classification differs: %v, %#v", scenario.name, err, status)
			}
			body, err := json.Marshal(status)
			if err != nil || bytes.Contains(body, []byte("private-")) {
				return fmt.Errorf("%s public connection exposed private data: %v", scenario.name, err)
			}
		}
		record.Bound = connectorBound(info, info.Extension.Connectors[0], record.Method)
		if err := saveConnectorRecord(ctx, db, record); err != nil {
			return err
		}
		second := record
		second.ID = protocol.PluginConnectorID{2}
		if err := saveConnectorRecord(ctx, db, second); err != nil {
			return err
		}
		record.Enabled = false
		if err := saveConnectorRecord(ctx, db, record); err != nil {
			return err
		}
		status, err = ReadPluginConnection(ctx, db, profile, reference, "connection")
		if err != nil || status.Authorization != protocol.PluginConnectionAuthorizationMissing {
			return fmt.Errorf("classification skipped the first canonical connection: %v, %#v", err, status)
		}
		if _, err := ReadPluginConnection(ctx, db, profile, reference, "missing"); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("undeclared card status was exposed: %v", err)
		}
		withoutServer := info
		changedExtension := *info.Extension
		changedExtension.Cards = append([]protocol.PluginCardDeclaration{}, info.Extension.Cards...)
		changedExtension.Cards[0].McpServer = nil
		withoutServer.Extension = &changedExtension
		if _, err := ReadPluginConnectionAdmitted(ctx, db, profile, withoutServer, "connection"); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
			return fmt.Errorf("card without an MCP server was exposed: %v", err)
		}
		changedExtension.Cards[0].McpServer = info.Extension.Cards[0].McpServer
		changedExtension.Connectors = nil
		if _, err := ReadPluginConnectionAdmitted(ctx, db, profile, withoutServer, "connection"); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
			return fmt.Errorf("card without a connector was exposed: %v", err)
		}
		request := protocol.NewRequest(node, protocol.Command{Kind: "read_plugin_connection", Data: &protocol.CommandReadPluginConnectionPayload{Package: reference, Card: "connection"}})
		scope := protocol.DefaultPluginContext()
		scope.Package = reference
		request.Plugin = &scope
		if err := CheckPluginConnectionRequest(request, info); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("unadmitted plugin read gained connection status: %v", err)
		}
		turn := protocol.NewTurnID()
		scope.Turn = &turn
		if err := CheckPluginConnectionRequest(request, info); err != nil {
			return fmt.Errorf("owned admitted HTTP card was refused: %v", err)
		}
		for _, scenario := range []string{"other package", "other card", "stdio", "missing server"} {
			changed := request
			input := *request.Command.Data.(*protocol.CommandReadPluginConnectionPayload)
			changed.Command.Data = &input
			packageInfo := info
			switch scenario {
			case "other package":
				input.Package.Name = "other"
			case "other card":
				input.Card = "other"
			case "stdio":
				packageInfo.Mcp = []protocol.PluginMcp{{Name: "input", Transport: protocol.PluginMcpTransportStdio}}
			case "missing server":
				packageInfo.Mcp = nil
			}
			if err := CheckPluginConnectionRequest(changed, packageInfo); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
				return fmt.Errorf("%s escaped the connection status fence: %v", scenario, err)
			}
		}
		scope.Turn = nil
		invocation := protocol.NewRequestID()
		scope.Invocation = &invocation
		if err := CheckPluginConnectionRequest(request, info); err != nil {
			return fmt.Errorf("owned callback card was refused: %v", err)
		}
		current := info
		current.Summary.SettingsRevision++
		if err := savePluginInventory(ctx, db, current, true); err != nil {
			return err
		}
		if _, err := ReadPluginConnection(ctx, db, profile, reference, "connection"); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
			return fmt.Errorf("controller connection read accepted a stale package: %v", err)
		}
		status, err = ReadPluginConnectionAdmitted(ctx, db, profile, info, "connection")
		if err != nil || status.Package != reference {
			return fmt.Errorf("admitted connection read lost its package capture: %v, %#v", err, status)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
