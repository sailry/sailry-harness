package store

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRuntimeCredentialExportIncludesCapturedAccountsWithoutRenewalMaterial(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		provider := providerFixture()
		provider.Authentication, provider.Endpoint = protocol.AuthenticationClaudeOAuth, "https://api.anthropic.com"
		provider.Api = protocol.ConversationModelApiAnthropic
		account := "account-fixture"
		grant := providers.AccountGrant{
			Kind: provider.Authentication, Access: "runtime-access", Refresh: "never-copy-refresh", IDToken: "never-copy-identity",
			ClientID: "never-copy-client", Subject: "never-copy-subject", Github: "never-copy-github", AccountID: &account,
			ExpiresAtMs: uint64(time.Now().Add(time.Hour).UnixMilli()),
		}
		body, _ := json.Marshal(grant)
		key := protocol.NewSecret(string(body))
		first, second := protocol.NewCredentialID(), protocol.NewCredentialID()
		provider.Credential = &protocol.CredentialRef{Node: node, ID: second}
		if err := saveProviderConfiguration(ctx, db, providerConfiguration{ID: provider.ID, Listed: true, Provider: &provider,
			Authorizations: map[protocol.CredentialID]providerAuthorization{
				first:  {Authentication: provider.Authentication, Revision: 4, Key: &key},
				second: {Authentication: provider.Authentication, Revision: 1, Key: &key},
			}}); err != nil {
			return err
		}
		records, err := ExportRuntimeCredentials(ctx, db, node)
		if err != nil || len(records) != 2 {
			return fmt.Errorf("runtime accounts missing retained generation: %d, %w", len(records), err)
		}
		encoded, _ := json.Marshal(records)
		if bytes.Contains(encoded, []byte("never-copy-")) {
			return fmt.Errorf("runtime export copied renewal material")
		}
		for _, record := range records {
			var value protocol.RuntimeCredential
			if json.Unmarshal(record.Body, &value) != nil || value.Controller != node || value.Generation != value.ID || value.Access.Expose() != grant.Access {
				return fmt.Errorf("runtime identity or token was lost")
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestRuntimeMCPExportOmitsOAuthClientAndNestedToken(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		info := pluginTestInfo("runtime-connector", 'd')
		extension := protocol.DefaultPluginExtension()
		origin, header, server := "https://mcp.example.test", "Authorization", "server"
		declaration := protocol.PluginConnectorDeclaration{
			ID: "account", Accounts: protocol.PluginConnectorAccountsSingle,
			Origins: []string{origin}, Fields: []protocol.PluginConnectorField{},
			Auth:   []protocol.PluginConnectorAuth{{Kind: "mcp_oauth", Data: protocol.PluginConnectorAuthMcpOauthPayload{Server: server}}},
			Inject: []protocol.PluginConnectorInjection{{Server: &server, Header: &header, Prefix: "Bearer "}},
		}
		extension.Connectors, info.Extension = []protocol.PluginConnectorDeclaration{declaration}, &extension
		if _, err := InstallPlugin(t.Context(), db, info, 0, true); err != nil {
			return err
		}
		refresh := "never-copy-refresh"
		expiry := uint64(time.Now().Add(time.Hour).UnixMilli())
		record := ConnectorRecord{
			ID: protocol.NewRequestID(), Plugin: info.Summary.Name, Connector: declaration.ID,
			Method: protocol.PluginConnectorKindMcpOauth, Revision: 11, AuthRevision: 7, GrantRevision: 9, Enabled: true,
			Grant: &authorization.Issued{Access: "mcp-runtime-access", Refresh: &refresh, ExpiresAtMs: &expiry,
				Extra: map[string]string{"mcp_oauth": `{"Config":{"ClientSecret":"never-copy-client-secret"},"Token":{"RefreshToken":"never-copy-nested-refresh"}}`, "unused": "never-copy-unused"}},
		}
		value, err := runtimeConnector(t.Context(), db, record)
		if err != nil {
			return err
		}
		body, _ := json.Marshal(value)
		if bytes.Contains(body, []byte("never-copy-")) || value.Access.Expose() != "mcp-runtime-access" || value.Generation != "7" || value.Revision != 11 {
			return fmt.Errorf("MCP execution export copied client or renewal state")
		}
		record.Method = protocol.PluginConnectorKindLocalCli
		value, err = runtimeConnector(t.Context(), db, record)
		if err != nil || value.State != "machine_bound" || value.Access.Expose() != "" {
			return fmt.Errorf("local CLI authentication was exported: %w", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestRuntimeCredentialStatesAndConnectorRefreshTime(t *testing.T) {
	t.Parallel()
	expired, future := uint64(1), uint64(time.Now().Add(time.Hour).UnixMilli())
	for _, fixture := range []struct {
		state  string
		expiry *uint64
		code   protocol.ErrorCode
	}{
		{"ready", &expired, protocol.ErrorCodeExpired},
		{"uncertain", &future, ""},
		{"uncertain", &expired, protocol.ErrorCodeOutcomeUnknown},
		{"rejected", &future, protocol.ErrorCodePermissionDenied},
		{"revoked", &future, protocol.ErrorCodeNotConfigured},
		{"machine_bound", nil, protocol.ErrorCodeNotConfigured},
	} {
		err := runtimeCredentialReady(protocol.RuntimeCredential{State: fixture.state, ExpiresAtMs: fixture.expiry})
		if fixture.code == "" {
			if err != nil {
				t.Fatal(err)
			}
		} else if !providerFixtureFault(err, fixture.code) {
			t.Fatalf("runtime state %s returned %v", fixture.state, err)
		}
	}
	issued, expiry, refresh := uint64(1_000_000), uint64(5_000_000), "refresh"
	record := ConnectorRecord{Method: protocol.PluginConnectorKindOauth2, Grant: &authorization.Issued{
		Access: "runtime", Refresh: &refresh, IssuedAtMs: &issued, ExpiresAtMs: &expiry,
	}}
	if due := ConnectorRenewalAt(record); due == nil || *due != 2_000_000 {
		t.Fatalf("connector quarter lifetime=%v", due)
	}
	record.Grant.IssuedAtMs = nil
	if due := ConnectorRenewalAt(record); due == nil || *due != expiry {
		t.Fatal("legacy connector deadline was guessed")
	}
	record.Grant.Refresh = nil
	if ConnectorRenewalAt(record) != nil {
		t.Fatal("nonrenewable connector was scheduled")
	}
}
