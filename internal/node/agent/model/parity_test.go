package model

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"sync"
	"testing"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"charm.land/fantasy/providers/google"
	"charm.land/fantasy/providers/openai"
	"github.com/cespare/xxhash/v2"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const sessionID = "11111111-1111-4111-8111-111111111111"
const systemText = "Captured instructions"

type capturedRequest struct {
	Path    string
	Headers http.Header
	Raw     []byte
	Body    map[string]any
}

type fixtureServer struct {
	*httptest.Server
	mu       sync.Mutex
	requests []capturedRequest
}

func serverFixture(t *testing.T, responses ...string) *fixtureServer {
	t.Helper()
	fixture := &fixtureServer{}
	fixture.Server = httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		raw, err := io.ReadAll(request.Body)
		if err != nil {
			t.Error(err)
			return
		}
		var body map[string]any
		if err := json.Unmarshal(raw, &body); err != nil {
			t.Error(err)
			return
		}
		fixture.mu.Lock()
		index := len(fixture.requests)
		fixture.requests = append(fixture.requests, capturedRequest{request.URL.RequestURI(), request.Header.Clone(), raw, body})
		fixture.mu.Unlock()
		if index >= len(responses) {
			http.Error(writer, "Unexpected extra model request", http.StatusInternalServerError)
			return
		}
		writer.Header().Set("Content-Type", "text/event-stream")
		io.WriteString(writer, responses[index])
	}))
	t.Cleanup(fixture.Close)
	return fixture
}

func (server *fixtureServer) captured() []capturedRequest {
	server.mu.Lock()
	defer server.mu.Unlock()
	return append([]capturedRequest(nil), server.requests...)
}

func fixture(t *testing.T, name string) []byte {
	t.Helper()
	body, err := os.ReadFile(filepath.Join("testdata", name))
	if err != nil {
		t.Fatal(err)
	}
	return body
}

func objectFixture(t *testing.T, name string) map[string]any {
	t.Helper()
	var value map[string]any
	if err := json.Unmarshal(fixture(t, name), &value); err != nil {
		t.Fatal(err)
	}
	return value
}

func headersFixture(t *testing.T, name string) map[string]string {
	t.Helper()
	var value map[string]string
	if err := json.Unmarshal(fixture(t, name), &value); err != nil {
		t.Fatal(err)
	}
	return value
}

func valueAt(value any, path string) any {
	for _, key := range strings.Split(path, ".") {
		switch current := value.(type) {
		case map[string]any:
			value = current[key]
		case []any:
			index, err := strconv.Atoi(key)
			if err != nil || index < 0 || index >= len(current) {
				return nil
			}
			value = current[index]
		default:
			return nil
		}
	}
	return value
}

func checkValue(t *testing.T, body any, path string, expected any) {
	t.Helper()
	if actual := valueAt(body, path); !reflect.DeepEqual(actual, expected) {
		t.Errorf("request %s = %#v; source expects %#v", path, actual, expected)
	}
}

func collect(t *testing.T, model fantasy.LanguageModel, call fantasy.Call) []fantasy.StreamPart {
	t.Helper()
	stream, err := model.Stream(t.Context(), call)
	if err != nil {
		t.Errorf("stream start: %v", err)
		return nil
	}
	var parts []fantasy.StreamPart
	for part := range stream {
		parts = append(parts, part)
		if part.Type == fantasy.StreamPartTypeError {
			t.Errorf("stream failure: %v", part.Error)
		}
	}
	return parts
}

func requestCall(t *testing.T, kind string) fantasy.Call {
	t.Helper()
	maximum := int64(512)
	call := fantasy.Call{
		Prompt: fantasy.Prompt{
			{Role: fantasy.MessageRoleSystem, Content: []fantasy.MessagePart{fantasy.TextPart{Text: systemText}}},
			{Role: fantasy.MessageRoleUser, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "Read the file"}}},
		},
		MaxOutputTokens: &maximum,
		Tools:           []fantasy.Tool{fantasy.FunctionTool{Name: "read", Description: "Read a file", InputSchema: objectFixture(t, "schema.json")}},
	}
	if kind == "responses" {
		call.Prompt = call.Prompt[1:]
		instructions := systemText
		call.ProviderOptions = openai.NewResponsesProviderOptions(&openai.ResponsesProviderOptions{Instructions: &instructions})
	}
	return call
}

func sourceRawTools(t *testing.T, kind string) []any {
	t.Helper()
	schema := objectFixture(t, "schema.json")
	switch kind {
	case "anthropic":
		return []any{map[string]any{"name": "read", "description": "Read a file", "input_schema": schema, "cache_control": map[string]any{"type": "ephemeral"}}}
	case "responses":
		return []any{map[string]any{"type": "function", "name": "read", "description": "Read a file", "parameters": schema, "strict": false}}
	default:
		return []any{map[string]any{"type": "function", "function": map[string]any{"name": "read", "description": "Read a file", "parameters": schema, "strict": false}}}
	}
}

