package providers

import (
	"regexp"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Project IDs are lower-case words joined by hyphens, optionally under a
// legacy "domain:" scope. Regions and locations are protocol.CloudName.
var cloudProject = regexp.MustCompile(`^(?:[a-z0-9.-]{1,63}:)?[a-z][a-z0-9-]{4,61}[a-z0-9]$`)

// ValidateCloud checks an Amazon Bedrock or Vertex AI provider: API-key
// authentication (the Bedrock API key or the service-account JSON), the
// region, or the project and location, its options name, and an endpoint
// that is a service host without a path.
func ValidateCloud(provider protocol.ConversationProvider) error {
	if provider.Authentication != protocol.AuthenticationApiKey {
		return invalid("cloud providers use API-key authentication")
	}
	switch provider.Api {
	case protocol.ConversationModelApiBedrock:
		options, ok := provider.Options.Bedrock()
		if !ok || !protocol.CloudName(options.Region) {
			return invalid("connection options do not match the provider")
		}
	case protocol.ConversationModelApiVertexAi:
		options, ok := provider.Options.VertexAi()
		if !ok || !cloudProject.MatchString(options.Project) || !protocol.CloudName(options.Location) {
			return invalid("connection options do not match the provider")
		}
	default:
		return invalid("connection options do not match the provider")
	}
	endpoint, err := Endpoint(provider.Endpoint)
	if err != nil {
		return err
	}
	if strings.Trim(endpoint.Path, "/") != "" {
		return invalid("cloud endpoint must be a service host without a path")
	}
	return nil
}

// cloudNamespace is the catalog namespace of a Bedrock runtime or Vertex AI
// service endpoint, "" for any other host.
func cloudNamespace(endpoint string) string {
	switch protocol.CloudEndpointApi(endpoint) {
	case protocol.ConversationModelApiBedrock:
		return "amazon-bedrock"
	case protocol.ConversationModelApiVertexAi:
		return "google-vertex"
	}
	return ""
}
