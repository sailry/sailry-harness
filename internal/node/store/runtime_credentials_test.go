package store

import (
	"database/sql"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPairedUsersConnectorRenewsOnExecutionNode(t *testing.T) {
	t.Parallel()
	storage, node, _, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("runtime-connector", 'd')
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		origin, header, server := "https://mcp.example.test", "Authorization", "server"
		declaration := protocol.PluginConnectorDeclaration{
			ID: "account", Accounts: protocol.PluginConnectorAccountsSingle,
			Origins: []string{origin}, Fields: []protocol.PluginConnectorField{},
			Auth:   []protocol.PluginConnectorAuth{{Kind: "mcp_oauth", Data: protocol.PluginConnectorAuthMcpOauthPayload{Server: server}}},
			Inject: []protocol.PluginConnectorInjection{{Server: &server, Header: &header, Prefix: "Bearer "}},
		}
		extension.Connectors, info.Extension = []protocol.PluginConnectorDeclaration{declaration}, &extension
		if _, err := InstallPlugin(ctx, db, info, 0, true); err != nil {
			return err
		}
		refresh := "never-copy-refresh"
		expiry := uint64(time.Now().Add(time.Hour).UnixMilli())
		record := ConnectorRecord{
			ID: protocol.NewRequestID(), Plugin: info.Summary.Name, Connector: declaration.ID,
			Method: protocol.PluginConnectorKindMcpOauth, Revision: 11, AuthRevision: 7, GrantRevision: 9, Enabled: true,
			State: protocol.PluginConnectorStateConnected, Fields: map[string]string{}, Secrets: map[string]protocol.Secret{},
			Grant: &authorization.Issued{Access: "mcp-runtime-access", Refresh: &refresh, ExpiresAtMs: &expiry,
				Extra: map[string]string{"mcp_oauth": `{"Config":{"ClientSecret":"never-copy-client-secret"},"Token":{"RefreshToken":"never-copy-nested-refresh"}}`, "unused": "never-copy-unused"}},
		}
		if err := saveConnectorRecord(ctx, db, record); err != nil {
			return err
		}
		renewals, err := ReadCredentialRenewals(ctx, db, node)
		if err != nil || len(renewals) != 1 || renewals[0].Connection == nil || renewals[0].Connection.ID != record.ID {
			return fmt.Errorf("the paired user's local connector did not own its renewal: %v", err)
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

func TestForwardedSessionAdmitsExpiredSameAccountCapture(t *testing.T) {
	t.Parallel()
	storage, node, controller, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		provider := providerFixture()
		provider.Revision, provider.Authentication = 1, protocol.AuthenticationClaudeOAuth
		provider.Api, provider.Endpoint = protocol.ConversationModelApiAnthropic, "https://api.anthropic.com"
		reference := protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
		provider.Credential = &reference
		expired := uint64(1)
		runtime := protocol.RuntimeCredential{
			Controller: controller, Kind: "provider", ID: reference.ID.String(), Generation: reference.ID.String(),
			Revision: 1, State: "ready", Provider: &provider.ID, Authentication: provider.Authentication,
			Endpoint: provider.Endpoint, Access: protocol.NewSecret("expired-runtime"), ExpiresAtMs: &expired,
		}
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Credential = provider.ID, "fixture", &reference
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
		capture := protocol.SessionProfile{Source: controller, Provider: provider}
		for _, fixture := range []struct {
			name, state, generation string
			code                    protocol.ErrorCode
		}{
			{"expired same account", "ready", reference.ID.String(), ""},
			{"revoked account", "revoked", reference.ID.String(), protocol.ErrorCodeNotConfigured},
			{"changed account", "ready", protocol.NewCredentialID().String(), protocol.ErrorCodePermissionDenied},
		} {
			runtime.State, runtime.Generation = fixture.state, fixture.generation
			if err := seedHistoricalRuntimeCredentials(ctx, db, runtime); err != nil {
				return err
			}
			err := validateCapturedConfig(ctx, db, node, capture, config)
			if fixture.code != "" {
				if !providerFixtureFault(err, fixture.code) {
					return fmt.Errorf("%s captured config validation: %v", fixture.name, err)
				}
			} else {
				if err != nil {
					return fmt.Errorf("same-account expiry rejected captured session: %w", err)
				}
				if _, err := ReadRuntimeProviderCredential(ctx, db, node, provider, reference); !providerFixtureFault(err, protocol.ErrorCodeExpired) {
					return fmt.Errorf("admission made expired access usable: %v", err)
				}
			}
			runtime.Revision++
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