func replay(call *fantasy.Call, kind string) {
	parts := []fantasy.MessagePart{}
	tool := fantasy.ToolCallPart{ToolCallID: "stable-call", ToolName: "read", Input: `{"path":"file"}`}
	switch kind {
	case "anthropic":
		parts = append(parts,
			fantasy.ReasoningPart{Text: "Consider", ProviderOptions: fantasy.ProviderOptions{anthropic.Name: &anthropic.ReasoningOptionMetadata{Signature: "signed-thinking"}}},
			fantasy.ReasoningPart{ProviderOptions: fantasy.ProviderOptions{anthropic.Name: &anthropic.ReasoningOptionMetadata{RedactedData: "redacted-data"}}},
		)
	case "google":
		parts = append(parts, fantasy.ReasoningPart{ProviderOptions: fantasy.ProviderOptions{google.Name: &google.ReasoningMetadata{Signature: "signed-thought", ToolID: "stable-call"}}})
	case "responses":
		encrypted := "encrypted-fixture"
		parts = append(parts, fantasy.ReasoningPart{Text: "Previous reasoning", ProviderOptions: fantasy.ProviderOptions{openai.Name: &openai.ResponsesReasoningMetadata{ItemID: "reasoning", EncryptedContent: &encrypted}}})
	}
	parts = append(parts, tool)
	call.Prompt = append(call.Prompt,
		fantasy.Message{Role: fantasy.MessageRoleAssistant, Content: parts},
		fantasy.Message{Role: fantasy.MessageRoleTool, Content: []fantasy.MessagePart{fantasy.ToolResultPart{ToolCallID: "stable-call", Output: fantasy.ToolResultOutputContentText{Text: "File contents"}}}},
	)
}

func saveCaptures(t *testing.T, name string, captured []capturedRequest) {
	t.Helper()
	directory := filepath.Join(t.TempDir(), name)
	if err := os.MkdirAll(directory, 0o700); err != nil {
		t.Fatal(err)
	}
	for index, request := range captured {
		t.Logf("raw request capture %s step %d: %q", name, index+1, request.Raw)
		if err := os.WriteFile(filepath.Join(directory, fmt.Sprintf("request-%d.raw.json", index+1)), request.Raw, 0o600); err != nil {
			t.Fatal(err)
		}
		envelope, err := json.MarshalIndent(struct {
			Path    string      `json:"path"`
			Headers http.Header `json:"headers"`
		}{request.Path, request.Headers}, "", "  ")
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(directory, fmt.Sprintf("request-%d.headers.json", index+1)), envelope, 0o600); err != nil {
			t.Fatal(err)
		}
	}
}

