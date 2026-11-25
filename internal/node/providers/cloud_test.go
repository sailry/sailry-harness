package providers

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func cloudFixture(api protocol.ConversationModelApi, options protocol.ConversationConnectionOptions) protocol.ConversationProvider {
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Cloud", true
	provider.Api, provider.Authentication = api, protocol.AuthenticationApiKey
	provider.Options = &options
	switch api {
	case protocol.ConversationModelApiBedrock:
		provider.Endpoint = "https://bedrock-runtime.us-east-1.amazonaws.com"
	default:
		provider.Endpoint = "https://aiplatform.googleapis.com"
	}
	return provider
}

func TestValidateCloud(t *testing.T) {
	t.Parallel()
	bedrock := func(region string) protocol.ConversationConnectionOptions {
		return protocol.ConversationConnectionOptions{Kind: "bedrock", Data: protocol.ConversationConnectionOptionsBedrockPayload{Region: region}}
	}
	vertex := func(project, location string) protocol.ConversationConnectionOptions {
		return protocol.ConversationConnectionOptions{Kind: "vertex_ai", Data: &protocol.ConversationConnectionOptionsVertexAiPayload{Project: project, Location: location}}
	}
	for _, row := range []struct {
		name    string
		api     protocol.ConversationModelApi
		options protocol.ConversationConnectionOptions
		valid   bool
	}{
		{"bedrock region", protocol.ConversationModelApiBedrock, bedrock("eu-central-1"), true},
		{"bedrock without region", protocol.ConversationModelApiBedrock, bedrock(""), false},
		{"bedrock region path", protocol.ConversationModelApiBedrock, bedrock("us-east-1/x"), false},
		{"bedrock vertex options", protocol.ConversationModelApiBedrock, vertex("proj-123", "global"), false},
		{"vertex project and location", protocol.ConversationModelApiVertexAi, vertex("proj-123", "us-central1"), true},
		{"vertex scoped project", protocol.ConversationModelApiVertexAi, vertex("example.com:proj-123", "eu"), true},
		{"vertex without project", protocol.ConversationModelApiVertexAi, vertex("", "global"), false},
		{"vertex location path", protocol.ConversationModelApiVertexAi, vertex("proj-123", "us/central"), false},
		{"vertex hosted options", protocol.ConversationModelApiVertexAi, protocol.ConversationConnectionOptions{Kind: "hosted", Data: protocol.ConversationConnectionOptionsHostedPayload{Product: "metered"}}, false},
	} {
		provider := cloudFixture(row.api, row.options)
		if err := ValidateConfiguration(provider); (err == nil) != row.valid {
			t.Errorf("%s: %v", row.name, err)
		}
	}
	// The endpoint is a service host: a path is refused when it is saved,
	// while a private host is accepted as given.
	for endpoint, valid := range map[string]bool{
		"https://bedrock-runtime.us-east-1.amazonaws.com/":   true,
		"https://vpce-1.bedrock-runtime.us-east-1.vpce.aws":  true,
		"https://bedrock-runtime.us-east-1.amazonaws.com/v1": false,
		"https://gateway.example/bedrock":                    false,
	} {
		provider := cloudFixture(protocol.ConversationModelApiBedrock, bedrock("us-east-1"))
		provider.Endpoint = endpoint
		if err := ValidateConfiguration(provider); (err == nil) != valid {
			t.Errorf("endpoint %s: %v", endpoint, err)
		}
	}
	signedIn := cloudFixture(protocol.ConversationModelApiBedrock, bedrock("us-east-1"))
	signedIn.Authentication = protocol.AuthenticationClaudeOAuth
	if ValidateCloud(signedIn) == nil {
		t.Error("a Bedrock provider accepted account authentication")
	}
	// Bedrock reasoning choices are Claude's; Vertex AI's are Gemini's.
	if validateEffort(protocol.ConversationModelApiBedrock, protocol.Effort{Kind: "max"}, 0) != nil ||
		validateEffort(protocol.ConversationModelApiVertexAi, protocol.Effort{Kind: "max"}, 0) == nil {
		t.Error("cloud reasoning choices do not follow their protocol")
	}
}

