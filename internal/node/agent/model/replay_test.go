package model

import (
	"encoding/json"
	"reflect"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func replayFixtureBinding() Binding {
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Api = protocol.NewProviderID(), protocol.ConversationModelApiAnthropic
	provider.Endpoint = "https://fixture.invalid/v1"
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider.ID, "claude-opus-5-5"
	return Binding{Provider: provider, Config: config, Session: protocol.NewSessionID()}
}

func replayFixtureCall() fantasy.Call {
	return fantasy.Call{
		Prompt: fantasy.Prompt{
			{Role: fantasy.MessageRoleSystem, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "Stable instructions"}}},
			{Role: fantasy.MessageRoleUser, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "Use the tool"}}},
		},
		Tools: []fantasy.Tool{fantasy.FunctionTool{Name: "echo", Description: "Return its input", InputSchema: map[string]any{"type": "object"}}},
	}
}

func replayFixtureAssistant() fantasy.Message {
	return fantasy.Message{Role: fantasy.MessageRoleAssistant, Content: []fantasy.MessagePart{
		fantasy.ReasoningPart{Text: "Native reasoning", ProviderOptions: fantasy.ProviderOptions{anthropic.Name: &anthropic.ReasoningOptionMetadata{Signature: "native-signature"}}},
		fantasy.ReasoningPart{ProviderOptions: fantasy.ProviderOptions{anthropic.Name: &anthropic.ReasoningOptionMetadata{RedactedData: "redacted-data"}}},
		fantasy.TextPart{Text: "I will use the tool"},
		fantasy.ToolCallPart{ToolCallID: "toolu_echo", ToolName: "echo", Input: `{"value":"ok"}`},
	}}
}

func replayFixtureResult() fantasy.Message {
	return fantasy.Message{Role: fantasy.MessageRoleTool, Content: []fantasy.MessagePart{
		fantasy.ToolResultPart{ToolCallID: "toolu_echo", Output: fantasy.ToolResultOutputContentText{Text: "ok"}},
	}}
}

func TestScopeReplayPreservesOnlyMatchingNativeThinking(t *testing.T) {
	binding := replayFixtureBinding()
	initial := replayFixtureCall()
	prefix, err := scopeReplay(binding, &initial, nil)
	if err != nil || len(prefix) != 64 {
		t.Fatalf("initial replay prefix = %q, %v", prefix, err)
	}
	for _, row := range []struct {
		name, origin string
		keep         bool
	}{
		{"matching", prefix, true},
		{"unknown", "", false},
		{"different", strings.Repeat("0", 64), false},
	} {
		t.Run(row.name, func(t *testing.T) {
			call := replayFixtureCall()
			call.Prompt = append(call.Prompt, replayFixtureAssistant(), replayFixtureResult())
			original := call
			before, err := json.Marshal(original)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := scopeReplay(binding, &call, map[int]string{2: row.origin}); err != nil {
				t.Fatal(err)
			}
			if len(call.Prompt) != 4 || slices.ContainsFunc(call.Prompt[2].Content, func(part fantasy.MessagePart) bool {
				return part.GetType() == fantasy.ContentTypeReasoning
			}) != row.keep {
				t.Fatal("native thinking did not follow its captured prefix")
			}
			content := call.Prompt[2].Content
			if !reflect.DeepEqual(content[len(content)-2:], original.Prompt[2].Content[2:]) || !reflect.DeepEqual(call.Prompt[3], original.Prompt[3]) {
				t.Fatal("replay filtering changed text, tool calls or their results")
			}
			after, err := json.Marshal(original)
			if err != nil || string(before) != string(after) {
				t.Fatal("replay projection changed its original history")
			}
		})
	}
}

func TestScopeReplayRetainsFreshThinkingAfterAChangedPrefix(t *testing.T) {
	binding := replayFixtureBinding()
	initial := replayFixtureCall()
	originalPrefix, err := scopeReplay(binding, &initial, nil)
	if err != nil {
		t.Fatal(err)
	}
	for _, change := range []string{"system", "tools", "messages"} {
		t.Run(change, func(t *testing.T) {
			call := replayFixtureCall()
			switch change {
			case "system":
				call.Prompt[0].Content = []fantasy.MessagePart{fantasy.TextPart{Text: "Updated instructions"}}
			case "tools":
				call.Tools = append(call.Tools, fantasy.FunctionTool{Name: "loaded_tool", InputSchema: map[string]any{"type": "object"}})
			case "messages":
				call.Prompt[1].Content = []fantasy.MessagePart{fantasy.TextPart{Text: "Changed earlier input"}}
			}
			call.Prompt = append(call.Prompt, replayFixtureAssistant(), replayFixtureResult())
			canonical := call
			freshPrefix, err := scopeReplay(binding, &call, map[int]string{2: originalPrefix})
			if err != nil {
				t.Fatal(err)
			}
			if len(call.Prompt[2].Content) != 2 {
				t.Fatal("a changed prefix retained invalid native thinking")
			}
			// Rebuild from unchanged canonical records, including their old
			// thinking, then add a reply made under the filtered request.
			next := canonical
			next.Prompt = append(slices.Clone(canonical.Prompt), replayFixtureAssistant())
			if _, err := scopeReplay(binding, &next, map[int]string{2: originalPrefix, 4: freshPrefix}); err != nil {
				t.Fatal(err)
			}
			if len(next.Prompt[2].Content) != 2 || len(next.Prompt[4].Content) != 4 {
				t.Fatal("replay filtering lost new valid thinking or restored old invalid thinking")
			}
		})
	}
}