func TestRequestParity(t *testing.T) {
	t.Run("user reasoning tag outranks discovery", func(t *testing.T) {
		for _, row := range []struct {
			name                    string
			tag, custom, discovered bool
			want                    any
		}{
			{"custom off over discovered on", false, true, true, nil},
			{"custom on over discovered off", true, true, false, "high"},
			{"discovery decides an untagged model", false, false, true, "high"},
			{"discovery disables an untagged model", true, false, false, nil},
		} {
			provider := protocol.DefaultConversationProvider()
			provider.Api, provider.Authentication = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey
			entry := protocol.DefaultConversationModel()
			entry.ID, entry.Reasoning, entry.CustomCapabilities = "custom/model", row.tag, row.custom
			capabilities := protocol.DefaultConversationDiscoveryCapabilities()
			capabilities.Reasoning = &row.discovered
			entry.Capabilities = &capabilities
			provider.Models = []protocol.ConversationModel{entry}
			config := protocol.DefaultSessionConfig()
			config.Model, config.Effort = entry.ID, protocol.Effort{Kind: "high"}
			params := requestParameters(Binding{Provider: provider, Config: config}, &fantasy.Call{})
			if got := params.Body["reasoning_effort"]; got != row.want {
				t.Errorf("%s: reasoning_effort = %v, want %v", row.name, got, row.want)
			}
		}
	})
	t.Run("provider source annotations survive official SDK streams", func(t *testing.T) {
		for _, kind := range []string{"responses", "chat", "anthropic", "google"} {
			t.Run(kind, func(t *testing.T) {
				stream := ""
				switch kind {
				case "responses":
					stream = "data: " + `{"type":"response.completed","response":{"id":"response","status":"completed","output":[{"id":"message","type":"message","role":"assistant","content":[{"type":"output_text","text":"结果🙂","annotations":[{"type":"url_citation","url":"https://example.com/source","title":"Public source","start_index":0,"end_index":3}]}]}]}}` + "\n\n"
				case "chat":
					stream = "data: " + `{"id":"chat","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"结果🙂","annotations":[{"type":"url_citation","url_citation":{"url":"https://example.com/source","title":"Public source","start_index":0,"end_index":3}}]},"finish_reason":null}]}` + "\n\ndata: " + `{"id":"chat","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}` + "\n\ndata: [DONE]\n\n"
				case "anthropic":
					frames := []string{`{"type":"message_start","message":{"id":"message","type":"message","role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}`, `{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}`, `{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"结果🙂"}}`, `{"type":"content_block_delta","index":0,"delta":{"type":"citations_delta","citation":{"type":"web_search_result_location","url":"https://example.com/source","title":"Public source","cited_text":"Original source","encrypted_index":"opaque"}}}`, `{"type":"content_block_stop","index":0}`, `{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}`, `{"type":"message_stop"}`}
					for _, frame := range frames {
						var value map[string]any
						if err := json.Unmarshal([]byte(frame), &value); err != nil {
							t.Fatal(err)
						}
						stream += "event: " + value["type"].(string) + "\ndata: " + frame + "\n\n"
					}
				case "google":
					stream = "data: " + `{"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"结果🙂"}]},"finishReason":"STOP","groundingMetadata":{"groundingChunks":[{"web":{"uri":"https://example.com/source","title":"Public source"}}],"groundingSupports":[{"segment":{"startIndex":0,"endIndex":10,"text":"结果🙂"},"groundingChunkIndices":[0]}],"searchEntryPoint":{"renderedContent":"<div>Search suggestions</div>"}}}]}` + "\n\n"
				}
				server := serverFixture(t, stream)
				name := "fixture"
				if kind == "google" {
					name = "gemini-3.1-pro"
				}
				language, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: name, Endpoint: server.URL, Key: "captured-key"})
				if err != nil {
					t.Fatal(err)
				}
				response, err := language.Stream(t.Context(), requestCall(t, kind))
				if err != nil {
					t.Fatal(err)
				}
				var terminal fantasy.StreamPart
				for part := range response {
					if part.Type == fantasy.StreamPartTypeError {
						t.Fatal(part.Error)
					}
					if part.Type == fantasy.StreamPartTypeFinish {
						terminal = part
					}
				}
				sources, ok := StreamSources(terminal)
				if !ok {
					t.Fatal("SDK lost native source metadata")
				}
				if kind == "google" {
					if len(sources.Grounding) != 1 || sources.Grounding[0].URI != "https://example.com/source" || sources.SearchSuggestions == nil || len(sources.Citations) != 0 {
						t.Fatal(sources)
					}
				} else {
					if len(sources.Citations) != 1 || sources.Citations[0].URI == nil || *sources.Citations[0].URI != "https://example.com/source" {
						t.Fatal(sources)
					}
					citation := sources.Citations[0]
					if kind == "anthropic" {
						if citation.Start != nil || citation.End != nil {
							t.Fatal("source document positions became answer offsets", citation)
						}
					} else if citation.Start == nil || citation.End == nil || *citation.Start != 0 || *citation.End != 3 {
						t.Fatal("Unicode citation range changed", citation)
					}
					*sources.Citations[0].URI = "changed"
					again, _ := StreamSources(terminal)
					if *again.Citations[0].URI != "https://example.com/source" {
						t.Fatal("source snapshot was mutable")
					}
				}
			})
		}
	})

	rawRustFixtures(t)
	capturedPolicyFixtures(t)
	for _, row := range []struct {
		name, kind, model, endpoint, path, tier string
		oauth                                   bool
	}{
		{"anthropic_key", "anthropic", "claude-opus-4-8", "/proxy/v1/messages", "/proxy/v1/messages", "", false},
		{"anthropic_adaptive", "anthropic", "claude-opus-4-8", "/v1", "/v1/messages", "", false},
		{"anthropic_manual", "anthropic", "claude-opus-4-8", "/v1/messages", "/v1/messages", "", false},
		{"anthropic_oauth", "anthropic", "claude-opus-4-8", "/v1", "/v1/messages?beta=true", "", true},
		{"responses_priority", "responses", "fixture", "/gateway/v1", "/gateway/v1/responses", "priority", false},
		{"responses_ultrafast", "responses", "fixture", "/gateway/v1", "/gateway/v1/responses", "ultrafast", false},
		{"responses_foreign_reasoner", "responses", "custom/reasoner", "/v1", "/v1/responses", "priority", false},
		{"codex_oauth", "responses", "fixture-model", "/backend-api/codex", "/backend-api/codex/responses", "", true},
		{"compatible_base", "chat", "uncatalogued", "/gateway/v1", "/gateway/v1/chat/completions", "", false},
		{"compatible_full", "chat", "uncatalogued", "/gateway/v1/chat/completions", "/gateway/v1/chat/completions", "", false},
		{"deepseek_reasoning", "chat", "deepseek/deepseek-r1", "/v1", "/v1/chat/completions", "", false},
		{"gemini_schema_disabled_thinking", "google", "gemini-3-flash-preview", "/proxy", "/proxy/v1beta/models/gemini-3-flash-preview:streamGenerateContent?alt=sse", "", false},
	} {
		t.Run(row.name, func(t *testing.T) {
			prefix := row.kind
			if prefix == "responses" || prefix == "anthropic" || prefix == "google" {
				prefix += "-tool"
			} else {
				prefix = "chat-text"
			}
			firstResponse := string(fixture(t, prefix+".sse"))
			if row.oauth && row.kind == "anthropic" {
				firstResponse = strings.ReplaceAll(firstResponse, `"name":"read"`, `"name":"_read"`)
			}
			server := serverFixture(t, firstResponse, string(fixture(t, row.kind+"-text.sse")))
			config := transportConfig{Kind: row.kind, Model: row.model, Endpoint: server.URL + row.endpoint, Key: "captured-key", JSON: map[string]any{}, Headers: map[string]string{}, OAuth: row.oauth}
			call := requestCall(t, row.kind)
			switch row.kind {
			case "responses":
				config.JSON = map[string]any{"store": false, "prompt_cache_key": "captured-session", "reasoning": map[string]any{"effort": "xhigh", "summary": "auto"}, "include": []string{"reasoning.encrypted_content"}, "tools": sourceRawTools(t, "responses")}
				if row.tier != "" {
					config.JSON["service_tier"] = row.tier
				}
				config.Remove = []string{"previous_response_id", "tool_choice"}
				if row.oauth {
					config.Key = "fixed-access"
					config.Headers = headersFixture(t, "codex-oauth-headers.json")
					config.JSON["prompt_cache_key"] = sessionID
					config.Remove = append(config.Remove, "max_output_tokens", "stream_options", "temperature")
				}
			case "anthropic":
				extra := map[string]any{"tools": sourceRawTools(t, "anthropic"), "thinking": map[string]any{"type": "disabled"}}
				if row.name == "anthropic_adaptive" || row.oauth {
					extra["thinking"] = map[string]any{"type": "adaptive", "display": "omitted"}
					extra["output_config"] = map[string]any{"effort": "xhigh"}
					maximum := int64(16000)
					call.MaxOutputTokens = &maximum
				}
				if row.name == "anthropic_manual" {
					extra["thinking"] = map[string]any{"type": "enabled", "budget_tokens": 4096}
					maximum := int64(16000)
					call.MaxOutputTokens = &maximum
				}
				call.ProviderOptions = anthropic.NewProviderOptions(&anthropic.ProviderOptions{ExtraBody: extra})
				cache := anthropic.NewProviderCacheControlOptions(&anthropic.ProviderCacheControlOptions{CacheControl: anthropic.CacheControl{Type: "ephemeral"}})
				call.Prompt[0].ProviderOptions = cache
				call.Prompt = call.Prompt[:1]
				for index := 0; index < 5; index++ {
					role := fantasy.MessageRoleUser
					if index%2 == 1 {
						role = fantasy.MessageRoleAssistant
					}
					message := fantasy.Message{Role: role, Content: []fantasy.MessagePart{fantasy.TextPart{Text: fmt.Sprintf("Message %d", index)}}}
					if index == 2 || index == 4 {
						message.ProviderOptions = cache
					}
					call.Prompt = append(call.Prompt, message)
				}
				if row.oauth {
					config.Key = ""
					config.Headers = map[string]string{"authorization": "Bearer fixed-access", "x-claude-code-session-id": sessionID}
					extra["metadata"] = claudeMetadata("install", "session", "account")
				}
			case "google":
				zero, thoughts := int64(0), true
				call.ProviderOptions = fantasy.ProviderOptions{google.Name: &google.ProviderOptions{ThinkingConfig: &google.ThinkingConfig{ThinkingBudget: &zero, IncludeThoughts: &thoughts}}}
			case "chat":
				config.Key = "compatible-key"
				config.JSON = map[string]any{"reasoning_effort": "high", "prompt_cache_key": "captured-session", "tools": sourceRawTools(t, "chat")}
				config.Remove = []string{"tool_choice", "max_completion_tokens", "budget_tokens"}
				temperature := 0.7
				call.Temperature = &temperature
			}
			model, err := newSDK(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			first := collect(t, model, call)
			checkStream(t, row.kind, first)
			replay(&call, row.kind)
			second := collect(t, model, call)
			var text string
			for _, part := range second {
				if part.Type == fantasy.StreamPartTypeTextDelta {
					text += part.Delta
				}
			}
			if text != "Read complete" {
				t.Errorf("second-step text = %q; source expects Read complete", text)
			}
			captured := server.captured()
			saveCaptures(t, row.name, captured)
			if len(captured) != 2 {
				t.Fatalf("ordinary steps emitted %d requests; want one per step", len(captured))
			}
			for _, sent := range captured {
				if sent.Path != row.path {
					t.Errorf("path = %q; source expects %q", sent.Path, row.path)
				}
				if strings.Contains(string(sent.Raw), "fixed-access") || strings.Contains(string(sent.Raw), "captured-key") || strings.Contains(string(sent.Raw), "compatible-key") {
					t.Error("authentication appeared in the JSON body")
				}
				if config.Kind != "google" {
					checkValue(t, sent.Body, "stream", true)
				}
			}
			checkRequest(t, config, captured)
		})
	}
	t.Run("cancellation", func(t *testing.T) { cancellationFixture(t, false) })
	t.Run("early_iterator_release", func(t *testing.T) { cancellationFixture(t, true) })
	t.Run("incomplete_tools_and_eof", incompleteFixture)
	t.Run("one_request_on_rejection", rejectionFixture)
	t.Run("usage_presence", usagePresenceFixture)
	t.Run("additional_source_contracts", additionalParityFixtures)
	t.Run("encrypted_reasoning_stream_round_trip", encryptedFixture)
	t.Run("claude_source_identity_and_hash", func(t *testing.T) {
		if !reflect.DeepEqual(claudeMetadata("install", "session", "account"), objectFixture(t, "claude-identity.json")) {
			t.Error("Claude nested metadata bytes differ from the source fixture")
		}
		digest := xxhash.NewWithSeed(claudeSeed)
		digest.Write([]byte("omp parity fixture"))
		if digest.Sum64() != 0xc707a0ca4d9f252e {
			t.Errorf("Claude hash differs from the independent source fixture: %x", digest.Sum64())
		}
	})
}

