package model

import (
	"bytes"
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestResponsesWirePreservesContentBreakpoints(t *testing.T) {
	var input []any
	if err := json.Unmarshal([]byte(`[
		{"type":"message","role":"developer","content":[
			{"type":"input_text","text":"stable instructions","prompt_cache_breakpoint":{"mode":"explicit"}}]},
		{"type":"message","role":"user","content":[
			{"type":"input_image","detail":"auto","image_url":"https://example.com/image.png","prompt_cache_breakpoint":{"mode":"explicit","ttl":"30m"}},
			{"type":"input_file","filename":"reference.pdf","file_data":"data:application/pdf;base64,JVBERg==","prompt_cache_breakpoint":{"mode":"explicit","ttl":"12h"}},
			{"type":"input_text","text":"uncached suffix"}]}
	]`), &input); err != nil {
		t.Fatal(err)
	}
	server := serverFixture(t, string(fixture(t, "responses-text.sse")))
	options := map[string]any{"mode": "explicit", "ttl": "12h"}
	// Exercise an already-built input at the wire boundary. Presets cannot replace input.
	m, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "fixture",
		JSON: map[string]any{"input": input, "prompt_cache_options": options}})
	if err != nil {
		t.Fatal(err)
	}
	collect(t, m, requestCall(t, "responses"))
	requests := server.captured()
	if len(requests) != 1 {
		t.Fatalf("got %d HTTP requests, want 1", len(requests))
	}
	checkValue(t, requests[0].Body, "input", input)
	checkValue(t, requests[0].Body, "prompt_cache_options", options)
}

func TestResponsesPresetCacheBreakpoints(t *testing.T) {
	const modelID = "gpt-6.2-astra"
	reserved := protocol.DefaultConversationPresetsRequestPolicy()
	reserved.Models = []string{modelID}
	reserved.Body["input"] = json.RawMessage(`[]`)
	if err := providers.ValidatePolicy(reserved); err == nil {
		t.Fatal("channel preset accepted an override of the conversation input")
	}
	for _, tc := range []struct {
		name     string
		template bool
	}{{"default implicit", false}, {"explicit template", true}} {
		t.Run(tc.name, func(t *testing.T) {
			server := serverFixture(t, string(fixture(t, "responses-text.sse")))
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Enabled = protocol.NewProviderID(), true
			provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey, server.URL
			listed := protocol.DefaultConversationModel()
			listed.ID = modelID
			provider.Models = []protocol.ConversationModel{listed}
			provider.Preset = &protocol.ConversationPresetsPresetRef{Source: protocol.ConversationPresetsSource{Kind: "partner"}, ID: "fixture"}
			preset := protocol.DefaultConversationPresetsBinding()
			if tc.template {
				policy := protocol.DefaultConversationPresetsRequestPolicy()
				policy.Models = []string{modelID}
				policy.Body["prompt_cache_options"] = json.RawMessage(`{"mode":"explicit","ttl":"12h"}`)
				policy.Body["prompt"] = json.RawMessage(`{"id":"pmpt_fixture","variables":{"prefix":{"type":"input_text","text":"stable","prompt_cache_breakpoint":{"mode":"explicit","ttl":"30m"}}}}`)
				preset.Policies = []protocol.ConversationPresetsRequestPolicy{policy}
			}
			provider.Binding = new(&preset)
			if err := providers.ValidateBinding(provider); err != nil {
				t.Fatal(err)
			}
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model = provider.ID, modelID
			config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			key := protocol.NewSecret("fixture")
			binding, err := Capture(provider, config, &key, protocol.NewSessionID())
			if err != nil {
				t.Fatal(err)
			}
			m, err := New(t.Context(), binding)
			if err != nil {
				t.Fatal(err)
			}
			call, err := BuildCall(binding, Request{System: systemText})
			if err != nil {
				t.Fatal(err)
			}
			collect(t, m, call)
			requests := server.captured()
			if len(requests) != 1 {
				t.Fatalf("got %d HTTP requests, want 1", len(requests))
			}
			request := requests[0]
			checkValue(t, request.Body, "prompt_cache_key", binding.Session.String())
			checkValue(t, request.Body, "input.0.content.0", map[string]any{"type": "input_text", "text": systemText})
			if tc.template {
				checkValue(t, request.Body, "prompt_cache_options", map[string]any{"mode": "explicit", "ttl": "12h"})
				checkValue(t, request.Body, "prompt.variables.prefix.prompt_cache_breakpoint", map[string]any{"mode": "explicit", "ttl": "30m"})
			} else if bytes.Contains(request.Raw, []byte(`"prompt_cache_breakpoint"`)) || bytes.Contains(request.Raw, []byte(`"prompt_cache_options"`)) {
				t.Fatalf("implicit request included explicit cache configuration: %s", request.Raw)
			}
		})
	}
}