func TestScopeReplayIgnoresCacheAnnotationsButKeepsBusinessFields(t *testing.T) {
	binding := replayFixtureBinding()
	initial := replayFixtureCall()
	prefix, err := scopeReplay(binding, &initial, nil)
	if err != nil {
		t.Fatal(err)
	}
	decorated := replayFixtureCall()
	cache := fantasy.ProviderOptions{anthropic.Name: &anthropic.ProviderCacheControlOptions{CacheControl: anthropic.CacheControl{Type: "ephemeral"}}}
	decorated.Prompt[0].ProviderOptions = cache
	decorated.Prompt[1].ProviderOptions = cache
	decorated.Prompt = append(decorated.Prompt, replayFixtureAssistant())
	decorated.Prompt[2].ProviderOptions = cache
	if _, err := scopeReplay(binding, &decorated, map[int]string{2: prefix}); err != nil {
		t.Fatal(err)
	}
	if len(decorated.Prompt[2].Content) != 4 {
		t.Fatal("cache annotations invalidated an otherwise matching prefix")
	}
	schemaPrefix := func(schema string) string {
		t.Helper()
		call := replayFixtureCall()
		SetToolSchemas(&call, map[string]json.RawMessage{"echo": json.RawMessage(schema)})
		value, err := scopeReplay(binding, &call, nil)
		if err != nil {
			t.Fatal(err)
		}
		return value
	}
	first := schemaPrefix(`{"type":"object","properties":{"cache_control":{"type":"string","default":"first"}}}`)
	if reordered := schemaPrefix(`{"properties":{"cache_control":{"default":"first","type":"string"}},"type":"object"}`); reordered != first {
		t.Fatal("JSON key order changed the replay prefix")
	}
	if changed := schemaPrefix(`{"type":"object","properties":{"cache_control":{"type":"string","default":"second"}}}`); changed == first {
		t.Fatal("a business field named cache_control was ignored")
	}
}

func TestScopeReplayRemovesEmptyAssistantEnvelopesAndFramesMessages(t *testing.T) {
	binding := replayFixtureBinding()
	call := replayFixtureCall()
	without := replayFixtureCall()
	call.Prompt = append(call.Prompt, fantasy.Message{Role: fantasy.MessageRoleAssistant, Content: replayFixtureAssistant().Content[:2]})
	actual, err := scopeReplay(binding, &call, nil)
	if err != nil {
		t.Fatal(err)
	}
	want, err := scopeReplay(binding, &without, nil)
	if err != nil || len(call.Prompt) != 2 || actual != want {
		t.Fatal("discarded thinking left an empty assistant envelope in the request")
	}
	framed := func(first, second string) string {
		call := replayFixtureCall()
		call.Prompt[1].Content = []fantasy.MessagePart{fantasy.TextPart{Text: first}}
		call.Prompt = append(call.Prompt, fantasy.Message{Role: fantasy.MessageRoleUser, Content: []fantasy.MessagePart{fantasy.TextPart{Text: second}}})
		value, err := scopeReplay(binding, &call, nil)
		if err != nil {
			t.Fatal(err)
		}
		return value
	}
	if framed("a", "bc") == framed("ab", "c") {
		t.Fatal("different message boundaries produced the same replay prefix")
	}
}

func TestScopeReplayLeavesOtherProtocolsAndLegacyModelsUntouched(t *testing.T) {
	for _, row := range []struct {
		api   protocol.ConversationModelApi
		model string
	}{
		{protocol.ConversationModelApiAnthropic, "claude-haiku-4-5"},
		{protocol.ConversationModelApiChatCompletions, "claude-opus-5-5"},
		{protocol.ConversationModelApiResponses, "gpt-6.1-sol"},
		{protocol.ConversationModelApiGemini, "gemini-fixture"},
	} {
		t.Run(string(row.api)+"/"+row.model, func(t *testing.T) {
			binding := replayFixtureBinding()
			binding.Provider.Api, binding.Config.Model = row.api, row.model
			call := replayFixtureCall()
			call.Prompt = append(call.Prompt, replayFixtureAssistant())
			before, err := json.Marshal(call)
			if err != nil {
				t.Fatal(err)
			}
			prefix, err := scopeReplay(binding, &call, nil)
			after, marshalErr := json.Marshal(call)
			if err != nil || marshalErr != nil || prefix != "" || string(before) != string(after) {
				t.Fatal("native replay scoping changed another protocol or a legacy model")
			}
		})
	}
}

func TestScopeReplayScopesTheTransportFlavor(t *testing.T) {
	binding := replayFixtureBinding()
	binding.Provider.Authentication = protocol.AuthenticationApiKey
	initial := replayFixtureCall()
	prefix, err := scopeReplay(binding, &initial, nil)
	if err != nil {
		t.Fatal(err)
	}
	binding.Provider.Authentication = protocol.AuthenticationClaudeOAuth
	continued := replayFixtureCall()
	continued.Prompt = append(continued.Prompt, replayFixtureAssistant())
	if _, err := scopeReplay(binding, &continued, map[int]string{2: prefix}); err != nil {
		t.Fatal(err)
	}
	if len(continued.Prompt[2].Content) != 2 {
		t.Fatal("changing transport identity retained old bound thinking")
	}
}
