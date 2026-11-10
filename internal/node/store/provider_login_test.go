package store

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderAccountLifecycleAndQuota(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("account-fixture", 'a')
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		extension.Actions = []protocol.PluginAction{protocol.PluginActionAuthenticateModels, protocol.PluginActionInspectCredentials}
		authentication, client := protocol.AuthenticationChatGpt, "oaiapp_fixture"
		declaration := protocol.DefaultPluginModelAuthDeclaration()
		declaration.ID, declaration.Authentication, declaration.Client.ClientID = "account", &authentication, &client
		declaration.Display.Label = "Account"
		declaration.Flow = protocol.PluginModelAuthFlow{Kind: "script", Data: protocol.PluginModelAuthFlowScriptPayload{
			Script: protocol.PluginModelAdapterScript{Entry: "auth.js", Resources: []string{}}, Refresh: true, Handler: "step", Requests: map[string]protocol.PluginModelAuthEndpoint{},
		}}
		extension.ModelAuth, info.Extension = []protocol.PluginModelAuthDeclaration{declaration}, &extension
		info, err := InstallPlugin(ctx, db, info, 0, true)
		if err != nil {
			return err
		}
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Revision, provider.Enabled = protocol.NewProviderID(), 1, true
		provider.Name, provider.Api, provider.Authentication, provider.Endpoint = "Account fixture", protocol.ConversationModelApiResponses, authentication, "https://api.openai.com/v1"
		saved := providerConfiguration{ID: provider.ID, Listed: true, Provider: &provider, Authorizations: map[protocol.CredentialID]providerAuthorization{}}
		if err := saveProviderConfiguration(ctx, db, saved); err != nil {
			return err
		}
		capture, err := CaptureProviderLogin(ctx, db, provider.ID, 1, protocol.NewRequestID())
		if err != nil {
			return err
		}
		late := capture
		label := "account@example.test"
		grant := providers.AccountGrant{Kind: authentication, Access: "private-access", Refresh: "private-refresh", IDToken: "private-identity",
			ClientID: client, Subject: "account-subject", Email: &label, ExpiresAtMs: uint64(time.Now().Add(10 * time.Second).UnixMilli()),
			Scopes: []string{"resource.invoke", "chatgpt.tokens.use.direct"}}
		changes, err := CompleteProviderLogin(ctx, db, node, capture, grant)
		if err != nil || len(changes) != 2 {
			return fmt.Errorf("complete login: %v, %d events", err, len(changes))
		}
		if _, err := CompleteProviderLogin(ctx, db, node, late, grant); err == nil {
			return fmt.Errorf("stale login capture completed")
		}
		credentials, err := providerCredentials(ctx, db)
		if err != nil || len(credentials) != 1 || credentials[0].AccountLabel == nil || *credentials[0].AccountLabel != label {
			return fmt.Errorf("credential metadata lost its public account label: %v", err)
		}
		public, _ := json.Marshal(credentials)
		if bytes.Contains(public, []byte(grant.Access)) || bytes.Contains(public, []byte(grant.Refresh)) || bytes.Contains(public, []byte(grant.IDToken)) {
			return fmt.Errorf("credential metadata exposed a protected grant")
		}
		current, err := currentProvider(ctx, db, provider.ID, nil)
		if err != nil {
			return err
		}
		current.Quota = &protocol.ConversationQuotaSnapshot{Provider: provider.ID, Credential: *current.Provider.Credential, Revision: 1, ObservedMs: 1,
			FiveHour: &protocol.ConversationQuotaWindow{UsedBasisPoints: 4250}}
		if !providerQuotaMatches(node, *current, *current.Quota) {
			return fmt.Errorf("current quota owner did not match")
		}
		if err := saveProviderConfiguration(ctx, db, *current); err != nil {
			return err
		}
		renewal, err := BeginProviderRenewal(ctx, db, node, *current.Provider, *current.Provider.Credential, time.Now())
		if err != nil || renewal == nil {
			return fmt.Errorf("renewal did not capture the expiring account: %v", err)
		}
		if _, err := BeginProviderRenewal(ctx, db, node, *current.Provider, *current.Provider.Credential, time.Now()); err == nil {
			return fmt.Errorf("pending rotating renewal was replayed")
		}
		grant.Access, grant.Refresh, grant.ExpiresAtMs = "rotated-access", "rotated-refresh", uint64(time.Now().Add(time.Hour).UnixMilli())
		changes, err = CompleteProviderRenewal(ctx, db, node, *renewal, grant)
		if err != nil || len(changes) != 2 || changes[1].Kind != "provider_quota_changed" {
			return fmt.Errorf("renewal did not invalidate the old quota revision: %v, %#v", err, changes)
		}
		current, err = currentProvider(ctx, db, provider.ID, nil)
		if err != nil || current.Quota != nil {
			return fmt.Errorf("old quota survived credential rotation: %v", err)
		}
		info.Summary.Digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
		version := "0.1.1"
		info.Summary.Version = &version
		info, err = InstallPlugin(ctx, db, info, info.Summary.Revision, true)
		if err != nil {
			return err
		}
		if renewal, err := BeginProviderRenewal(ctx, db, node, *current.Provider, *current.Provider.Credential, time.Now()); err != nil || renewal != nil {
			return fmt.Errorf("implementation-only package update invalidated account ownership: %v", err)
		}
		updated := info
		updated.Extension.ModelAuth[0].Scopes = []string{"changed-scope"}
		if err := savePluginInventory(ctx, db, updated, true); err != nil {
			return err
		}
		if _, err := BeginProviderRenewal(ctx, db, node, *current.Provider, *current.Provider.Credential, time.Now()); err == nil {
			return fmt.Errorf("changed authorization scopes retained an existing account")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