func checkStream(t *testing.T, kind string, parts []fantasy.StreamPart) {
	t.Helper()
	finish := false
	calls := 0
	signed, redacted := false, false
	for _, part := range parts {
		if metadata, ok := part.ProviderMetadata[anthropic.Name].(*anthropic.ReasoningOptionMetadata); ok {
			signed = signed || metadata.Signature == "signed-thinking"
			redacted = redacted || metadata.RedactedData == "redacted-data"
		}
		if metadata, ok := part.ProviderMetadata[google.Name].(*google.ReasoningMetadata); ok {
			signed = signed || metadata.Signature == "signed-thought" && metadata.ToolID == "stable-call"
		}
		if part.Type == fantasy.StreamPartTypeFinish {
			finish = true
			if part.Usage.CacheReadTokens != 8 {
				t.Errorf("cache-read usage = %d; source expects 8", part.Usage.CacheReadTokens)
			}
			if kind == "anthropic" && part.Usage.CacheCreationTokens != 4 {
				t.Errorf("cache-write usage = %d; source expects 4", part.Usage.CacheCreationTokens)
			}
		}
		if part.Type == fantasy.StreamPartTypeToolCall {
			calls++
			if part.ID != "stable-call" || part.ToolCallName != "read" || !json.Valid([]byte(part.ToolCallInput)) {
				t.Errorf("tool identity or arguments changed: %#v", part)
			}
		}
	}
	if !finish {
		t.Error("terminal completion was not surfaced")
	}
	if kind != "chat" && calls != 1 {
		t.Errorf("complete tool calls = %d; source expects 1", calls)
	}
	if kind == "anthropic" && (!signed || !redacted) {
		t.Error("streamed signed or redacted reasoning metadata was lost")
	}
	if kind == "google" && !signed {
		t.Error("streamed tool thought signature or its call association was lost")
	}
}

