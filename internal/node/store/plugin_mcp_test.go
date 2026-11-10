package store

import (
	"database/sql"
	"encoding/json"
	"errors"
	"net/url"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMCPConfigurationAuthority(t *testing.T) {
	t.Run("bound secret destination and explicit allow revision", func(t *testing.T) {
		_, _, storage := schemaFixture(t)
		ctx := t.Context()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			config := protocol.PluginMcpConfiguration{Schema: pluginMCPSchema, Servers: map[string]protocol.PluginMcpServer{"server": {
				Kind: "streamable-http", Data: &protocol.PluginMcpServerHttpPayload{Url: "https://example.test/mcp", Headers: map[string]string{"Authorization": ""}, Reuse: true},
			}}}
			schema, err := plugins.ValidateSettings([]byte(`{"$schema":"`+plugins.SettingsSchema+`","type":"object","additionalProperties":false,"properties":{"token":{"type":"string","x-sailry-secret":{"server":"server","header":"Authorization"}}}}`), config.Servers)
			if err != nil {
				return err
			}
			info := pluginTestInfo("mcp-fixture", 'a')
			info.Settings = &schema
			info.Mcp = []protocol.PluginMcp{{Name: "server", Transport: protocol.PluginMcpTransportStreamableHttp, Allowed: true, Availability: protocol.PluginMcpAvailabilityAvailable}}
			info, err = InstallPlugin(ctx, db, info, 0, true)
			if err != nil {
				return err
			}
			settings, err := SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{
				Package: plugins.PackageReference(info), Values: map[string]json.RawMessage{},
				Secrets: map[string]protocol.PluginSettingsSecretUpdate{"token": {Kind: "replace", Data: protocol.NewSecret("captured-secret")}},
			}, MCPSettingsTargets(info.Settings, config))
			if err != nil {
				return err
			}
			changed := protocol.PluginMcpConfiguration{Schema: pluginMCPSchema, Servers: map[string]protocol.PluginMcpServer{"server": {
				Kind: "streamable-http", Data: &protocol.PluginMcpServerHttpPayload{Url: "https://other.test/mcp", Headers: map[string]string{"Authorization": ""}, Reuse: true},
			}}}
			if _, err := SavePluginMCP(ctx, db, protocol.CommandSavePluginMcpPayload{Package: settings.Package, Configuration: changed}, config); err == nil {
				return errors.New("raw configuration changed a bound credential destination")
			}
			info, err = SetPluginMCPAllowed(ctx, db, protocol.CommandSetPluginMcpAllowedPayload{Package: settings.Package, Server: "server", Allowed: false})
			if err != nil {
				return err
			}
			revision := info.Summary.SettingsRevision
			info, err = SetPluginMCPAllowed(ctx, db, protocol.CommandSetPluginMcpAllowedPayload{Package: plugins.PackageReference(info), Server: "server", Allowed: false})
			if err != nil || info.Summary.SettingsRevision != revision {
				return errors.New("unchanged MCP allowance rewrote configuration")
			}
			captured, err := ResolvePluginSettings(ctx, db, settings.Package)
			if err != nil || captured.Secrets["token"].Expose() != "captured-secret" {
				return errors.New("allowance update lost an admitted credential capture")
			}
			current, err := ResolvePluginSettings(ctx, db, plugins.PackageReference(info))
			if err != nil || current.MCPAllowed["server"] {
				return errors.New("MCP allowance was not persisted")
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("primitive forms preserve constraints and ordering", func(t *testing.T) {
		schema := json.RawMessage(`{"type":"object","propertyOrder":["email","kind"],"properties":{"kind":{"type":"string","enum":["review","ship"],"enumNames":["Review","Ship"]},"email":{"type":"string","format":"email"},"count":{"type":"integer","minimum":1,"maximum":3},"targets":{"type":"array","items":{"anyOf":[{"const":"one","title":"One"},{"const":"two","title":"Two"}]},"minItems":1,"maxItems":2}},"required":["email","kind"],"additionalProperties":false}`)
		spec, err := MCPFormSpec("Choose delivery", schema)
		if err != nil {
			t.Fatal(err)
		}
		fields := spec.Input.Data.(*protocol.ConversationQuestionInputFormPayload).Fields
		if len(fields) != 4 || fields[0].Name != "email" || fields[1].Name != "kind" || fields[2].Name != "count" {
			t.Fatalf("form order = %+v", fields)
		}
		validator, err := compileMCPForm(schema)
		if err != nil {
			t.Fatal(err)
		}
		if err := validator.Validate(map[string]any{"email": "person@example.test", "kind": "review", "count": 2}); err != nil {
			t.Fatal(err)
		}
		for _, values := range []map[string]any{
			{"email": "invalid", "kind": "review"},
			{"email": "person@example.test", "kind": "unknown"},
			{"email": "person@example.test", "kind": "review", "count": 4},
		} {
			if validator.Validate(values) == nil {
				t.Fatalf("invalid form answer accepted: %+v", values)
			}
		}
		if _, err := MCPFormSpec("Nested", json.RawMessage(`{"type":"object","properties":{"nested":{"type":"object"}}}`)); err == nil {
			t.Fatal("unsupported nested form was accepted")
		}
	})
	t.Run("form schemas cannot load host files", func(t *testing.T) {
		filename := filepath.Join(t.TempDir(), "external-schema.json")
		if err := os.WriteFile(filename, []byte(`{"type":"object"}`), 0o600); err != nil {
			t.Fatal(err)
		}
		path := filepath.ToSlash(filename)
		if path[0] != '/' {
			path = "/" + path
		}
		address := (&url.URL{Scheme: "file", Path: path}).String()
		body, err := json.Marshal(map[string]any{"type": "object", "properties": map[string]any{"name": map[string]any{"type": "string"}}, "$ref": address})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := MCPFormSpec("Choose", body); err == nil {
			t.Fatal("MCP form loaded an external host schema")
		}
	})
}
