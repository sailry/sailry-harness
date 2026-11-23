package model

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAnthropicCompatibleEndpoint(t *testing.T) {
	for _, row := range []struct{ name, configured, prefix, beta string }{
		{name: "origin"},
		{name: "versioned base", configured: "/v1/"},
		{name: "gateway messages URL", configured: "/gateway/v1/messages/", prefix: "/gateway"},
		{name: "escaped gateway prefix and beta", configured: "/tenant%2Fproject/v1/messages", prefix: "/tenant%2Fproject", beta: "context-management-2025-06-27"},
	} {
		t.Run(row.name, func(t *testing.T) {
			const key = "fixture-key"
			response := fixture(t, "anthropic-text.sse")
			messagesPath := row.prefix + "/v1/messages"
			query := "beta=true"
			chatBeta := "claude-code-20250219,interleaved-thinking-2025-05-14,effort-2025-11-24"
			searchBeta := "claude-code-20250219"
			if row.beta != "" {
				chatBeta += "," + row.beta
				searchBeta += "," + row.beta
			}
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Header.Get("x-api-key") != key || r.Header.Get("Authorization") != "" || r.Header.Get("anthropic-version") != "2023-06-01" {
					t.Error("expected Anthropic API-key and version headers")
				}
				switch r.Method + " " + r.URL.EscapedPath() {
				case "GET " + row.prefix + "/v1/models":
					w.Header().Set("Content-Type", "application/json")
					io.WriteString(w, `{"data":[{"id":"fixture-model","capabilities":{
						"thinking":{"supported":true,"types":{"adaptive":{"supported":true},"enabled":{"supported":false}}},
						"effort":{"supported":true,"low":{"supported":true}}
					}}],"has_more":false}`)
				case "POST " + messagesPath:
					if r.URL.RawQuery != query || r.Header.Get("anthropic-beta") != chatBeta || r.Header.Get("Content-Type") != "application/json" {
						t.Errorf("Messages query=%q beta=%q content type=%q", r.URL.RawQuery, r.Header.Get("anthropic-beta"), r.Header.Get("Content-Type"))
					}
					var body map[string]any
					if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
						t.Errorf("decode Messages body: %v", err)
					} else {
						checkValue(t, body, "thinking.type", "adaptive")
						checkValue(t, body, "output_config.effort", "low")
					}
					w.Header().Set("Content-Type", "text/event-stream")
					w.Write(response)
				default:
					t.Errorf("unexpected request %s %s", r.Method, r.URL.EscapedPath())
					http.NotFound(w, r)
				}
			}))
			t.Cleanup(server.Close)
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Enabled = protocol.NewProviderID(), true
			provider.Api, provider.Authentication = protocol.ConversationModelApiAnthropic, protocol.AuthenticationApiKey
			provider.Endpoint = server.URL + row.configured
			if row.beta != "" {
				provider.Headers = map[string]string{"Anthropic-Beta": row.beta}
			}
			secret := protocol.NewSecret(key)
			catalog, err := providers.Discover(t.Context(), provider, &secret)
			if err != nil {
				t.Fatal(err)
			}
			if catalog.Endpoint != server.URL+row.prefix || len(catalog.Models) != 1 || catalog.Models[0].ID != "fixture-model" {
				t.Fatalf("discovery catalog = %+v", catalog)
			}
			selection := protocol.DefaultSessionConfig()
			selection.Provider, selection.Model = provider.ID, catalog.Models[0].ID
			selection.Mode, selection.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			selection.Effort = protocol.Effort{Kind: "low"}
			model := protocol.DefaultConversationModel()
			model.ID = selection.Model
			provider.Models = []protocol.ConversationModel{providers.RefreshDiscoveredCapabilities(provider, model, catalog.Models[0].Capabilities)}
			binding, err := Capture(provider, selection, &secret, protocol.NewSessionID())
			if err != nil {
				t.Fatal(err)
			}
			language, err := New(t.Context(), binding)
			if err != nil {
				t.Fatal(err)
			}
			record := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Text(0, "hello")), policyClock{}.now)
			call, err := BuildCall(binding, Request{Records: []canonical.Record{record}})
			if err != nil {
				t.Fatal(err)
			}
			collect(t, language, call)
			search, err := PrepareNativeSearch(t.Context(), binding, map[string]any{"model": selection.Model})
			if err != nil {
				t.Fatal(err)
			}
			if search.URL.EscapedPath() != messagesPath || search.URL.RawQuery != query || search.Header.Get("anthropic-beta") != searchBeta {
				t.Errorf("native search path = %s, beta = %q", search.URL.RequestURI(), search.Header.Get("anthropic-beta"))
			}
		})
	}
}
