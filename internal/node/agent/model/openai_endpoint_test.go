package model

import (
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOpenAICompatibleEndpoint(t *testing.T) {
	t.Parallel()
	for _, api := range []struct {
		protocol protocol.ConversationModelApi
		kind     string
		route    string
	}{
		{protocol.ConversationModelApiChatCompletions, "chat", "/chat/completions"},
		{protocol.ConversationModelApiResponses, "responses", "/responses"},
	} {
		t.Run(api.kind, func(t *testing.T) {
			for _, endpoint := range []struct{ name, configured, base string }{
				{"origin", "", "/v1"},
				{"origin with slash", "/", "/v1"},
				{"versioned base", "/v1/", "/v1"},
				{"versioned prefix", "/gateway/v2/", "/gateway/v2"},
				{"unversioned prefix", "/gateway/openai", "/gateway/openai"},
				{"full endpoint", "/gateway/v1" + api.route + "/", "/gateway/v1"},
				{"operation in prefix", "/gateway/chat/completions" + api.route, "/gateway/chat/completions"},
				{"root-level endpoint", api.route, ""},
				{"escaped prefix", "/tenant%2Fproject/v1" + api.route, "/tenant%2Fproject/v1"},
			} {
				t.Run(endpoint.name, func(t *testing.T) {
					const key = "fixture-key"
					response := fixture(t, api.kind+"-text.sse")
					server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
						if r.Header.Get("Authorization") != "Bearer "+key || r.Header.Get("X-Api-Key") != "" {
							t.Error("expected bearer authentication")
						}
						switch r.Method + " " + r.URL.EscapedPath() {
						case "GET " + endpoint.base + "/models":
							w.Header().Set("Content-Type", "application/json")
							io.WriteString(w, `{"data":[{"id":"fixture-model"}]}`)
						case "POST " + endpoint.base + api.route:
							if r.Header.Get("Content-Type") != "application/json" {
								t.Error("expected a JSON request")
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
					provider.Api, provider.Authentication = api.protocol, protocol.AuthenticationApiKey
					provider.Endpoint = server.URL + endpoint.configured
					secret := protocol.NewSecret(key)
					catalog, err := providers.Discover(t.Context(), provider, &secret)
					if err != nil {
						t.Fatal(err)
					}
					if catalog.Endpoint != server.URL+endpoint.base || len(catalog.Models) != 1 || catalog.Models[0].ID != "fixture-model" {
						t.Fatalf("discovery catalog = %+v", catalog)
					}
					provider.Models = []protocol.ConversationModel{{ID: catalog.Models[0].ID}}
					selection := protocol.DefaultSessionConfig()
					selection.Model = catalog.Models[0].ID
					binding := Binding{Provider: provider, Config: selection, Secret: &secret}
					language, err := New(t.Context(), binding)
					if err != nil {
						t.Fatal(err)
					}
					collect(t, language, requestCall(t, api.kind))
					if api.protocol == protocol.ConversationModelApiResponses {
						request, err := PrepareNativeSearch(t.Context(), binding, map[string]any{"model": selection.Model})
						if err != nil {
							t.Fatal(err)
						}
						if path := request.URL.EscapedPath(); path != endpoint.base+api.route {
							t.Errorf("native search path = %s", path)
						}
					}
				})
			}
		})
	}
}
