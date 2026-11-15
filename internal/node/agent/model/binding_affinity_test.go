package model

import (
	"encoding/hex"
	"encoding/json"
	"reflect"
	"testing"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAnthropicAPIKeyAffinity(t *testing.T) {
	providerID, session := protocol.NewProviderID(), protocol.NewSessionID()
	var firstMetadata any
	for _, modelID := range []string{"claude-haiku-4-5", "claude-opus-5-5", "claude-haiku-5.5", "claude-future-fixture", "compatible-fixture"} {
		t.Run(modelID, func(t *testing.T) {
			response := string(fixture(t, "anthropic-text.sse"))
			server := serverFixture(t, response, response, response)
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Enabled = providerID, true
			provider.Api, provider.Authentication = protocol.ConversationModelApiAnthropic, protocol.AuthenticationApiKey
			provider.Endpoint = server.URL
			selection := protocol.DefaultSessionConfig()
			selection.Provider, selection.Model = provider.ID, modelID
			selection.Mode, selection.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			secret := protocol.NewSecret("fixture-api-key")
			binding, err := Capture(provider, selection, &secret, session)
			if err != nil {
				t.Fatal(err)
			}
			transport, err := transportBinding(binding)
			if err != nil {
				t.Fatal(err)
			}
			language, err := newSDK(t.Context(), transport)
			if err != nil {
				t.Fatal(err)
			}
			cache := anthropic.NewProviderCacheControlOptions(&anthropic.ProviderCacheControlOptions{CacheControl: anthropic.CacheControl{Type: "ephemeral"}})
			call := requestCall(t, "anthropic")
			call.Prompt = fantasy.Prompt{
				{Role: fantasy.MessageRoleSystem, Content: []fantasy.MessagePart{fantasy.TextPart{Text: systemText}}, ProviderOptions: cache},
				{Role: fantasy.MessageRoleUser, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "Earlier question"}}, ProviderOptions: cache},
				{Role: fantasy.MessageRoleAssistant, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "Earlier reply"}}},
				{Role: fantasy.MessageRoleUser, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "Read the file"}}, ProviderOptions: cache},
			}
			collect(t, language, call)

			replay(&call, "anthropic")
			call.Prompt[1].ProviderOptions, call.Prompt[3].ProviderOptions = nil, nil
			call.Prompt[len(call.Prompt)-1].ProviderOptions = cache
			collect(t, language, call)

			binding.Session = protocol.NewSessionID()
			nextTransport, err := transportBinding(binding)
			if err != nil {
				t.Fatal(err)
			}
			nextLanguage, err := newSDK(t.Context(), nextTransport)
			if err != nil {
				t.Fatal(err)
			}
			collect(t, nextLanguage, call)

			requests := server.captured()
			if len(requests) != 3 {
				t.Fatalf("HTTP requests = %d, want 3", len(requests))
			}
			first, continued, fresh := requests[0].Body, requests[1].Body, requests[2].Body
			identity := readAffinityIdentity(t, first)
			if identity["session_id"] != session.String() || len(identity) != 2 {
				t.Fatalf("API-key identity = %#v", identity)
			}
			device, err := hex.DecodeString(identity["device_id"])
			if err != nil || len(device) != 32 {
				t.Fatalf("API-key device identity is not an opaque digest: %#v", identity)
			}
			if !reflect.DeepEqual(first["metadata"], continued["metadata"]) {
				t.Error("tool results and moving cache markers changed the session identity")
			}
			freshIdentity := readAffinityIdentity(t, fresh)
			if freshIdentity["session_id"] != binding.Session.String() || freshIdentity["device_id"] != identity["device_id"] || reflect.DeepEqual(first["metadata"], fresh["metadata"]) {
				t.Errorf("new session did not retain its provider identity and change its session identity: %#v", freshIdentity)
			}
			if firstMetadata == nil {
				firstMetadata = first["metadata"]
			} else if !reflect.DeepEqual(firstMetadata, first["metadata"]) {
				t.Error("changing only the Anthropic model changed the affinity metadata")
			}
			for _, request := range requests {
				if request.Path != "/v1/messages" || request.Body["model"] != modelID || request.Headers.Get("x-api-key") != secret.Expose() {
					t.Errorf("affinity changed the request route, model or API-key authentication: path=%q model=%v", request.Path, request.Body["model"])
				}
			}
			checkValue(t, first, "messages.2.content.0.cache_control", map[string]any{"type": "ephemeral"})
			checkValue(t, continued, "messages.2.content.0.cache_control", nil)
			checkValue(t, continued, "messages.4.content.0.type", "tool_result")
			checkValue(t, continued, "messages.4.content.0.content", "File contents")
			checkValue(t, continued, "messages.4.content.0.cache_control", map[string]any{"type": "ephemeral"})
			checkValue(t, continued, "messages.3.content.0.signature", "signed-thinking")
		})
	}
}

