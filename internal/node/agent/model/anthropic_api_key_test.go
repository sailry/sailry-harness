package model

import (
	"encoding/json"
	"io"
	"reflect"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAnthropicAPIKeyIdentityPreservesConversation(t *testing.T) {
	for _, modelID := range []string{"claude-sonnet-5-5", "future-model"} {
		t.Run(modelID, func(t *testing.T) {
			server := serverFixture(t, string(fixture(t, "anthropic-tool.sse")), string(fixture(t, "anthropic-text.sse")))
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Api, provider.Authentication = protocol.NewProviderID(), protocol.ConversationModelApiAnthropic, protocol.AuthenticationApiKey
			provider.Endpoint = server.URL
			selection := protocol.DefaultSessionConfig()
			selection.Model = modelID
			secret := protocol.NewSecret("fixture-key")
			binding := Binding{Provider: provider, Config: selection, Secret: &secret, Session: protocol.NewSessionID()}
			config, err := transportBinding(binding)
			if err != nil {
				t.Fatal(err)
			}
			if config.OAuth {
				t.Fatal("API key selected OAuth transport")
			}
			language, err := newSDK(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			call := requestCall(t, "anthropic")
			before, _ := json.Marshal(call.Prompt)
			parts := collect(t, language, call)
			foundTool := false
			for _, part := range parts {
				if part.Type == fantasy.StreamPartTypeToolCall {
					foundTool = true
					if part.ToolCallName != "read" {
						t.Fatalf("tool name changed: %q", part.ToolCallName)
					}
				}
			}
			if !foundTool {
				t.Fatal("missing tool call")
			}
			after, _ := json.Marshal(call.Prompt)
			if string(before) != string(after) {
				t.Fatal("request identity mutated canonical prompt")
			}
			replay(&call, "anthropic")
			collect(t, language, call)
			requests := server.captured()
			for _, request := range requests {
				if request.Path != "/v1/messages?beta=true" || request.Headers.Get("x-api-key") != "fixture-key" || request.Headers.Get("Authorization") != "" || request.Headers.Get("x-app") != "cli" {
					t.Fatalf("unexpected route/authentication: %s %v", request.Path, request.Headers)
				}
				if strings.Contains(request.Headers.Get("anthropic-beta"), "oauth-") || strings.Contains(string(request.Raw), "x-anthropic-billing-header") {
					t.Fatal("API key request gained OAuth or billing fields")
				}
				checkValue(t, request.Body, "system.0.text", claudeIdentity)
				checkValue(t, request.Body, "system.1.text", systemText)
				checkValue(t, request.Body, "tools.0.name", "read")
				if len(wireArray(request.Body["system"])) != 2 {
					t.Fatal("system identity was duplicated or original blocks changed")
				}
			}
			checkValue(t, requests[1].Body, "messages.1.content.2.name", "read")
			checkValue(t, requests[1].Body, "messages.2.content.0.tool_use_id", "stable-call")
			if !reflect.DeepEqual(requests[0].Body["metadata"], requests[1].Body["metadata"]) {
				t.Fatal("tool round trip changed session identity")
			}
			search, err := PrepareNativeSearch(t.Context(), binding, map[string]any{"model": modelID, "system": "Search instructions", "tools": []any{map[string]any{"type": "web_search_20250305", "name": "web_search"}}})
			if err != nil {
				t.Fatal(err)
			}
			raw, err := io.ReadAll(search.Body)
			search.Body.Close()
			if err != nil {
				t.Fatal(err)
			}
			var searchBody map[string]any
			if err := json.Unmarshal(raw, &searchBody); err != nil {
				t.Fatal(err)
			}
			checkValue(t, searchBody, "system.0.text", claudeIdentity)
			checkValue(t, searchBody, "system.1.text", "Search instructions")
			checkValue(t, searchBody, "tools.0.name", "web_search")
			if !reflect.DeepEqual(searchBody["metadata"], requests[0].Body["metadata"]) || search.Header.Get("x-api-key") != "fixture-key" || search.URL.RequestURI() != "/v1/messages?beta=true" {
				t.Fatal("native search identity differs from conversation")
			}
		})
	}
}

func TestAnthropicClientBetasFollowRequestFeatures(t *testing.T) {
	for _, row := range []struct{ body, want string }{
		{`{}`, "claude-code-20250219"},
		{`{"thinking":{"type":"adaptive"},"output_config":{"effort":"low"}}`, "claude-code-20250219,interleaved-thinking-2025-05-14,effort-2025-11-24"},
		{`{"thinking":{"type":"disabled"}}`, "claude-code-20250219"},
	} {
		got := anthropicBetaHeader("https://fixture.invalid", false, true, []byte(row.body), map[string]string{"Anthropic-Beta": "custom,claude-code-20250219"}, map[string]string{"anthropic-beta": "custom"})
		if got != row.want+",custom" {
			t.Fatalf("beta for %s = %q", row.body, got)
		}
	}
}
