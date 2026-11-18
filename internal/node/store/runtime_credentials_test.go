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
		records, err := exportRuntimeCredentials(ctx, db, node, map[protocol.CredentialID]runtimeCapture{first: {}, second: {}})
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
		// Simulate the controller's last pre-upgrade export to one host.
		host := protocol.NodeID{31}
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		digest, err := configurationManifestDigest(records)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO configuration_follow_versions(user,source,category,revision,digest) VALUES(?1,?2,'runtime_credentials',1,?3)", user.ID.String(), host[:], digest); err != nil {
			return err
		}
		known := protocol.ConfigurationManifest{Source: node, Category: "runtime_credentials", Revision: 1, Records: records}
		// An older host can keep its existing material, but no fresh token is sent.
		legacy, err := ExportConfigurationManifest(ctx, db, node, host, "runtime_credentials", true, nil, &known)
		if err != nil || len(legacy.Records) != 2 || len(legacy.Records[0].Body)+len(legacy.Records[1].Body) != 0 {
			return fmt.Errorf("legacy peer did not keep metadata-only records: %v", err)
		}
		known.Captured = []protocol.CapturedModelCredential{{ID: first}}
		other, err := ExportConfigurationManifest(ctx, db, node, protocol.NodeID{32}, "runtime_credentials", true, nil, &known)
		if err != nil || len(other.Records) != 0 {
			return fmt.Errorf("a different host obtained the historical account: %v", err)
		}
		only, err := ExportConfigurationManifest(ctx, db, node, host, "runtime_credentials", true, nil, &known)
		if err != nil || len(only.Records) != 1 || only.Records[0].ID != "provider:"+first.String() {
			return fmt.Errorf("a host received a credential outside its turn: %v", err)
		}
		known = only
		known.Captured = []protocol.CapturedModelCredential{{ID: first, Waiting: true}}
		waiting, err := ExportConfigurationManifest(ctx, db, node, host, "runtime_credentials", true, nil, &known)
		if err != nil || len(waiting.Records) != 1 {
			return fmt.Errorf("replacement without trusted workspace was exported: %v", err)
		}
		var retained protocol.RuntimeCredential
		if err := json.Unmarshal(waiting.Records[0].Body, &retained); err != nil {
			return err
		}
		if retained.RefusedReplacement == nil || retained.RefusedReplacement.ID != second || retained.RefusedReplacement.Reason != "identity_unavailable" {
			return fmt.Errorf("the waiting host cannot report the rejected login")
		}
		org := "original-workspace"
		grant.OrgID = &org
		material, err := json.Marshal(grant)
		if err != nil {
			return err
		}
		issued := protocol.NewSecret(string(material))
		if err := saveProviderConfiguration(ctx, db, providerConfiguration{ID: provider.ID, Listed: true, Provider: &provider,
			Authorizations: map[protocol.CredentialID]providerAuthorization{
				first:  {Authentication: provider.Authentication, Revision: 5, Key: &issued},
				second: {Authentication: provider.Authentication, Revision: 2, Key: &issued},
			}}); err != nil {
			return err
		}
		reauthorized, err := ExportConfigurationManifest(ctx, db, node, host, "runtime_credentials", true, nil, &known)
		if err != nil || len(reauthorized.Records) != 2 {
			return fmt.Errorf("verified waiting account did not receive its replacement: %v", err)
		}
		known = reauthorized
		known.Captured = []protocol.CapturedModelCredential{}
		finished, err := ExportConfigurationManifest(ctx, db, node, host, "runtime_credentials", true, nil, &known)
		if err != nil || len(finished.Records) != 0 {
			return fmt.Errorf("credentials remained in the inventory after completion: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestNodeLocalConnectorRenewalIsNeverFollowed(t *testing.T) {
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
		records, err := exportRuntimeCredentials(ctx, db, node, nil)
		if err != nil || len(records) != 0 {
			return fmt.Errorf("connector authorization appeared in the retained model credentials: %v", err)
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
			if _, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{
				Source: controller, Category: "runtime_credentials", Revision: runtime.Revision,
				Records: []protocol.ConfigurationFollowRecord{followRecord(t, runtime.CacheID(), runtime.Revision, runtime, nil)},
			}); err != nil {
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