func TestAnthropicAffinityProtocolScope(t *testing.T) {
	for _, api := range []protocol.ConversationModelApi{
		protocol.ConversationModelApiChatCompletions,
		protocol.ConversationModelApiResponses,
		protocol.ConversationModelApiDeepSeek,
		protocol.ConversationModelApiGemini,
		protocol.ConversationModelApiOpenCodeGo,
		protocol.ConversationModelApiOpenCodeZen,
		protocol.ConversationModelApiAzureOpenAi,
		protocol.ConversationModelApiAzureAi,
	} {
		t.Run(string(api), func(t *testing.T) {
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Api = protocol.NewProviderID(), api
			provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "https://fixture.invalid/v1"
			if api == protocol.ConversationModelApiAzureOpenAi {
				provider.Options = &protocol.ConversationConnectionOptions{Kind: "azure_open_ai", Data: protocol.ConversationConnectionOptionsAzureOpenAiPayload{ApiVersion: "2026-01-01"}}
			}
			selection := protocol.DefaultSessionConfig()
			selection.Model = "claude-opus-5-5"
			secret := protocol.NewSecret("fixture-api-key")
			transport, err := transportBinding(Binding{Provider: provider, Config: selection, Secret: &secret, Session: protocol.NewSessionID()})
			if err != nil {
				t.Fatal(err)
			}
			if _, exists := transport.JSON["metadata"]; exists {
				t.Errorf("%s received Anthropic affinity metadata", api)
			}
		})
	}
}

func TestAnthropicOAuthAffinityPreservesIdentity(t *testing.T) {
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Api = protocol.NewProviderID(), protocol.ConversationModelApiAnthropic
	provider.Authentication, provider.Endpoint = protocol.AuthenticationClaudeOAuth, "https://api.anthropic.com"
	installation, account := "install", "account"
	grant := providers.AccountGrant{Kind: provider.Authentication, Access: "fixture-oauth-access", InstallationID: &installation, AccountID: &account}
	raw, err := json.Marshal(grant)
	if err != nil {
		t.Fatal(err)
	}
	secret := protocol.NewSecret(string(raw))
	selection := protocol.DefaultSessionConfig()
	selection.Model = "claude-opus-5-5"
	session := protocol.NewSessionID()
	transport, err := transportBinding(Binding{Provider: provider, Config: selection, Secret: &secret, Session: session})
	if err != nil {
		t.Fatal(err)
	}
	identity := readAffinityIdentity(t, transport.JSON)
	if !transport.OAuth || transport.Headers["Authorization"] != "Bearer fixture-oauth-access" ||
		identity["device_id"] != "ba954c55410a7cce3187c0fc769f3735ac42a2a9b9e935a98a62034615c83c40" ||
		identity["session_id"] != session.String() || identity["account_uuid"] != account {
		t.Errorf("OAuth identity or authorization changed: %#v", identity)
	}
}

func readAffinityIdentity(t *testing.T, body map[string]any) map[string]string {
	t.Helper()
	raw, ok := valueAt(body, "metadata.user_id").(string)
	if !ok || raw == "" {
		t.Fatal("Anthropic request has no affinity identity")
	}
	var identity map[string]string
	if err := json.Unmarshal([]byte(raw), &identity); err != nil {
		t.Fatalf("Anthropic identity is not a JSON string: %v", err)
	}
	return identity
}
