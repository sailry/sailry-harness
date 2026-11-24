package providers

import (
	"net/url"
	"regexp"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Cloud regions and locations are lower-case names such as "us-east-1",
// "us-central1", "global" or "eu". Project IDs are lower-case words joined by
// hyphens, optionally under a legacy "domain:" scope.
var (
	cloudRegion  = regexp.MustCompile(`^[a-z][a-z0-9-]{0,62}[a-z0-9]$`)
	cloudProject = regexp.MustCompile(`^(?:[a-z0-9.-]{1,63}:)?[a-z][a-z0-9-]{4,61}[a-z0-9]$`)
)

// ValidateCloud checks an Amazon Bedrock or Vertex AI provider: API-key
// authentication (the Bedrock API key or the service-account JSON) and the
// region, or the project and location, its options name.
func ValidateCloud(provider protocol.ConversationProvider) error {
	if provider.Authentication != protocol.AuthenticationApiKey {
		return invalid("cloud providers use API-key authentication")
	}
	switch provider.Api {
	case protocol.ConversationModelApiBedrock:
		options, ok := provider.Options.Bedrock()
		if !ok || !cloudRegion.MatchString(options.Region) {
			return invalid("connection options do not match the provider")
		}
	case protocol.ConversationModelApiVertexAi:
		options, ok := provider.Options.VertexAi()
		if !ok || !cloudProject.MatchString(options.Project) || !cloudRegion.MatchString(options.Location) {
			return invalid("connection options do not match the provider")
		}
	default:
		return invalid("connection options do not match the provider")
	}
	return nil
}

// cloudNamespace is the catalog namespace of a Bedrock runtime or Vertex AI
// service endpoint, "" for any other host.
func cloudNamespace(endpoint string) string {
	u, err := url.Parse(strings.TrimSpace(endpoint))
	if err != nil || u.Scheme != "https" {
		return ""
	}
	host := strings.ToLower(u.Hostname())
	if strings.HasPrefix(host, "bedrock-runtime.") && strings.HasSuffix(host, ".amazonaws.com") {
		return "amazon-bedrock"
	}
	if host == "aiplatform.googleapis.com" || strings.HasSuffix(host, "-aiplatform.googleapis.com") ||
		strings.HasPrefix(host, "aiplatform.") && strings.HasSuffix(host, ".rep.googleapis.com") {
		return "google-vertex"
	}
	return ""
}
