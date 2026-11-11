package store

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConnectorLifecycleAndConsent(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("connector-fixture", 'a')
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		secretField := "token"
		declaration := protocol.DefaultPluginConnectorDeclaration()
		declaration.ID, declaration.Display.Label, declaration.Accounts = "account", "Fixture service", protocol.PluginConnectorAccountsSingle
		declaration.Origins = []string{"https://api.example.test"}
		declaration.Fields = []protocol.PluginConnectorField{{Name: "note", Title: "Note", Optional: true}, {Name: secretField, Title: "Token", Secret: true}}
		declaration.Auth = []protocol.PluginConnectorAuth{{Kind: "secret", Data: protocol.PluginConnectorAuthSecretPayload{Fields: []string{secretField}}}}
		declaration.Inject = []protocol.PluginConnectorInjection{{Header: &secretField, Prefix: "Bearer ", Field: &secretField}}
		extension.Connectors, info.Extension = []protocol.PluginConnectorDeclaration{declaration}, &extension
		info, err := InstallPlugin(ctx, db, info, 0, true)
		if err != nil {
			return err
		}
		input := protocol.CommandStartConnectorLoginPayload{Package: plugins.PackageReference(info), Connector: declaration.ID, Method: protocol.PluginConnectorKindSecret,
			Input: protocol.PluginConnectorInput{Fields: map[string]string{"note": "first"}, Secrets: map[string]protocol.Secret{secretField: protocol.NewSecret("protected-token")}}}
		capture, err := PrepareConnectorLogin(ctx, db, protocol.NewRequestID(), input)
		if err != nil {
			return err
		}
		if err := StartConnectorLogin(ctx, db, capture); err != nil {
			return err
		}
		identity := &protocol.PluginConnectorIdentity{ID: "account-id", Name: "Account"}
		value, err := CompleteConnectorLogin(ctx, db, capture, nil, identity, false)
		if err != nil || value.State != protocol.PluginConnectorStateConnected || len(value.Configured) != 1 {
			return fmt.Errorf("connector did not connect: %v, %#v", err, value)
		}
		public, _ := json.Marshal(value)
		if bytes.Contains(public, []byte("protected-token")) {
			return fmt.Errorf("connector projection exposed a credential")
		}
		before, err := ReadConnectorRecord(ctx, db, value.ID)
		if err != nil {
			return err
		}
		command := protocol.Command{Kind: "update_connection", Data: &protocol.CommandUpdateConnectionPayload{Connection: value.ID, ExpectedRevision: value.Revision,
			Input: protocol.PluginConnectorInput{Fields: map[string]string{"note": "second"}, Secrets: map[string]protocol.Secret{}}}}
		if _, err := ExecuteConnectorCommand(ctx, db, command, nil); err != nil {
			return err
		}
		after, err := ReadConnectorRecord(ctx, db, value.ID)
		if err != nil || before.AuthRevision != after.AuthRevision || after.Secrets[secretField].Expose() != "protected-token" {
			return fmt.Errorf("metadata edit changed authorization or removed a kept secret: %v", err)
		}
		command.Data = &protocol.CommandUpdateConnectionPayload{Connection: value.ID, ExpectedRevision: after.Revision, Input: protocol.PluginConnectorInput{
			Fields: map[string]string{}, Secrets: map[string]protocol.Secret{secretField: protocol.NewSecret("replacement-token")}}}
		if _, err := ExecuteConnectorCommand(ctx, db, command, nil); err != nil {
			return err
		}
		after, err = ReadConnectorRecord(ctx, db, value.ID)
		if err != nil || after.AuthRevision == before.AuthRevision || after.State != protocol.PluginConnectorStateNotConnected {
			return fmt.Errorf("credential edit retained the prior authorization: %v", err)
		}
		previous := &ConnectorBound{Origins: []string{"https://old.example.test"}, Targets: map[string]protocol.PluginMcpDefinition{
			"server": {Kind: "streamable-http", Data: protocol.PluginMcpDefinitionStreamableHttpPayload{Url: "https://old.example.test/mcp", Headers: []string{"Authorization"}}},
		}}
		current := &ConnectorBound{Origins: []string{"https://new.example.test"}, Targets: map[string]protocol.PluginMcpDefinition{
			"server": {Kind: "streamable-http", Data: protocol.PluginMcpDefinitionStreamableHttpPayload{Url: "https://new.example.test/mcp", Headers: []string{"Authorization"}}},
		}}
		consent := connectorConsent(previous, current)
		if consent == nil || len(consent.Changes) != 2 || consent.Changes[0].Kind != "origins" || consent.Changes[1].Kind != "mcp" {
			return fmt.Errorf("changed origins and MCP destinations did not require consent: %#v", consent)
		}
		attempt := protocol.NewRequestID()
		after.Login, after.State = &attempt, protocol.PluginConnectorStateConnecting
		if err := saveConnectorRecord(ctx, db, after); err != nil {
			return err
		}
		if err := RecoverConnectorLogins(ctx, db); err != nil {
			return err
		}
		after, err = ReadConnectorRecord(ctx, db, value.ID)
		if err != nil || after.Login != nil || after.State != protocol.PluginConnectorStateNotConnected {
			return fmt.Errorf("interrupted connector authorization remained running: %v", err)
		}
		command = protocol.Command{Kind: "remove_connection", Data: &protocol.CommandRemoveConnectionPayload{Connection: after.ID, ExpectedRevision: after.Revision}}
		if _, err := ExecuteConnectorCommand(ctx, db, command, nil); err != nil {
			return err
		}
		values, err := Connections(ctx, db)
		if err != nil || len(values) != 0 {
			return fmt.Errorf("removed connector remained visible: %v", err)
		}
		// Removing a standalone MCP package takes the connection only it can use;
		// a package with declared connectors keeps its own for a later install.
		standalone := pluginTestInfo("mcp-standalone", 'b')
		standalone.McpSource = &protocol.PluginMcpDefinition{Kind: "streamable-http", Data: protocol.PluginMcpDefinitionStreamableHttpPayload{Url: "https://mcp.example.test/mcp", Headers: []string{}}}
		if standalone, err = InstallPlugin(ctx, db, standalone, 0, true); err != nil {
			return err
		}
		kept, owned := after, after
		owned.ID, owned.Plugin, owned.Connector = protocol.NewRequestID(), standalone.Summary.Name, "server"
		for _, record := range []ConnectorRecord{kept, owned} {
			if err := saveConnectorRecord(ctx, db, record); err != nil {
				return err
			}
		}
		for _, removed := range []protocol.PluginInfo{info, standalone} {
			if _, err := RemovePlugin(ctx, db, removed.Summary.Name, removed.Summary.Revision); err != nil {
				return err
			}
		}
		if _, err := ReadConnectorRecord(ctx, db, kept.ID); err != nil {
			return fmt.Errorf("a removed package lost the connection of its declared connector: %v", err)
		}
		if _, err := ReadConnectorRecord(ctx, db, owned.ID); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("a removed standalone MCP package kept its connection: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
