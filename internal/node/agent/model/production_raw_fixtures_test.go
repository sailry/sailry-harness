package model

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// These cases exercise the production factory against independently captured
// source requests, including captured policy member order and opaque options.
func productionRawRustFixtures(t *testing.T, captures map[string]rawRustCapture) {
	t.Helper()
	for _, row := range []struct {
		name, kind, model, suffix string
		api                       protocol.ConversationModelApi
	}{
		{"factory-responses-speed-policy", "responses", "custom/reasoner", "/gateway/v1", protocol.ConversationModelApiResponses},
		{"factory-gemini-generation-policy", "google", "gemini-3-flash-preview", "/proxy/v1beta", protocol.ConversationModelApiGemini},
	} {
		t.Run(row.name, func(t *testing.T) {
			server := serverFixture(t, string(fixture(t, row.kind+"-text.sse")))
			provider := protocol.DefaultConversationProvider()
			provider.ID, _ = protocol.ParseProviderID(rawRustProvider)
			provider.Name = "Authorization fixture"
			provider.Api = row.api
			provider.Authentication = protocol.AuthenticationApiKey
			provider.Enabled = true
			provider.Endpoint = server.URL + row.suffix
			provider.DefaultModel = row.model
			metadata := protocol.DefaultConversationModel()
			metadata.ID, metadata.Context, metadata.Output, metadata.Tools = row.model, 100000, 512, true
			metadata.Reasoning = row.kind == "responses"
			if metadata.Reasoning {
				capabilities := protocol.DefaultConversationDiscoveryCapabilities()
				capabilities.Features = []protocol.ConversationFeaturesFeature{{ID: "speed", Options: []protocol.ConversationFeaturesFeatureOption{{ID: "fast"}}}}
				metadata.Capabilities = &capabilities
			}
			provider.Models = []protocol.ConversationModel{metadata}
			policy := protocol.DefaultConversationPresetsRequestPolicy()
			policy.Models = []string{row.model}
			if row.kind == "responses" {
				policy.Body["metadata"] = json.RawMessage(`{"fixture":"production"}`)
			} else {
				policy.Body["generationConfig"] = json.RawMessage(`{"stopSequences":["END"],"candidateCount":1}`)
			}
			preset := protocol.DefaultConversationPresetsBinding()
			preset.Policies = []protocol.ConversationPresetsRequestPolicy{policy}
			presetPointer := &preset
			provider.Binding = &presetPointer
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model = provider.ID, row.model
			config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			config.Effort = protocol.Effort{Kind: "default"}
			if row.kind == "responses" {
				config.Effort = protocol.Effort{Kind: "high"}
				config.Features = map[string]string{"speed": "fast"}
			}
			session, _ := protocol.ParseSessionID(sessionID)
			secret := protocol.NewSecret("fixed-access")
			binding, err := Capture(provider, config, &secret, session)
			if err != nil {
				t.Fatal(err)
			}
			model, err := New(t.Context(), binding)
			if err != nil {
				t.Fatal(err)
			}
			record := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Read the file")), policyClock{}.now)
			schema := json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}`)
			var input map[string]any
			if err := json.Unmarshal(schema, &input); err != nil {
				t.Fatal(err)
			}
			call, err := BuildCall(binding, Request{
				System: systemText, Records: []canonical.Record{record},
				Tools:       []Tool{{Name: "read", Description: "Read a file", InputSchema: input}},
				ToolSchemas: map[string]json.RawMessage{"read": schema},
			})
			if err != nil {
				t.Fatal(err)
			}
			collect(t, model, call)
			requests := server.captured()
			if len(requests) != 1 {
				t.Fatalf("production model calls emitted %d HTTP requests", len(requests))
			}
			golden, exists := captures[row.name+"/1"]
			if !exists {
				t.Fatalf("missing independent source factory capture %s", row.name)
			}
			checkRawRustCapture(t, golden, requests[0])
			if row.kind == "google" {
				checkValue(t, requests[0].Body, "generationConfig.stopSequences", []any{"END"})
				checkValue(t, requests[0].Body, "generationConfig.candidateCount", float64(1))
			}
		})
	}
}
