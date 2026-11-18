package model

import (
	"encoding/hex"
	"encoding/json"
	"reflect"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"github.com/sailry/sailry-harness/internal/link"
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
			for index, request := range requests {
				if request.Path != "/v1/messages" || request.Body["model"] != modelID || request.Headers.Get("x-api-key") != secret.Expose() {
					t.Errorf("affinity changed the request route, model or API-key authentication: path=%q model=%v", request.Path, request.Body["model"])
				}
				want := session.String()
				if index == 2 {
					want = binding.Session.String()
				}
				if request.Headers.Get("x-session-affinity") != want || request.Headers.Get("X-Session-Id") != want ||
					request.Headers.Get("User-Agent") != "Sailry/"+link.ProductVersion {
					t.Errorf("API-key request %d identity headers = %v", index, request.Headers)
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
	for _, name := range []string{"x-session-affinity", "X-Session-Id"} {
		if _, exists := transport.Headers[name]; exists {
			t.Errorf("OAuth request received API-key affinity header %s", name)
		}
	}
}

func TestChatGPTOAuthAffinityOnTheWire(t *testing.T) {
	response := string(fixture(t, "responses-text.sse"))
	server := serverFixture(t, response, response, response, response)
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Api = protocol.NewProviderID(), protocol.ConversationModelApiResponses
	provider.Authentication, provider.Endpoint = protocol.AuthenticationChatGptOAuth, "https://chatgpt.com/backend-api/codex"
	account := "fixture-account"
	raw, err := json.Marshal(providers.AccountGrant{Kind: provider.Authentication, Access: "fixture-access", AccountID: &account})
	if err != nil {
		t.Fatal(err)
	}
	secret := protocol.NewSecret(string(raw))
	selection := protocol.DefaultSessionConfig()
	selection.Provider, selection.Model = provider.ID, "fixture-model"
	first, other := protocol.NewSessionID(), protocol.NewSessionID()
	sessions := []protocol.SessionID{first, first, other, {}}
	for _, session := range sessions {
		binding := Binding{Provider: provider, Config: selection, Secret: &secret, Session: session}
		transport, err := transportBinding(binding)
		if err != nil {
			t.Fatal(err)
		}
		if transport.Endpoint != provider.Endpoint {
			t.Fatal("OAuth binding changed its service endpoint")
		}
		// Capture the actual request locally after the destination check.
		transport.Endpoint = server.URL + "/backend-api/codex"
		language, err := newSDK(t.Context(), transport)
		if err != nil {
			t.Fatal(err)
		}
		call, err := BuildCall(binding, Request{System: systemText})
		if err != nil {
			t.Fatal(err)
		}
		collect(t, language, call)
	}
	requests := server.captured()
	if len(requests) != len(sessions) {
		t.Fatalf("captured %d requests, want %d", len(requests), len(sessions))
	}
	for i, request := range requests {
		want := ""
		var key any
		if sessions[i] != (protocol.SessionID{}) {
			want = sessions[i].String()
			key = want
		}
		if request.Path != "/backend-api/codex/responses" ||
			request.Headers.Get("Authorization") != "Bearer fixture-access" ||
			request.Headers.Get("ChatGPT-Account-ID") != account ||
			request.Headers.Get("Session-Id") != want {
			t.Fatalf("request %d did not preserve the OAuth route, account and session", i)
		}
		checkValue(t, request.Body, "prompt_cache_key", key)
		checkValue(t, request.Body, "store", false)
	}
}

func TestAPIKeyAffinityHeadersAcrossProtocols(t *testing.T) {
	for _, api := range []protocol.ConversationModelApi{
		protocol.ConversationModelApiAnthropic,
		protocol.ConversationModelApiChatCompletions,
		protocol.ConversationModelApiResponses,
		protocol.ConversationModelApiGemini,
	} {
		t.Run(string(api), func(t *testing.T) {
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Api = protocol.NewProviderID(), api
			provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "https://fixture.invalid/v1"
			selection := protocol.DefaultSessionConfig()
			selection.Model = "fixture-model"
			secret := protocol.NewSecret("fixture-api-key")
			session := protocol.NewSessionID()
			transport, err := transportBinding(Binding{Provider: provider, Config: selection, Secret: &secret, Session: session})
			if err != nil {
				t.Fatal(err)
			}
			if transport.Headers["x-session-affinity"] != session.String() || transport.Headers["X-Session-Id"] != session.String() ||
				transport.Headers["User-Agent"] != "Sailry/"+link.ProductVersion {
				t.Errorf("headers = %v", transport.Headers)
			}
			transport, err = transportBinding(Binding{Provider: provider, Config: selection, Secret: &secret})
			if err != nil {
				t.Fatal(err)
			}
			if _, exists := transport.Headers["x-session-affinity"]; exists {
				t.Error("a binding without a session received an affinity header")
			}
		})
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

func TestAPIKeyIdentityHeadersOnTheWire(t *testing.T) {
	for _, row := range []struct {
		api      protocol.ConversationModelApi
		kind     string
		response string
		suffix   string
		key      string
	}{
		{protocol.ConversationModelApiAnthropic, "anthropic", "anthropic-text.sse", "", "x-api-key"},
		{protocol.ConversationModelApiChatCompletions, "chat", "chat-text.sse", "/v1", "Authorization"},
		{protocol.ConversationModelApiResponses, "responses", "responses-text.sse", "/v1", "Authorization"},
		{protocol.ConversationModelApiGemini, "google", "google-text.sse", "", "x-goog-api-key"},
	} {
		t.Run(row.kind, func(t *testing.T) {
			server := serverFixture(t, string(fixture(t, row.response)))
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Enabled = protocol.NewProviderID(), true
			provider.Api, provider.Authentication = row.api, protocol.AuthenticationApiKey
			provider.Endpoint = server.URL + row.suffix
			selection := protocol.DefaultSessionConfig()
			selection.Provider, selection.Model = provider.ID, "fixture-model"
			secret := protocol.NewSecret("fixture-api-key")
			session := protocol.NewSessionID()
			transport, err := transportBinding(Binding{Provider: provider, Config: selection, Secret: &secret, Session: session})
			if err != nil {
				t.Fatal(err)
			}
			language, err := newSDK(t.Context(), transport)
			if err != nil {
				t.Fatal(err)
			}
			collect(t, language, requestCall(t, row.kind))
			requests := server.captured()
			if len(requests) != 1 {
				t.Fatalf("requests = %d, want 1", len(requests))
			}
			headers := requests[0].Headers
			if headers.Get("User-Agent") != "Sailry/"+link.ProductVersion || headers.Get("x-session-affinity") != session.String() ||
				headers.Get("X-Session-Id") != session.String() || !strings.Contains(headers.Get(row.key), secret.Expose()) {
				t.Errorf("received headers = %v", headers)
			}
		})
	}
}

func TestPromptCacheKeyRequiresSession(t *testing.T) {
	for _, api := range []protocol.ConversationModelApi{protocol.ConversationModelApiResponses, protocol.ConversationModelApiChatCompletions} {
		t.Run(string(api), func(t *testing.T) {
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Api = protocol.NewProviderID(), api
			provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "https://fixture.invalid/v1"
			selection := protocol.DefaultSessionConfig()
			selection.Model = "fixture-model"
			session := protocol.NewSessionID()
			call := requestCall(t, "chat")
			if params := requestParameters(Binding{Provider: provider, Config: selection, Session: session}, &call); params.Body["prompt_cache_key"] != session.String() {
				t.Errorf("session cache key = %v", params.Body["prompt_cache_key"])
			}
			if params := requestParameters(Binding{Provider: provider, Config: selection}, &call); params.Body["prompt_cache_key"] != nil || slices.Contains(params.Order, "prompt_cache_key") && api == protocol.ConversationModelApiChatCompletions {
				t.Errorf("request without a session received cache key %v", params.Body["prompt_cache_key"])
			}
		})
	}
}

func TestAPIKeyCustomHeaders(t *testing.T) {
	server := serverFixture(t, string(fixture(t, "anthropic-text.sse")))
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled = protocol.NewProviderID(), true
	provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiAnthropic, protocol.AuthenticationApiKey, server.URL
	// Stored providers are validated; the transport still keeps the credential its own.
	provider.Headers = map[string]string{"user-agent": "claude-cli/9", "anthropic-beta": "fixture-beta", "X-Session-Id": "pinned", "x-api-key": "other"}
	selection := protocol.DefaultSessionConfig()
	selection.Provider, selection.Model = provider.ID, "fixture-model"
	secret := protocol.NewSecret("fixture-api-key")
	transport, err := transportBinding(Binding{Provider: provider, Config: selection, Secret: &secret, Session: protocol.NewSessionID()})
	if err != nil {
		t.Fatal(err)
	}
	language, err := newSDK(t.Context(), transport)
	if err != nil {
		t.Fatal(err)
	}
	collect(t, language, requestCall(t, "anthropic"))
	headers := server.captured()[0].Headers
	if headers.Get("User-Agent") != "claude-cli/9" || headers.Get("anthropic-beta") != "fixture-beta" || headers.Get("X-Session-Id") != "pinned" ||
		headers.Get("x-api-key") != secret.Expose() || len(headers.Values("User-Agent")) != 1 {
		t.Errorf("received headers = %v", headers)
	}
}
