package providers

import (
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
