package providers

import (
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCustomHeaderValidation(t *testing.T) {
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Endpoint = protocol.NewProviderID(), "Gateway", "https://gateway.example"
	provider.Api, provider.Authentication = protocol.ConversationModelApiAnthropic, protocol.AuthenticationApiKey
	provider.Headers = map[string]string{"User-Agent": "claude-cli/9", "anthropic-beta": "a,b"}
	if err := ValidateConfiguration(provider); err != nil {
		t.Fatalf("valid headers rejected: %v", err)
	}
	for name, headers := range map[string]map[string]string{
		"credential":     {"X-Api-Key": "other"},
		"authorization":  {"authorization": "Bearer other"},
		"framing":        {"Content-Length": "1"},
		"encoding":       {"Accept-Encoding": "br"},
		"invalid name":   {"bad name": "x"},
		"invalid value":  {"X-Test": "a\nb"},
		"case duplicate": {"X-Test": "a", "x-test": "b"},
	} {
		provider.Headers = headers
		if ValidateConfiguration(provider) == nil {
			t.Errorf("%s headers accepted", name)
		}
	}
	provider.Headers = map[string]string{"X-Test": "a"}
	provider.Authentication = protocol.AuthenticationClaudeOAuth
	provider.Endpoint = "https://api.anthropic.com"
	if err := validateHeaders(provider); err == nil {
		t.Error("a sign-in provider accepted custom headers")
	}
}

func TestDiscoveryForwardsUserAgentAndCustomHeaders(t *testing.T) {
	var seen http.Header
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		seen = request.Header.Clone()
		writer.Header().Set("Content-Type", "application/json")
		io.WriteString(writer, `{"data":[{"id":"claude-fixture","display_name":"Claude Fixture"}],"has_more":false}`)
	}))
	defer server.Close()

	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Endpoint = protocol.NewProviderID(), "Gateway", server.URL
	provider.Api, provider.Authentication = protocol.ConversationModelApiAnthropic, protocol.AuthenticationApiKey
	secret := protocol.NewSecret("fixture-key")

	if _, err := discover(t.Context(), provider, &secret, server.Client()); err != nil {
		t.Fatalf("default discovery failed: %v", err)
	}
	if seen.Get("User-Agent") != "claude-cli/2.1.330 (external, sdk-cli)" || seen.Get("anthropic-version") != "2023-06-01" ||
		seen.Get("Accept") != "application/json" || seen.Get("x-api-key") != "fixture-key" || seen.Get("Authorization") != "" {
		t.Fatalf("default discovery headers = %v", seen)
	}

	provider.Headers = map[string]string{
		"user-agent":     "gateway-client/2",
		"accept":         "application/vnd.gateway+json",
		"anthropic-beta": "custom-beta-1",
		"X-Tenant-Id":    "tenant-a",
		"x-api-key":      "untrusted",
		"Content-Length": "999",
	}
	if _, err := discover(t.Context(), provider, &secret, server.Client()); err != nil {
		t.Fatalf("custom header discovery failed: %v", err)
	}
	if seen.Get("User-Agent") != "gateway-client/2" || len(seen.Values("User-Agent")) != 1 ||
		seen.Get("anthropic-beta") != "custom-beta-1" || seen.Get("X-Tenant-Id") != "tenant-a" ||
		seen.Get("x-api-key") != "fixture-key" || seen.Get("anthropic-version") != "2023-06-01" || seen.Get("Accept") != "application/vnd.gateway+json" {
		t.Fatalf("custom discovery headers = %v", seen)
	}
}
