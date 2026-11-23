package model

import (
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

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
			query := ""
			if row.beta != "" {
				query = "beta=true"
			}
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Header.Get("x-api-key") != key || r.Header.Get("Authorization") != "" || r.Header.Get("anthropic-version") != "2023-06-01" {
					t.Error("expected Anthropic API-key and version headers")
				}
				switch r.Method + " " + r.URL.EscapedPath() {
				case "GET " + row.prefix + "/v1/models":
					w.Header().Set("Content-Type", "application/json")
					io.WriteString(w, `{"data":[{"id":"fixture-model"}],"has_more":false}`)
				case "POST " + messagesPath:
					if r.URL.RawQuery != query || r.Header.Get("anthropic-beta") != row.beta || r.Header.Get("Content-Type") != "application/json" {
						t.Errorf("Messages query=%q beta=%q content type=%q", r.URL.RawQuery, r.Header.Get("anthropic-beta"), r.Header.Get("Content-Type"))
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
			selection.Model = catalog.Models[0].ID
			binding := Binding{Provider: provider, Config: selection, Secret: &secret}
			language, err := New(t.Context(), binding)
			if err != nil {
				t.Fatal(err)
			}
			collect(t, language, requestCall(t, "anthropic"))
			search, err := PrepareNativeSearch(t.Context(), binding, map[string]any{"model": selection.Model})
			if err != nil {
				t.Fatal(err)
			}
			if search.URL.EscapedPath() != messagesPath || search.URL.RawQuery != query || search.Header.Get("anthropic-beta") != row.beta {
				t.Errorf("native search path = %s, beta = %q", search.URL.RequestURI(), search.Header.Get("anthropic-beta"))
			}
		})
	}
}