func checkRequest(t *testing.T, config transportConfig, captured []capturedRequest) {
	t.Helper()
	first, second := captured[0], captured[1]
	declarationPath, golden, systemPath := "tools.0.function", "chat-tool.json", "messages.0.content"
	switch config.Kind {
	case "responses":
		declarationPath, golden, systemPath = "tools.0", "responses-tool.json", "instructions"
		checkValue(t, first.Body, "store", false)
		checkValue(t, first.Body, "prompt_cache_key", config.JSON["prompt_cache_key"])
		checkValue(t, first.Body, "reasoning.effort", "xhigh")
		checkValue(t, first.Body, "reasoning.summary", "auto")
		checkValue(t, first.Body, "include", []any{"reasoning.encrypted_content"})
		checkValue(t, first.Body, "previous_response_id", nil)
		if tier := config.JSON["service_tier"]; tier != nil {
			checkValue(t, first.Body, "service_tier", tier)
		}
		if config.OAuth {
			for key, expected := range headersFixture(t, "codex-oauth-headers.json") {
				if first.Headers.Get(key) != expected {
					t.Errorf("captured OAuth header %s = %q; want %q", key, first.Headers.Get(key), expected)
				}
			}
			for _, key := range []string{"max_output_tokens", "previous_response_id", "stream_options", "temperature"} {
				checkValue(t, first.Body, key, nil)
			}
		}
		if !strings.Contains(string(second.Raw), `"encrypted_content":"encrypted-fixture"`) {
			t.Error("encrypted reasoning replay was not retained")
		}
	case "anthropic":
		declarationPath, golden, systemPath = "tools.0", "anthropic-tool.json", "system"
		if config.OAuth {
			for key, expected := range headersFixture(t, "claude-oauth-headers.json") {
				if first.Headers.Get(key) != expected {
					t.Errorf("captured OAuth header %s = %q; want %q", key, first.Headers.Get(key), expected)
				}
			}
			checkValue(t, first.Body, "metadata", objectFixture(t, "claude-identity.json"))
			checkValue(t, first.Body, "system.1.text", claudeIdentity)
			checkValue(t, first.Body, "system.2.text", systemText)
			checkValue(t, second.Body, "system.2.text", systemText)
			checkClaudeWire(t, first.Raw)
			checkClaudeWire(t, second.Raw)
			// Source billing checksums cover each final body. Only the captured
			// instruction string is stable across steps; billing bytes change.
			systemPath = "system.2.text"
		} else {
			if first.Headers.Get("x-api-key") != config.Key {
				t.Error("Anthropic API key header changed")
			}
			if first.Headers.Get("anthropic-version") != "2023-06-01" {
				t.Error("Anthropic version header changed")
			}
			checkValue(t, first.Body, "system.0.text", systemText)
			checkValue(t, first.Body, "system.0.cache_control", map[string]any{"type": "ephemeral"})
			for index := 0; index < 5; index++ {
				var expected any
				if index == 2 || index == 4 {
					expected = map[string]any{"type": "ephemeral"}
				}
				checkValue(t, first.Body, fmt.Sprintf("messages.%d.content.0.cache_control", index), expected)
			}
			checkValue(t, first.Body, "tools.0.eager_input_streaming", nil)
		}
		if !strings.Contains(string(second.Raw), `"signature":"signed-thinking"`) || !strings.Contains(string(second.Raw), `"data":"redacted-data"`) {
			t.Error("signed or redacted Anthropic reasoning replay changed")
		}
	case "google":
		declarationPath, golden, systemPath = "tools.functionDeclarations.0", "google-tool.json", "systemInstruction"
		checkValue(t, first.Body, "generationConfig.thinkingConfig.thinkingBudget", float64(0))
		if first.Headers.Get("x-goog-api-key") != config.Key || first.Headers.Get("authorization") != "" {
			t.Error("Gemini authentication headers changed")
		}
		if strings.Contains(first.Path, "key=") {
			t.Error("Gemini key was sent in the URL")
		}
		if !strings.Contains(string(second.Raw), `"thoughtSignature":"c2lnbmVkLXRob3VnaHQ="`) {
			t.Error("Gemini thought signature was not base64-preserved")
		}
	default:
		checkValue(t, first.Body, "messages.0.role", "system")
		checkValue(t, first.Body, "temperature", 0.7)
		checkValue(t, first.Body, "max_tokens", float64(512))
		checkValue(t, first.Body, "max_completion_tokens", nil)
		checkValue(t, first.Body, "reasoning_effort", "high")
		checkValue(t, first.Body, "prompt_cache_key", "captured-session")
	}
	if config.Kind != "anthropic" && config.Kind != "google" && first.Headers.Get("authorization") != "Bearer "+config.Key {
		t.Error("captured bearer header changed")
	}
	expected := objectFixture(t, golden)
	if config.OAuth && config.Kind == "anthropic" {
		expected["name"] = "_read"
		expected["eager_input_streaming"] = true
		expected["cache_control"] = map[string]any{"type": "ephemeral", "ttl": "1h"}
	}
	checkValue(t, first.Body, declarationPath, expected)
	before := rawAt(first.Raw, systemPath)
	after := rawAt(second.Raw, systemPath)
	if len(before) == 0 || len(after) == 0 {
		t.Error("captured system field is missing")
	}
	if !bytes.Equal(before, after) {
		t.Errorf("captured system bytes changed between steps: %s / %s", before, after)
	}
	if config.Kind == "responses" {
		checkValue(t, first.Body, "instructions", systemText)
	}
	if config.Kind == "google" {
		checkValue(t, first.Body, "systemInstruction.parts.0.text", systemText)
	}
}