func TestCloudCatalogNamespace(t *testing.T) {
	t.Parallel()
	for endpoint, namespace := range map[string]string{
		"https://bedrock-runtime.us-east-1.amazonaws.com":   "amazon-bedrock",
		"https://bedrock-runtime.eu-west-3.amazonaws.com/":  "amazon-bedrock",
		"https://aiplatform.googleapis.com":                 "google-vertex",
		"https://us-central1-aiplatform.googleapis.com":     "google-vertex",
		"https://aiplatform.eu.rep.googleapis.com":          "google-vertex",
		"http://bedrock-runtime.us-east-1.amazonaws.com":    "",
		"https://bedrock-runtime.us-east-1.amazonaws.com.x": "",
		"https://bedrock-runtime..amazonaws.com":            "",
		"https://bedrock-runtime.a.b.amazonaws.com":         "",
		"https://-aiplatform.googleapis.com":                "",
		"https://us.central1-aiplatform.googleapis.com":     "",
		"https://aiplatform..rep.googleapis.com":            "",
	} {
		if got, err := ChannelCatalogNamespace(endpoint, protocol.AuthenticationApiKey); err != nil || got != namespace {
			t.Errorf("namespace of %s = %q, %v; want %q", endpoint, got, err, namespace)
		}
	}
	for _, options := range []protocol.ConversationConnectionOptions{
		{Kind: "bedrock", Data: protocol.ConversationConnectionOptionsBedrockPayload{Region: "ap-south-1"}},
		{Kind: "vertex_ai", Data: protocol.ConversationConnectionOptionsVertexAiPayload{Project: "p", Location: "global"}},
		{Kind: "vertex_ai", Data: protocol.ConversationConnectionOptionsVertexAiPayload{Project: "p", Location: "us"}},
		{Kind: "vertex_ai", Data: protocol.ConversationConnectionOptionsVertexAiPayload{Project: "p", Location: "asia-northeast1"}},
	} {
		var endpoint string
		if value, ok := options.Bedrock(); ok {
			endpoint = value.Endpoint()
		} else if value, ok := options.VertexAi(); ok {
			endpoint = value.Endpoint()
		}
		if namespace, _ := ChannelCatalogNamespace(endpoint, protocol.AuthenticationApiKey); namespace == "" {
			t.Errorf("the canonical endpoint %s has no catalog namespace", endpoint)
		}
	}
}

func TestCloudPresetsAndDiscovery(t *testing.T) {
	t.Parallel()
	found := map[string]protocol.ConversationModelApi{}
	for _, preset := range Bundled().Builtin {
		for _, route := range preset.Routes {
			if api := RouteAPI(route.Protocol); api == protocol.ConversationModelApiBedrock || api == protocol.ConversationModelApiVertexAi {
				found[preset.ID] = api
				if err := ValidatePreset(preset, true); err != nil {
					t.Errorf("preset %s: %v", preset.ID, err)
				}
			}
		}
	}
	if found["bedrock"] != protocol.ConversationModelApiBedrock || found["vertex-ai"] != protocol.ConversationModelApiVertexAi {
		t.Errorf("cloud presets = %v", found)
	}
	// Their models are entered by ID; listing would need a control-plane API.
	provider := cloudFixture(protocol.ConversationModelApiBedrock, protocol.ConversationConnectionOptions{Kind: "bedrock", Data: protocol.ConversationConnectionOptionsBedrockPayload{Region: "us-east-1"}})
	secret := protocol.NewSecret("bedrock-key")
	if _, err := Discover(t.Context(), provider, &secret); err == nil {
		t.Error("Bedrock discovery did not ask for model IDs")
	}
}
