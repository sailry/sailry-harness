package providers

import (
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/http/httpguts"
)

// maxCustomHeaders bounds the request headers one provider adds.
const maxCustomHeaders = 32

// Claude Code's print mode sends this User-Agent to Messages gateways.
const anthropicAPIKeyUserAgent = "claude-cli/2.1.330 (external, sdk-cli)"

// APIKeyHeaderDefaults keeps native model discovery and conversation requests
// on the same client and protocol defaults before applying provider overrides.
func APIKeyHeaderDefaults(api protocol.ConversationModelApi) map[string]string {
	if api == protocol.ConversationModelApiAnthropic {
		return map[string]string{
			"User-Agent":        anthropicAPIKeyUserAgent,
			"Accept":            "application/json",
			"anthropic-version": "2023-06-01",
			"x-app":             "cli",
		}
	}
	return map[string]string{"User-Agent": "Sailry/" + link.ProductVersion}
}

// reservedHeaders carry credentials or message framing that the transport
// sets itself; a custom value would misplace a key or corrupt the body.
var reservedHeaders = []string{
	"authorization", "x-api-key", "x-goog-api-key", "api-key",
	"host", "content-length", "content-type", "transfer-encoding", "connection", "accept-encoding",
}

// ReservedHeader reports whether name is set only by the transport.
func ReservedHeader(name string) bool {
	return slices.ContainsFunc(reservedHeaders, func(reserved string) bool { return strings.EqualFold(reserved, name) })
}

func validateHeaders(provider protocol.ConversationProvider) error {
	if len(provider.Headers) == 0 {
		return nil
	}
	if provider.Authentication != protocol.AuthenticationApiKey {
		return invalid("custom request headers require API key authentication")
	}
	if len(provider.Headers) > maxCustomHeaders {
		return invalid("provider has too many custom request headers")
	}
	seen := map[string]bool{}
	for name, value := range provider.Headers {
		folded := strings.ToLower(name)
		if !httpguts.ValidHeaderFieldName(name) || !httpguts.ValidHeaderFieldValue(value) || len(value) > 4096 || seen[folded] {
			return invalid("custom request header " + name + " is invalid")
		}
		if ReservedHeader(name) {
			return invalid("request header " + name + " is set by Sailry")
		}
		seen[folded] = true
	}
	return nil
}