func rawAt(raw []byte, path string) []byte {
	for _, key := range strings.Split(path, ".") {
		if len(raw) == 0 {
			return nil
		}
		if raw[0] == '[' {
			var values []json.RawMessage
			if json.Unmarshal(raw, &values) != nil {
				return nil
			}
			index, err := strconv.Atoi(key)
			if err != nil || index < 0 || index >= len(values) {
				return nil
			}
			raw = values[index]
		} else {
			var values map[string]json.RawMessage
			if json.Unmarshal(raw, &values) != nil {
				return nil
			}
			raw = values[key]
		}
	}
	return raw
}

func checkClaudeWire(t *testing.T, raw []byte) {
	t.Helper()
	if !bytes.HasPrefix(raw, []byte(`{"model":`)) {
		t.Error("Claude final-body source field order changed")
	}
	marker := []byte(`"system":[{"type":"text","text":"x-anthropic-billing-header: cc_version=2.1.280.`)
	start := bytes.Index(raw, marker)
	if start < 0 {
		t.Error("Claude final-body billing/system encoding changed")
		return
	}
	offset := bytes.Index(raw[start:], []byte("cch="))
	if offset < 0 || start+offset+9 > len(raw) {
		t.Error("Claude final-body checksum missing")
		return
	}
	offset += start + 4
	actual := string(raw[offset : offset+5])
	restored := bytes.Clone(raw)
	copy(restored[offset:offset+5], "00000")
	digest := xxhash.NewWithSeed(claudeSeed)
	digest.Write(restored)
	if expected := fmt.Sprintf("%05x", digest.Sum64()&0xfffff); actual != expected {
		t.Errorf("Claude final-body checksum %s; source expects %s", actual, expected)
	}
}

