package model

import (
	"reflect"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestClaude55CatalogCompatibility(t *testing.T) {
	for _, family := range []string{"haiku", "sonnet", "opus"} {
		t.Run(family, func(t *testing.T) {
			canonical := "claude-" + family + "-5-5"
			entry, ok := catalogEntry("anthropic", canonical)
			if !ok || !entry.Rig.Adaptive {
				t.Fatalf("%s adaptive capability = %v, found=%v", canonical, entry.Rig.Adaptive, ok)
			}
			for _, alias := range []string{
				"claude-" + family + "-5.5",
				canonical + "-20261007",
				"claude-" + family + "-5.5-20261007",
			} {
				if got, ok := catalogEntry("anthropic", alias); !ok || !reflect.DeepEqual(got, entry) {
					t.Errorf("%s lost canonical capabilities: found=%v, got=%+v", alias, ok, got)
				}
			}
		})
	}
	for _, lookup := range []struct{ provider, model string }{
		{"anthropic", "claude-haiku-5.6"},
		{"anthropic", "claude-haiku-5.5-custom"},
		{"openai", "claude-haiku-5.5"},
		{"fixture", "claude-haiku-5-5"},
	} {
		if entry, ok := catalogEntry(lookup.provider, lookup.model); ok || entry.Rig.Adaptive {
			t.Errorf("unlisted %s/%s gained Claude capabilities: %+v", lookup.provider, lookup.model, entry)
		}
	}
}

func TestClaudeThinkingParameters(t *testing.T) {
	for _, row := range []struct {
		name, model, effort string
		api                 protocol.ConversationModelApi
		thinking, output    map[string]any
		reasoning           any
	}{
		{
			name: "dotted high", model: "claude-haiku-5.5", effort: "high", api: protocol.ConversationModelApiAnthropic,
			thinking: map[string]any{"type": "adaptive", "display": "summarized"}, output: map[string]any{"effort": "high"},
		},
		{
			name: "canonical high", model: "claude-haiku-5-5", effort: "high", api: protocol.ConversationModelApiAnthropic,
			thinking: map[string]any{"type": "adaptive", "display": "summarized"}, output: map[string]any{"effort": "high"},
		},
		{
			name: "dotted none", model: "claude-haiku-5.5", effort: "none", api: protocol.ConversationModelApiAnthropic,
			thinking: map[string]any{"type": "disabled"},
		},
		{
			name: "canonical none", model: "claude-haiku-5-5", effort: "none", api: protocol.ConversationModelApiAnthropic,
			thinking: map[string]any{"type": "disabled"},
		},
		{
			name: "sonnet dotted none", model: "claude-sonnet-5.5", effort: "none", api: protocol.ConversationModelApiAnthropic,
			thinking: map[string]any{"type": "between_tools"},
		},
		{
			name: "sonnet canonical none", model: "claude-sonnet-5-5", effort: "none", api: protocol.ConversationModelApiAnthropic,
			thinking: map[string]any{"type": "between_tools"},
		},
		{
			name: "opus always on", model: "claude-opus-5-5", effort: "none", api: protocol.ConversationModelApiAnthropic,
			thinking: map[string]any{"type": "adaptive", "display": "summarized"}, output: map[string]any{"effort": "high"},
		},
		{
			name: "legacy high", model: "claude-haiku-4-5", effort: "high", api: protocol.ConversationModelApiAnthropic,
			thinking: map[string]any{"type": "enabled", "budget_tokens": int64(16000), "display": "summarized"},
		},
		{
			name: "chat completions", model: "claude-haiku-5.5", effort: "high", api: protocol.ConversationModelApiChatCompletions,
			reasoning: "high",
		},
	} {
		t.Run(row.name, func(t *testing.T) {
			provider := protocol.DefaultConversationProvider()
			provider.Api, provider.Authentication = row.api, protocol.AuthenticationApiKey
			provider.Endpoint = "https://fixture.invalid/v1"
			config := protocol.DefaultSessionConfig()
			config.Model, config.Effort = row.model, protocol.Effort{Kind: row.effort}
			binding := Binding{Provider: provider, Config: config}
			output := int64(20000)
			params := requestParameters(binding, &fantasy.Call{MaxOutputTokens: &output})
			thinking, _ := params.Body["thinking"].(map[string]any)
			if !reflect.DeepEqual(thinking, row.thinking) {
				t.Errorf("thinking = %#v, want %#v", thinking, row.thinking)
			}
			outputConfig, _ := params.Body["output_config"].(map[string]any)
			if !reflect.DeepEqual(outputConfig, row.output) {
				t.Errorf("output_config = %#v, want %#v", outputConfig, row.output)
			}
			if got := params.Body["reasoning_effort"]; got != row.reasoning {
				t.Errorf("reasoning_effort = %#v, want %#v", got, row.reasoning)
			}
			transport, err := transportBinding(binding)
			if err != nil || transport.Model != row.model {
				t.Errorf("wire model = %q, error=%v; want %q", transport.Model, err, row.model)
			}
		})
	}
}
