package model

import (
	"testing"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/openai"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOpenAIParallelToolCalls(t *testing.T) {
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Enabled = true
	provider.Api = protocol.ConversationModelApiResponses
	provider.Authentication = protocol.AuthenticationApiKey
	provider.Endpoint = "https://api.example.test/v1"
	provider.DefaultModel = "fixture"
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider.ID, "fixture"
	binding := Binding{Provider: provider, Config: config}
	tool := Tool{
		Name:        "read_file",
		Description: "Read a file",
		InputSchema: map[string]any{"type": "object", "properties": map[string]any{"path": map[string]any{"type": "string"}}},
	}

	t.Run("responses with tools defaults parallel_tool_calls to true", func(t *testing.T) {
		call, err := BuildCall(binding, Request{System: systemText, Tools: []Tool{tool}})
		if err != nil {
			t.Fatal(err)
		}
		body, err := requestJSON(transportConfig{Kind: "responses", Model: "fixture"}, call)
		if err != nil {
			t.Fatal(err)
		}
		if got, ok := body["parallel_tool_calls"].(bool); !ok || !got {
			t.Fatalf("parallel_tool_calls = %#v, want true", body["parallel_tool_calls"])
		}
	})

	t.Run("responses without tools omits parallel_tool_calls", func(t *testing.T) {
		call, err := BuildCall(binding, Request{System: systemText})
		if err != nil {
			t.Fatal(err)
		}
		body, err := requestJSON(transportConfig{Kind: "responses", Model: "fixture"}, call)
		if err != nil {
			t.Fatal(err)
		}
		if _, exists := body["parallel_tool_calls"]; exists {
			t.Fatalf("unexpected parallel_tool_calls without tools: %#v", body["parallel_tool_calls"])
		}
	})

	t.Run("explicit provider option and transport overrides win", func(t *testing.T) {
		disabled := false
		call, err := BuildCall(binding, Request{System: systemText, Tools: []Tool{tool}})
		if err != nil {
			t.Fatal(err)
		}
		call.ProviderOptions[openai.Name] = &openai.ResponsesProviderOptions{ParallelToolCalls: &disabled}
		body, err := requestJSON(transportConfig{Kind: "responses", Model: "fixture"}, call)
		if err != nil {
			t.Fatal(err)
		}
		if got, ok := body["parallel_tool_calls"].(bool); !ok || got {
			t.Fatalf("provider option override = %#v, want false", body["parallel_tool_calls"])
		}

		call, err = BuildCall(binding, Request{System: systemText, Tools: []Tool{tool}})
		if err != nil {
			t.Fatal(err)
		}
		body, err = requestJSON(transportConfig{Kind: "responses", Model: "fixture", JSON: map[string]any{"parallel_tool_calls": false}}, call)
		if err != nil {
			t.Fatal(err)
		}
		if got, ok := body["parallel_tool_calls"].(bool); !ok || got {
			t.Fatalf("config.JSON override = %#v, want false", body["parallel_tool_calls"])
		}

		body, err = requestJSON(transportConfig{Kind: "responses", Model: "fixture", Remove: []string{"parallel_tool_calls"}}, call)
		if err != nil {
			t.Fatal(err)
		}
		if _, exists := body["parallel_tool_calls"]; exists {
			t.Fatalf("config.Remove did not remove parallel_tool_calls: %#v", body["parallel_tool_calls"])
		}
	})

	t.Run("chat completions forwards explicit provider option", func(t *testing.T) {
		enabled := true
		call := fantasy.Call{
			Prompt:          fantasy.Prompt{{Role: fantasy.MessageRoleUser, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "hi"}}}},
			Tools:           []fantasy.Tool{fantasy.FunctionTool(tool)},
			ProviderOptions: fantasy.ProviderOptions{openai.Name: &openai.ProviderOptions{ParallelToolCalls: &enabled}},
		}
		body, err := requestJSON(transportConfig{Kind: "chat", Model: "fixture"}, call)
		if err != nil {
			t.Fatal(err)
		}
		if got, ok := body["parallel_tool_calls"].(bool); !ok || !got {
			t.Fatalf("chat parallel_tool_calls = %#v, want true", body["parallel_tool_calls"])
		}
	})
}
