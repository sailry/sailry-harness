package node

import (
	"database/sql"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestRetainedMCPAuthorization(t *testing.T) {
	for _, scenario := range []string{"disabled connection", "changed credential", "withdrawn server permission"} {
		t.Run(scenario, func(t *testing.T) {
			node := nodeFixture(t)
			ctx := t.Context()
			source := t.TempDir()
			documents := map[string]string{
				"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"mcp-auth-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["http.request"],"connectors":[{"id":"account","display":{"label":"Fixture account"},"accounts":"single","origins":["https://fixture.test"],"auth":[{"kind":"secret","fields":["token"]}],"fields":[{"name":"token","title":"Token","secret":true}],"inject":[{"server":"server","header":"Authorization","prefix":"Bearer ","field":"token"}]}]}}}`,
				"mcp.json":    `{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"server":{"type":"streamable-http","url":"https://fixture.test/mcp"}}}`,
			}
			for name, document := range documents {
				if err := os.WriteFile(filepath.Join(source, name), []byte(document), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			info, err := node.plugins.manager.Install(ctx, source, "", "mcp-auth-fixture")
			if err != nil || len(info.Issues) != 0 {
				t.Fatalf("MCP connector fixture package: %v, %+v", err, info.Issues)
			}
			var connection protocol.PluginConnectorConnection
			err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				var err error
				info, err = store.InstallPlugin(ctx, db, info, 0, true)
				if err != nil {
					return err
				}
				capture, err := store.PrepareConnectorLogin(ctx, db, protocol.NewRequestID(), protocol.CommandStartConnectorLoginPayload{
					Package: plugins.PackageReference(info), Connector: "account", Method: protocol.PluginConnectorKindSecret,
					Input: protocol.PluginConnectorInput{Fields: map[string]string{}, Secrets: map[string]protocol.Secret{"token": protocol.NewSecret("captured-token")}},
				})
				if err != nil {
					return err
				}
				if err := store.StartConnectorLogin(ctx, db, capture); err != nil {
					return err
				}
				connection, err = store.CompleteConnectorLogin(ctx, db, capture, nil, nil, false)
				return err
			})
			if err != nil {
				t.Fatal(err)
			}
			info, settings, configuration, err := node.capturedPluginMCP(ctx, plugins.PackageReference(info), true)
			if err != nil {
				t.Fatal(err)
			}
			prepared, err := node.prepareMCPLaunch(ctx, info, settings, "server", configuration.Servers["server"])
			if err != nil || prepared.Validate == nil || prepared.Launch.Headers["Authorization"] != "Bearer captured-token" {
				t.Fatalf("MCP authorization capture failed: %v", err)
			}
			if err := prepared.Validate(ctx); err != nil {
				t.Fatal(err)
			}
			// What a caller adds to the prepared launch afterwards, as the
			// computer helper's session variable, is not a changed credential.
			prepared.Launch.Headers["X-Test-Session"] = "run-1"
			if err := prepared.Validate(ctx); err != nil {
				t.Fatalf("a value added after authorization failed validation: %v", err)
			}
			err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				switch scenario {
				case "disabled connection":
					_, err := store.ExecuteConnectorCommand(ctx, db, protocol.Command{Kind: "set_connection_enabled", Data: &protocol.CommandSetConnectionEnabledPayload{
						Connection: connection.ID, ExpectedRevision: connection.Revision, Enabled: false,
					}}, nil)
					return err
				case "changed credential":
					_, err := store.ExecuteConnectorCommand(ctx, db, protocol.Command{Kind: "update_connection", Data: &protocol.CommandUpdateConnectionPayload{
						Connection: connection.ID, ExpectedRevision: connection.Revision, Input: protocol.PluginConnectorInput{Fields: map[string]string{}, Secrets: map[string]protocol.Secret{"token": protocol.NewSecret("replacement-token")}},
					}}, nil)
					return err
				default:
					_, err := store.SetPluginMCPAllowed(ctx, db, protocol.CommandSetPluginMcpAllowedPayload{Package: plugins.PackageReference(info), Server: "server", Allowed: false})
					return err
				}
			})
			if err != nil {
				t.Fatal(err)
			}
			if err := prepared.Validate(ctx); err == nil {
				t.Fatal("retained MCP launch accepted invalidated authorization")
			}
			if prepared.Launch.Headers["Authorization"] != "Bearer captured-token" {
				t.Fatal("validation mutated the captured MCP launch")
			}
		})
	}
}

// A connector whose every credential is optional starts its server without
// an account; one that requires a credential waits for its account.
func TestOptionalConnectorMCPWithoutAccount(t *testing.T) {
	for _, optional := range []string{"true", "false"} {
		t.Run("optional "+optional, func(t *testing.T) {
			node := nodeFixture(t)
			ctx := t.Context()
			source := t.TempDir()
			documents := map[string]string{
				"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"mcp-optional-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["http.request"],"connectors":[{"id":"account","display":{"label":"Fixture account"},"accounts":"single","origins":["https://fixture.test"],"auth":[{"kind":"secret","fields":["token"]}],"fields":[{"name":"token","title":"Token","secret":true,"optional":` + optional + `}],"inject":[{"server":"server","header":"X-Fixture-Key"}]}]}}}`,
				"mcp.json":    `{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"server":{"type":"streamable-http","url":"https://fixture.test/mcp"}}}`,
			}
			for name, document := range documents {
				if err := os.WriteFile(filepath.Join(source, name), []byte(document), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			info, err := node.plugins.manager.Install(ctx, source, "", "mcp-optional-fixture")
			if err != nil || len(info.Issues) != 0 {
				t.Fatalf("MCP connector fixture package: %v, %+v", err, info.Issues)
			}
			err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				var err error
				info, err = store.InstallPlugin(ctx, db, info, 0, true)
				return err
			})
			if err != nil {
				t.Fatal(err)
			}
			info, settings, configuration, err := node.capturedPluginMCP(ctx, plugins.PackageReference(info), true)
			if err != nil {
				t.Fatal(err)
			}
			prepared, err := node.prepareMCPLaunch(ctx, info, settings, "server", configuration.Servers["server"])
			if optional == "false" {
				if nodeFault(err).(*protocol.Fault).Code != protocol.ErrorCodeNotConfigured {
					t.Fatalf("a server that requires an account started without one: %v", err)
				}
				return
			}
			if err != nil || len(prepared.Launch.Headers) != 0 {
				t.Fatalf("a server with an optional credential did not start without an account: %v", err)
			}
			if err := prepared.Validate(ctx); err != nil {
				t.Fatal(err)
			}
		})
	}
}
