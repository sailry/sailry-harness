package model

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestGatewayCacheAffinityOnWire(t *testing.T) {
	for _, row := range []struct {
		name, model, endpointPath, kind string
		api                             protocol.ConversationModelApi
		cacheKey, openCode              bool
	}{
		{"Go", "deepseek-v4-flash", "/zen/go/v1", "chat", protocol.ConversationModelApiOpenCodeGo, false, true},
		{"Zen", "gpt-6.2-sol", "/zen/v1/responses", "responses", protocol.ConversationModelApiOpenCodeZen, false, true},
		{"Azure OpenAI", "gpt-6.2-sol", "/", "chat", protocol.ConversationModelApiAzureOpenAi, true, false},
		{"unknown Azure deployment", "production", "/", "chat", protocol.ConversationModelApiAzureOpenAi, false, false},
		{"legacy Azure deployment", "gpt-4o", "/", "chat", protocol.ConversationModelApiAzureOpenAi, false, false},
		{"DeepSeek automatic", "deepseek-v4-flash", "", "chat", protocol.ConversationModelApiDeepSeek, false, false},
	} {
		t.Run(row.name, func(t *testing.T) {
			response := string(fixture(t, row.kind+"-text.sse"))
			server := serverFixture(t, response, response, response)
			provider := protocol.DefaultConversationProvider()
			provider.Api, provider.Authentication = row.api, protocol.AuthenticationApiKey
			provider.Endpoint = server.URL + row.endpointPath
			provider.Models = []protocol.ConversationModel{{ID: row.model}}
			if row.api == protocol.ConversationModelApiAzureOpenAi {
				provider.Options = &protocol.ConversationConnectionOptions{Kind: "azure_open_ai", Data: protocol.ConversationConnectionOptionsAzureOpenAiPayload{ApiVersion: "2025-01-01-preview"}}
			}
			config := protocol.DefaultSessionConfig()
			config.Model = row.model
			key := protocol.NewSecret("fixture-api-key")
			session := protocol.NewSessionID()
			sessions := []protocol.SessionID{session, session, {}}
			for _, id := range sessions {
				binding := Binding{Provider: provider, Config: config, Session: id, Secret: &key}
				language, err := New(t.Context(), binding)
				if err != nil {
					t.Fatal(err)
				}
				call, err := BuildCall(binding, Request{System: systemText})
				if err != nil {
					t.Fatal(err)
				}
				collect(t, language, call)
			}
			captured := server.captured()
			if len(captured) != len(sessions) {
				t.Fatalf("requests = %d, want %d", len(captured), len(sessions))
			}
			for i, request := range captured {
				var wantKey any
				wantSession := ""
				if sessions[i] != (protocol.SessionID{}) {
					if row.cacheKey {
						wantKey = sessions[i].String()
					}
					if row.openCode {
						wantSession = sessions[i].String()
					}
				}
				checkValue(t, request.Body, "prompt_cache_key", wantKey)
				if got := request.Headers.Get("x-opencode-session"); got != wantSession {
					t.Errorf("OpenCode session header = %q, want %q", got, wantSession)
				}
			}
		})
	}
}

func TestFoundryAzureOpenAICacheAffinityFromDeployment(t *testing.T) {
	response := string(fixture(t, "responses-text.sse"))
	captured := make(chan map[string]any, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/projects/fixture/deployments":
			fmt.Fprint(w, `{"value":[{"name":"production","modelName":"gpt-6.2-astra","modelPublisher":"OpenAI"}]}`)
		case "/api/projects/fixture/openai/v1/responses":
			var body map[string]any
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				t.Error(err)
			}
			captured <- body
			w.Header().Set("Content-Type", "text/event-stream")
			fmt.Fprint(w, response)
		default:
			t.Errorf("unexpected Foundry path %q", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()
	provider := protocol.DefaultConversationProvider()
	provider.Api, provider.Authentication = protocol.ConversationModelApiAzureAi, protocol.AuthenticationApiKey
	provider.Endpoint = server.URL + "/api/projects/fixture"
	config := protocol.DefaultSessionConfig()
	config.Model = "production"
	key := protocol.NewSecret("fixture-api-key")
	binding := Binding{Provider: provider, Config: config, Session: protocol.NewSessionID(), Secret: &key}
	language, err := New(t.Context(), binding)
	if err != nil {
		t.Fatal(err)
	}
	call, err := BuildCall(binding, Request{System: systemText})
	if err != nil {
		t.Fatal(err)
	}
	collect(t, language, call)
	select {
	case body := <-captured:
		checkValue(t, body, "model", "production")
		checkValue(t, body, "prompt_cache_key", binding.Session.String())
	default:
		t.Fatal("Foundry did not send a Responses request")
	}
}