func cancellationFixture(t *testing.T, early bool) {
	t.Helper()
	closed := make(chan struct{})
	prefix := fixture(t, "responses-partial.sse")
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		io.Copy(io.Discard, request.Body)
		writer.Header().Set("Content-Type", "text/event-stream")
		writer.WriteHeader(http.StatusOK)
		writer.Write(prefix)
		writer.(http.Flusher).Flush()
		<-request.Context().Done()
		close(closed)
	}))
	t.Cleanup(server.Close)
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	model, err := newSDK(ctx, transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "captured-key"})
	if err != nil {
		t.Fatal(err)
	}
	stream, err := model.Stream(ctx, requestCall(t, "responses"))
	if err != nil {
		t.Fatal(err)
	}
	partial, failure := false, false
	for part := range stream {
		if part.Type == fantasy.StreamPartTypeFinish {
			t.Error("cancelled stream emitted a successful finish")
		}
		if part.Type == fantasy.StreamPartTypeError {
			failure = true
		}
		if part.Type == fantasy.StreamPartTypeTextDelta {
			if part.Delta != "Partial" {
				t.Errorf("partial output = %q", part.Delta)
			}
			partial = true
			if early {
				break
			}
			cancel()
		}
	}
	if !partial {
		t.Error("no partial output before terminal completion")
	}
	if !early && !failure {
		t.Error("explicit cancellation was not surfaced as failure")
	}
	<-closed
}

func incompleteFixture(t *testing.T) {
	for _, broken := range []bool{false, true} {
		t.Run(fmt.Sprintf("broken_transport_%v", broken), func(t *testing.T) {
			response := fixture(t, "anthropic-incomplete.sse")
			server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				io.Copy(io.Discard, request.Body)
				writer.Header().Set("Content-Type", "text/event-stream")
				if broken {
					writer.Header().Set("Content-Length", strconv.Itoa(len(response)+100))
				}
				writer.Write(response)
			}))
			defer server.Close()
			model, err := newSDK(t.Context(), transportConfig{Kind: "anthropic", Model: "claude-opus-4-8", Endpoint: server.URL, Key: "captured-key"})
			if err != nil {
				t.Fatal(err)
			}
			stream, err := model.Stream(t.Context(), requestCall(t, "anthropic"))
			if err != nil {
				t.Fatal(err)
			}
			failure, usage := false, false
			for part := range stream {
				if metadata, ok := part.ProviderMetadata[UsageMetadataKey].(*UsageMetadata); ok && metadata.CacheRead != nil && metadata.CacheWrite != nil {
					usage = usage || *metadata.CacheRead == 8 && *metadata.CacheWrite == 4
				}
				if part.Type == fantasy.StreamPartTypeError {
					failure = true
				}
				if part.Type == fantasy.StreamPartTypeToolCall {
					t.Errorf("incomplete tool became executable: %#v", part)
				}
				if part.Type == fantasy.StreamPartTypeFinish {
					usage = usage || part.Usage.CacheReadTokens == 8 && part.Usage.CacheCreationTokens == 4
					if broken || part.FinishReason == fantasy.FinishReasonStop || part.FinishReason == fantasy.FinishReasonToolCalls {
						t.Errorf("EOF became successful completion: %#v", part)
					}
				}
			}
			if broken && !failure {
				t.Error("broken HTTP stream did not surface a failure")
			}
			if !usage {
				t.Error("partial-attempt usage was not retained")
			}
		})
	}
}

func rejectionFixture(t *testing.T) {
	for _, kind := range []string{"responses", "anthropic", "google", "chat"} {
		t.Run(kind, func(t *testing.T) {
			var lock sync.Mutex
			requests := 0
			server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				lock.Lock()
				requests++
				lock.Unlock()
				writer.Header().Set("Content-Type", "application/json")
				writer.Header().Set("Retry-After", "7")
				writer.WriteHeader(http.StatusTooManyRequests)
				io.WriteString(writer, `{"error":{"message":"rate limit exceeded","code":"rate_limit_exceeded"}}`)
			}))
			defer server.Close()
			name := "fixture"
			if kind == "google" {
				name = "gemini-3-flash-preview"
			}
			if kind == "anthropic" {
				name = "claude-opus-4-8"
			}
			model, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: name, Endpoint: server.URL, Key: "captured-key"})
			if err != nil {
				t.Fatal(err)
			}
			stream, failure := model.Stream(t.Context(), requestCall(t, kind))
			if failure == nil {
				for part := range stream {
					if part.Type == fantasy.StreamPartTypeError {
						failure = part.Error
					}
					if part.Type == fantasy.StreamPartTypeFinish {
						t.Error("HTTP rejection became successful completion")
					}
				}
			}
			if failure == nil {
				t.Fatal("HTTP rejection was lost")
			}
			var provider *fantasy.ProviderError
			if !errors.As(failure, &provider) {
				t.Errorf("HTTP context unavailable: %T", failure)
			} else {
				if provider.StatusCode != 429 {
					t.Errorf("HTTP status = %d; source expects 429", provider.StatusCode)
				}
				if provider.ResponseHeaders["Retry-After"] != "7" && provider.ResponseHeaders["retry-after"] != "7" {
					t.Error("Retry-After was lost")
				}
			}
			lock.Lock()
			count := requests
			lock.Unlock()
			if count != 1 {
				t.Errorf("one rejection emitted %d HTTP requests", count)
			}
		})
	}
}

