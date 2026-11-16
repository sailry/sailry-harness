package providers

import (
	"bytes"
	"encoding/json"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRuntimeAccountExcludesRenewalMaterial(t *testing.T) {
	t.Parallel()
	account, installation := "account-fixture", "installation-fixture"
	for _, authentication := range []protocol.Authentication{
		protocol.AuthenticationChatGpt, protocol.AuthenticationChatGptOAuth,
		protocol.AuthenticationClaudeOAuth, protocol.AuthenticationCopilot, protocol.AuthenticationPlugin,
	} {
		t.Run(string(authentication), func(t *testing.T) {
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Authentication = protocol.NewProviderID(), authentication
			provider.Endpoint = map[protocol.Authentication]string{
				protocol.AuthenticationChatGpt: "https://api.openai.com/v1", protocol.AuthenticationChatGptOAuth: "https://chatgpt.com/backend-api/codex",
				protocol.AuthenticationClaudeOAuth: "https://api.anthropic.com", protocol.AuthenticationCopilot: "https://api.githubcopilot.com",
				protocol.AuthenticationPlugin: "https://model.example.test",
			}[authentication]
			if authentication == protocol.AuthenticationClaudeOAuth {
				provider.Api = protocol.ConversationModelApiAnthropic
			} else {
				provider.Api = protocol.ConversationModelApiResponses
			}
			grant := AccountGrant{
				Kind: authentication, Access: "runtime-access", Refresh: "never-copy-refresh", IDToken: "never-copy-identity",
				Github: "never-copy-github", ClientID: "never-copy-client", Subject: "never-copy-subject",
				ExpiresAtMs: uint64(time.Now().Add(time.Hour).UnixMilli()), AccountID: &account, InstallationID: &installation,
				Endpoint: provider.Endpoint, Scopes: []string{"resource.invoke", "chatgpt.tokens.use.direct"},
			}
			if authentication == protocol.AuthenticationPlugin {
				reference := protocol.PluginReference{Name: "runtime-fixture", Digest: "fixture", SettingsRevision: 1}
				binding := &protocol.ConversationPresetsBinding{Package: &reference, Auth: &protocol.PluginModelAuthBinding{Flow: "account"}}
				provider.Binding = &binding
				refresh := "never-copy-refresh"
				expiry := grant.ExpiresAtMs
				grant.Record = &AccountRecord{
					Target: AccountTarget{Provider: provider.ID, Package: reference, Flow: "account", Endpoint: provider.Endpoint,
						Fields: map[string]string{"client_secret": "never-copy-client-secret"},
						Declaration: protocol.PluginModelAuthDeclaration{Inject: []protocol.PluginModelAuthInjection{
							{Header: "Authorization", Template: "Bearer {access}"}, {Header: "X-Account", Template: "{extra.account}"},
						}}},
					Issued: authorization.Issued{Access: grant.Access, Refresh: &refresh, ExpiresAtMs: &expiry,
						Extra: map[string]string{"account": account, "id_token": "never-copy-identity", "unused": "never-copy-unused"}},
				}
			}
			runtime, err := RuntimeAccount(provider, grant)
			if err != nil {
				t.Fatal(err)
			}
			body, err := json.Marshal(runtime)
			if err != nil || bytes.Contains(body, []byte("never-copy-")) {
				t.Fatalf("runtime export included controller material: %v", err)
			}
			secret, err := RuntimeAccountSecret(runtime)
			if err != nil {
				t.Fatal(err)
			}
			_, headers, err := AccountHeaders(provider, secret)
			if err != nil || headers.Get("Authorization") != "Bearer runtime-access" {
				t.Fatalf("runtime credential cannot execute its captured model: %v", err)
			}
			other := provider
			other.ID = protocol.NewProviderID()
			if _, _, err := AccountHeaders(other, secret); err == nil {
				t.Fatal("runtime credential escaped its captured provider")
			}
			other = provider
			other.Endpoint = "https://unrelated.example.test"
			if _, _, err := AccountHeaders(other, secret); err == nil {
				t.Fatal("runtime credential escaped its captured endpoint")
			}
		})
	}
}

func TestRuntimeInjectionCannotRenderRenewalFields(t *testing.T) {
	t.Parallel()
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Authentication, provider.Endpoint = protocol.NewProviderID(), protocol.AuthenticationPlugin, "https://model.example.test"
	refresh := "renewal-only-secret"
	for _, template := range []string{"{refresh}", "{extra.id_token}", "{extra.alias}"} {
		grant := AccountGrant{Kind: provider.Authentication, Record: &AccountRecord{
			Target: AccountTarget{Provider: provider.ID, Endpoint: provider.Endpoint,
				Declaration: protocol.PluginModelAuthDeclaration{Inject: []protocol.PluginModelAuthInjection{{Header: "Authorization", Template: template}}}},
			Issued: authorization.Issued{Access: "runtime-access", Refresh: &refresh, Extra: map[string]string{"id_token": refresh, "alias": refresh}},
		}}
		if _, err := RuntimeAccount(provider, grant); err == nil {
			t.Fatalf("runtime credential rendered controller-only injection %q", template)
		}
	}
}

func TestAccountRenewalUsesIssuedLifetime(t *testing.T) {
	t.Parallel()
	issued, expiry := uint64(1_000_000), uint64(5_000_000)
	grant := AccountGrant{IssuedAtMs: &issued, ExpiresAtMs: expiry}
	if due := grant.RenewalAt(); due == nil || *due != 2_000_000 {
		t.Fatalf("quarter-TTL renewal=%v", due)
	}
	earliest := uint64(3_000_000)
	grant.EarliestRefreshAt = &earliest
	if due := grant.RenewalAt(); due == nil || *due != earliest {
		t.Fatalf("server earliest refresh was ignored: %v", due)
	}
	grant.IssuedAtMs, grant.EarliestRefreshAt = nil, nil
	if due := grant.RenewalAt(); due == nil || *due != expiry-60_000 {
		t.Fatalf("legacy renewal no longer uses its original threshold: %v", due)
	}
	grant.ExpiresAtMs = 0
	if grant.RenewalAt() != nil {
		t.Fatal("unknown expiry invented a proactive deadline")
	}
}