func usagePresenceFixture(t *testing.T) {
	// Probe the source contract's distinction between unreported and known-zero
	// counts. This is an added boundary probe, not a literal Rust test fixture.
	for _, kind := range []string{"responses", "anthropic", "google", "chat"} {
		t.Run(kind, func(t *testing.T) {
			base := string(fixture(t, kind+"-text.sse"))
			var responses []string
			for _, known := range []bool{false, true} {
				var output strings.Builder
				lines := strings.Split(base, "\n")
				for index, line := range lines {
					if strings.HasPrefix(line, "data: {") {
						var value map[string]any
						json.Unmarshal([]byte(strings.TrimPrefix(line, "data: ")), &value)
						replaceUsage(value, known)
						encoded, _ := json.Marshal(value)
						line = "data: " + string(encoded)
					}
					output.WriteString(line)
					if index < len(lines)-1 {
						output.WriteByte('\n')
					}
				}
				responses = append(responses, output.String())
			}
			server := serverFixture(t, responses...)
			name := "fixture"
			if kind == "google" {
				name = "gemini-3-flash-preview"
			}
			if kind == "anthropic" {
				name = "claude-opus-4-8"
			}
			model, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: name, Endpoint: server.URL, Key: "captured-key"})
			if err != nil {
				t.Fatal(err)
			}
			var reports [][]byte
			for attempt := range 2 {
				parts := collect(t, model, requestCall(t, kind))
				for _, part := range parts {
					if part.Type == fantasy.StreamPartTypeFinish {
						metadata, ok := part.ProviderMetadata[UsageMetadataKey].(*UsageMetadata)
						if !ok {
							t.Error("SDK did not retain optional usage metadata")
						} else {
							if metadata.Reported && !metadata.Precise {
								t.Error("SDK does not preserve individual raw usage field presence")
							}
							counts := []*int64{metadata.Input, metadata.Output, metadata.Total, metadata.CacheRead}
							if kind == "anthropic" {
								counts = append(counts, metadata.CacheWrite)
							} else {
								counts = append(counts, metadata.Reasoning)
							}
							for _, count := range counts {
								if attempt == 0 && count != nil {
									t.Error("SDK turned an unreported usage field into a known count")
								}
								if attempt == 1 && (count == nil || *count != 0) {
									t.Error("SDK lost a reported zero usage field")
								}
							}
						}
						report, _ := json.Marshal(struct {
							Usage    fantasy.Usage
							Metadata fantasy.ProviderMetadata
						}{part.Usage, part.ProviderMetadata})
						reports = append(reports, report)
					}
				}
			}
			if len(reports) != 2 {
				t.Errorf("usage finishes = %d; want two attempts", len(reports))
				return
			}
			if bytes.Equal(reports[0], reports[1]) {
				t.Error("Fantasy erased unreported-versus-known-zero usage presence")
			}
		})
	}
}

func replaceUsage(value map[string]any, known bool) {
	for key, child := range value {
		if key == "usage" || key == "usageMetadata" {
			if !known {
				delete(value, key)
				continue
			}
			fields, _ := child.(map[string]any)
			zeroCounters(fields)
			continue
		}
		if child, ok := child.(map[string]any); ok {
			replaceUsage(child, known)
		}
	}
}

func zeroCounters(value map[string]any) {
	for key, child := range value {
		if nested, ok := child.(map[string]any); ok {
			zeroCounters(nested)
		} else {
			value[key] = float64(0)
		}
	}
}

func encryptedFixture(t *testing.T) {
	server := serverFixture(t, string(fixture(t, "responses-encrypted.sse")), string(fixture(t, "responses-text.sse")))
	model, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL + "/v1", Key: "captured-key"})
	if err != nil {
		t.Fatal(err)
	}
	call := requestCall(t, "responses")
	first := collect(t, model, call)
	var metadata *openai.ResponsesReasoningMetadata
	for _, part := range first {
		if value, ok := part.ProviderMetadata[openai.Name].(*openai.ResponsesReasoningMetadata); ok {
			metadata = value
		}
	}
	if metadata == nil || metadata.EncryptedContent == nil || *metadata.EncryptedContent != "encrypted-reasoning" {
		t.Error("source encrypted reasoning event was not retained")
		return
	}
	if metadata.ItemID != "rs_fixture" || len(metadata.Summary) != 0 {
		t.Errorf("encrypted reasoning identity changed: %#v", metadata)
	}
	call.Prompt = append(call.Prompt,
		fantasy.Message{Role: fantasy.MessageRoleAssistant, Content: []fantasy.MessagePart{
			fantasy.ReasoningPart{ProviderOptions: fantasy.ProviderOptions{openai.Name: metadata}},
			fantasy.ToolCallPart{ToolCallID: "stable-call", ToolName: "read", Input: "{}"},
		}},
		fantasy.Message{Role: fantasy.MessageRoleTool, Content: []fantasy.MessagePart{fantasy.ToolResultPart{ToolCallID: "stable-call", Output: fantasy.ToolResultOutputContentText{Text: "File contents"}}}},
	)
	collect(t, model, call)
	captured := server.captured()
	saveCaptures(t, "encrypted_reasoning_stream_round_trip", captured)
	if len(captured) != 2 {
		t.Fatalf("encrypted reasoning steps emitted %d requests", len(captured))
	}
	if !bytes.Contains(captured[1].Raw, []byte(`"encrypted_content":"encrypted-reasoning"`)) {
		t.Error("received encrypted reasoning was not replayed")
	}
}
