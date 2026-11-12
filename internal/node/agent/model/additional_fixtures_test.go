package model

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/google"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// additionalParityFixtures runs under TestRequestParity.
func additionalParityFixtures(t *testing.T) {
	for _, kind := range []string{"anthropic", "responses", "chat", "google"} {
		for _, early := range []bool{false, true} {
			t.Run(fmt.Sprintf("stream_owner_%s_early_%v", kind, early), func(t *testing.T) { b4StreamOwner(t, kind, early) })
		}
	}
	for _, kind := range []string{"chat", "responses"} {
		t.Run("malformed_stream_"+kind, func(t *testing.T) { b4MalformedStream(t, kind) })
	}
	for _, row := range []struct {
		name, kind, model, suffix string
		budget, goHeader          bool
	}{
		{"opencode_go", "chat", "glm-5", "/zen/go/v1", true, true},
		{"opencode_zen_chat", "chat", "deepseek-r1", "/zen/v1", true, false},
		{"opencode_zen_responses", "responses", "gpt-6-fixture", "/v1", true, false},
		{"direct_responses_budget", "responses", "fixture", "/v1", false, false},
		{"deepseek_budget", "chat", "deepseek-r1", "/v1", false, false},
	} {
		t.Run(row.name, func(t *testing.T) {
			server := serverFixture(t, string(fixture(t, row.kind+"-text.sse")))
			config := transportConfig{Kind: row.kind, Model: row.model, Endpoint: server.URL + row.suffix, Key: "key", JSON: map[string]any{}, Remove: []string{"tool_choice"}}
			if row.budget {
				config.JSON["budget_tokens"] = 4096
			} else {
				config.Remove = append(config.Remove, "budget_tokens")
			}
			if row.goHeader {
				config.Headers = map[string]string{"x-opencode-session": sessionID}
			}
			call := requestCall(t, row.kind)
			maximum := int64(8192)
			call.MaxOutputTokens = &maximum
			model, err := newSDK(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			collect(t, model, call)
			requests := server.captured()
			if len(requests) != 1 {
				t.Fatalf("ordinary step sent %d requests", len(requests))
			}
			saveCaptures(t, row.name, requests)
			last := "chat/completions"
			outputField := "max_tokens"
			if row.kind == "responses" {
				last, outputField = "responses", "max_output_tokens"
			}
			if requests[0].Path != row.suffix+"/"+last {
				t.Errorf("gateway path = %q; source expects %s/%s", requests[0].Path, row.suffix, last)
			}
			checkValue(t, requests[0].Body, outputField, float64(8192))
			if row.budget {
				checkValue(t, requests[0].Body, "budget_tokens", float64(4096))
			} else {
				checkValue(t, requests[0].Body, "budget_tokens", nil)
			}
			if row.goHeader && requests[0].Headers.Get("x-opencode-session") != sessionID {
				t.Error("OpenCode Go session header was lost")
			}
		})
	}
	for _, row := range []struct {
		name, suffix, version, model string
	}{
		{"azure_openai", "/openai/deployments/deployment", "2025-01-01-preview", "deployment"},
		{"foundry_maas", "/v1", "2024-05-01-preview", "fixture"},
		{"foundry_proxy", "/proxy/v1", "2024-05-01-preview", "fixture"},
	} {
		t.Run(row.name, func(t *testing.T) {
			server := serverFixture(t, string(fixture(t, "chat-text.sse")))
			model, err := newSDK(t.Context(), transportConfig{Kind: "chat", Model: row.model, Endpoint: server.URL + row.suffix, Key: "azure-key", Headers: map[string]string{"api-key": "azure-key"}, Query: map[string]string{"api-version": row.version}, RemoveHeaders: []string{"Authorization"}, Remove: []string{"budget_tokens"}})
			if err != nil {
				t.Fatal(err)
			}
			collect(t, model, requestCall(t, "chat"))
			requests := server.captured()
			if len(requests) != 1 {
				t.Fatalf("cloud step sent %d requests", len(requests))
			}
			saveCaptures(t, row.name, requests)
			if path := row.suffix + "/chat/completions?api-version=" + row.version; requests[0].Path != path {
				t.Errorf("cloud path = %q; source expects %q", requests[0].Path, path)
			}
			if _, present := requests[0].Headers["Authorization"]; present {
				t.Error("Azure request retained a bearer header")
			}
			if requests[0].Headers.Get("api-key") != "azure-key" {
				t.Error("Azure API key header was lost")
			}
			checkValue(t, requests[0].Body, "model", row.model)
			checkValue(t, requests[0].Body, "max_tokens", float64(512))
			checkValue(t, requests[0].Body, "budget_tokens", nil)
		})
	}
	for _, kind := range []string{"chat", "responses"} {
		t.Run("copilot_"+kind, func(t *testing.T) { b4Copilot(t, kind) })
		t.Run("repeated_call_ids_"+kind, func(t *testing.T) { b4RepeatedCalls(t, kind) })
	}
	t.Run("gemini_image_order_and_level", b4GeminiImages)
	t.Run("gemini_buffered_usage_fields", b4GeminiBufferedUsage)
}

func b4StreamOwner(t *testing.T, kind string, early bool) {
	closed, release := make(chan struct{}), make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		io.Copy(io.Discard, r.Body)
		w.Header().Set("Content-Type", "text/event-stream")
		prefix := b4Partial(kind)
		if kind == "google" {
			prefix = strings.Repeat(prefix, 128)
		}
		io.WriteString(w, prefix)
		w.(http.Flusher).Flush()
		select {
		case <-r.Context().Done():
			close(closed)
		case <-release:
		}
	}))
	defer server.Close()
	defer close(release)
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	name := "fixture"
	if kind == "anthropic" {
		name = "claude-opus-4-8"
	} else if kind == "google" {
		name = "gemini-3-flash-preview"
	}
	model, err := newSDK(ctx, transportConfig{Kind: kind, Model: name, Endpoint: server.URL, Key: "captured-key"})
	if err != nil {
		t.Fatal(err)
	}
	stream, err := model.Stream(ctx, requestCall(t, kind))
	if err != nil {
		t.Fatal(err)
	}
	partial, failed := false, false
	for part := range stream {
		if part.Type == fantasy.StreamPartTypeFinish {
			t.Error("released stream emitted a successful finish")
		}
		if part.Type == fantasy.StreamPartTypeError {
			failed = true
		}
		if part.Type == fantasy.StreamPartTypeTextDelta {
			partial = true
			if part.Delta != "Partial" {
				t.Errorf("partial output = %q; want Partial", part.Delta)
			}
			if early {
				break
			}
			cancel()
		}
	}
	if !partial {
		t.Error("text was not delivered before the server's terminal event")
	}
	if !early && !failed {
		t.Error("explicit cancellation did not surface a failure")
	}
	select {
	case <-closed:
	case <-t.Context().Done():
		t.Error("server request context remained open after iterator release")
	}
}

func b4Partial(kind string) string {
	switch kind {
	case "responses":
		return "data: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"pending\",\"object\":\"response\",\"created_at\":1,\"model\":\"fixture\",\"status\":\"in_progress\",\"output\":[]}}\n\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"item_id\":\"message\",\"output_index\":0,\"content_index\":0,\"delta\":\"Partial\",\"logprobs\":[]}\n\n"
	case "chat":
		return "data: {\"id\":\"chat\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Partial\"},\"finish_reason\":null}]}\n\n"
	case "anthropic":
		return "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"pending\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-opus-4-8\",\"content\":[],\"usage\":{\"input_tokens\":12,\"output_tokens\":0}}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Partial\"}}\n\n"
	default:
		return "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Partial\"}],\"role\":\"model\"},\"index\":0}],\"modelVersion\":\"gemini-3-flash-preview\"}\n\n"
	}
}

func b4MalformedStream(t *testing.T, kind string) {
	response := b4Partial(kind) + "data: invalid-json\n\n"
	if kind == "chat" {
		response = "data: {\"id\":\"chat\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"unfinished\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\"}}]},\"finish_reason\":null}]}\n\n"
	}
	server := serverFixture(t, response)
	model, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: "fixture", Endpoint: server.URL, Key: "captured-key"})
	if err != nil {
		t.Fatal(err)
	}
	stream, err := model.Stream(t.Context(), requestCall(t, kind))
	if err != nil {
		t.Fatal(err)
	}
	failed, partial := false, ""
	for part := range stream {
		switch part.Type {
		case fantasy.StreamPartTypeToolCall:
			t.Errorf("incomplete arguments became executable: %#v", part)
		case fantasy.StreamPartTypeFinish:
			t.Error("malformed or partial stream emitted a successful finish")
		case fantasy.StreamPartTypeError:
			failed = true
		case fantasy.StreamPartTypeTextDelta:
			partial += part.Delta
		}
	}
	if !failed {
		t.Error("malformed or partial stream ended without a failure")
	}
	if kind == "responses" && partial != "Partial" {
		t.Errorf("visible partial output = %q; want Partial", partial)
	}
	if count := len(server.captured()); count != 1 {
		t.Errorf("partial stream was resent %d times", count)
	}
}

func b4Copilot(t *testing.T, kind string) {
	server := serverFixture(t, string(fixture(t, kind+"-text.sse")), string(fixture(t, kind+"-text.sse")))
	headers := map[string]string{
		"authorization": "Bearer exchanged-token", "copilot-integration-id": "vscode-chat",
		"x-initiator": "agent", "editor-version": "vscode/1.99.0", "x-request-id": "captured-request",
	}
	model, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: "fixture", Endpoint: server.URL + "/captured", Key: "exchanged-token", Headers: headers, Remove: []string{"service_tier"}})
	if err != nil {
		t.Fatal(err)
	}
	call := requestCall(t, kind)
	call.Headers = map[string]string{"copilot-vision-request": "true", "x-request-id": "first-request"}
	collect(t, model, call)
	call.Headers = map[string]string{"x-request-id": "second-request"}
	collect(t, model, call)
	requests := server.captured()
	if len(requests) != 2 {
		t.Fatalf("Copilot steps sent %d requests", len(requests))
	}
	saveCaptures(t, "copilot_"+kind, requests)
	for index, request := range requests {
		for key, expected := range headers {
			if key == "x-request-id" {
				continue
			}
			if actual := request.Headers.Get(key); actual != expected {
				t.Errorf("Copilot header %s = %q; want %q", key, actual, expected)
			}
		}
		checkValue(t, request.Body, "service_tier", nil)
		if bytes.Contains(request.Raw, []byte("exchanged-token")) {
			t.Error("Copilot token leaked into request JSON")
		}
		expected := "first-request"
		if index == 1 {
			expected = "second-request"
		}
		if request.Headers.Get("x-request-id") != expected {
			t.Error("Copilot request ID was reused across steps")
		}
	}
	if requests[0].Headers.Get("copilot-vision-request") != "true" || requests[1].Headers.Get("copilot-vision-request") != "" {
		t.Error("Copilot vision header leaked across request boundaries")
	}
}

func b4RepeatedCalls(t *testing.T, kind string) {
	server := serverFixture(t, string(fixture(t, kind+"-text.sse")))
	call := requestCall(t, kind)
	for _, reason := range []string{"First denial", "Second denial", "Completed"} {
		call.Prompt = append(call.Prompt,
			fantasy.Message{Role: fantasy.MessageRoleAssistant, Content: []fantasy.MessagePart{fantasy.ToolCallPart{ToolCallID: "call-fixture-0", ToolName: "read", Input: "{}"}}},
			fantasy.Message{Role: fantasy.MessageRoleTool, Content: []fantasy.MessagePart{fantasy.ToolResultPart{ToolCallID: "call-fixture-0", Output: fantasy.ToolResultOutputContentText{Text: reason}}}},
		)
	}
	model, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: "fixture", Endpoint: server.URL, Key: "key"})
	if err != nil {
		t.Fatal(err)
	}
	collect(t, model, call)
	requests := server.captured()
	if len(requests) != 1 {
		t.Fatalf("history replay sent %d requests", len(requests))
	}
	saveCaptures(t, "repeated_calls_"+kind, requests)
	var calls, results []any
	if kind == "responses" {
		input, _ := requests[0].Body["input"].([]any)
		for _, item := range input {
			switch valueAt(item, "type") {
			case "function_call":
				calls = append(calls, item)
			case "function_call_output":
				results = append(results, item)
			}
		}
	} else {
		messages, _ := requests[0].Body["messages"].([]any)
		for _, message := range messages {
			if tools, ok := valueAt(message, "tool_calls").([]any); ok {
				calls = append(calls, tools...)
			}
			if valueAt(message, "role") == "tool" {
				results = append(results, message)
			}
		}
	}
	if len(calls) != 3 || len(results) != 3 {
		t.Fatalf("repeated calls/results = %d/%d; want 3/3", len(calls), len(results))
	}
	for index, reason := range []string{"First denial", "Second denial", "Completed"} {
		if kind == "responses" {
			checkValue(t, calls[index], "call_id", "call-fixture-0")
			checkValue(t, results[index], "call_id", "call-fixture-0")
			checkValue(t, results[index], "output", reason)
		} else {
			checkValue(t, calls[index], "id", "call-fixture-0")
			checkValue(t, results[index], "tool_call_id", "call-fixture-0")
			checkValue(t, results[index], "content", reason)
		}
	}
	if bytes.Contains(requests[0].Raw, []byte("__sailry")) {
		t.Error("private history marker leaked into request")
	}
}

func b4GeminiImages(t *testing.T) {
	server := serverFixture(t, string(fixture(t, "google-text.sse")))
	call := requestCall(t, "google")
	call.Prompt[1].Content = []fantasy.MessagePart{
		fantasy.TextPart{Text: "Before"}, fantasy.FilePart{Data: []byte("image"), MediaType: "image/png"},
		fantasy.TextPart{Text: "After"}, fantasy.FilePart{Data: []byte("second"), MediaType: "image/jpeg"},
	}
	level, thoughts := "high", true
	call.ProviderOptions = fantasy.ProviderOptions{google.Name: &google.ProviderOptions{ThinkingConfig: &google.ThinkingConfig{ThinkingLevel: &level, IncludeThoughts: &thoughts}}}
	model, err := newSDK(t.Context(), transportConfig{Kind: "google", Model: "gemini-3-flash-preview", Endpoint: server.URL + "/proxy", Key: "google-key"})
	if err != nil {
		t.Fatal(err)
	}
	collect(t, model, call)
	requests := server.captured()
	if len(requests) != 1 {
		t.Fatalf("Gemini image step sent %d requests", len(requests))
	}
	saveCaptures(t, "gemini_image_order", requests)
	for _, expected := range []struct {
		path  string
		value any
	}{
		{"contents.0.parts.0.text", "Before"},
		{"contents.0.parts.1.inlineData.mimeType", "image/png"},
		{"contents.0.parts.1.inlineData.data", "aW1hZ2U="},
		{"contents.0.parts.2.text", "After"},
		{"contents.0.parts.3.inlineData.mimeType", "image/jpeg"},
		{"contents.0.parts.3.inlineData.data", "c2Vjb25k"},
		{"generationConfig.thinkingConfig.thinkingLevel", "high"},
		{"generationConfig.thinkingConfig.includeThoughts", true},
	} {
		checkValue(t, requests[0].Body, expected.path, expected.value)
	}
	if strings.Contains(requests[0].Path, "key=") {
		t.Error("Gemini API key leaked into URL query")
	}
}

func b4GeminiBufferedUsage(t *testing.T) {
	// One response buffers multiple complete SDK frames. The final usage object
	// mixes reported zero and absent fields, which typed scalar values erase.
	prefix := strings.Repeat(b4Partial("google"), 128)
	terminal := "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Complete\"}]},\"finishReason\":\"STOP\",\"index\":0}],\"usageMetadata\":{\"promptTokenCount\":0,\"totalTokenCount\":0,\"cachedContentTokenCount\":0},\"modelVersion\":\"gemini-3-flash-preview\"}\n\n"
	server := serverFixture(t, prefix+terminal)
	model, err := newSDK(t.Context(), transportConfig{Kind: "google", Model: "gemini-3-flash-preview", Endpoint: server.URL, Key: "captured-key"})
	if err != nil {
		t.Fatal(err)
	}
	parts := collect(t, model, requestCall(t, "google"))
	var usage *UsageMetadata
	text := ""
	for _, part := range parts {
		if part.Type == fantasy.StreamPartTypeTextDelta {
			text += part.Delta
		}
		if part.Type == fantasy.StreamPartTypeFinish {
			usage, _ = part.ProviderMetadata[UsageMetadataKey].(*UsageMetadata)
		}
	}
	if text != strings.Repeat("Partial", 128)+"Complete" {
		t.Error("the passive observer changed buffered output")
	}
	if usage == nil || !usage.Precise {
		t.Fatal("buffered raw usage metadata was not retained")
	}
	for _, count := range []*int64{usage.Input, usage.Total, usage.CacheRead} {
		if count == nil || *count != 0 {
			t.Error("buffered reported zero was lost")
		}
	}
	if usage.Output != nil || usage.Reasoning != nil {
		t.Error("buffered missing counts became known values")
	}
	if len(server.captured()) != 1 {
		t.Error("the passive observer issued another model request")
	}
}

// TestCopilotRequestHeaders checks what a Copilot generation request declares
// about itself, and that no other account kind receives those headers.
func TestCopilotRequestHeaders(t *testing.T) {
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Api, provider.Authentication, provider.Enabled = protocol.NewProviderID(), protocol.ConversationModelApiChatCompletions, protocol.AuthenticationCopilot, true
	entry := protocol.DefaultConversationModel()
	entry.ID, entry.Vision = "fixture", true
	provider.Models = []protocol.ConversationModel{entry}
	binding := Binding{Provider: provider, Config: protocol.SessionConfig{Provider: provider.ID, Model: "fixture", Effort: protocol.Effort{Kind: "default"}}}
	turn, created := protocol.NewTurnID(), time.Unix(100, 0)
	message := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(role, blocks...), created)
	}
	asked := message(canonical.User, canonical.Text(0, "Read the file"))
	called := message(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "call", Name: "read", Arguments: json.RawMessage(`{}`)}})
	answered := message(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: "call", Name: "read", Status: canonical.ToolSuccess, Result: canonical.Payload{Value: json.RawMessage(`"done"`)}}})
	environment := message(canonical.User, canonical.Text(0, "Environment"))
	environment.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, TurnContext: true}
	pixels := []byte("\x89PNG\r\n\x1a\nfixture")
	pictured := message(canonical.User, canonical.Text(0, "Describe it"), canonical.Block{Index: 1, Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: "digest", Mime: "image/png", Size: uint64(len(pixels))}}})
	identifiers := map[string]bool{}
	for _, row := range []struct {
		name              string
		request           Request
		initiator, vision string
	}{
		{"a message the user wrote", Request{Records: []canonical.Record{asked}}, "user", ""},
		{"turn context after the user's message", Request{Records: []canonical.Record{asked, environment}}, "user", ""},
		{"a tool result", Request{Records: []canonical.Record{asked, called, answered}}, "agent", ""},
		{"a delegated session", Request{Records: []canonical.Record{asked}, Delegated: true}, "agent", ""},
		{"an image", Request{Records: []canonical.Record{pictured}, Assets: map[string][]byte{"digest": pixels}}, "user", "true"},
		{"headers of the caller", Request{Records: []canonical.Record{asked}, Headers: map[string]string{"x-fixture": "kept"}}, "user", ""},
	} {
		call, err := BuildCall(binding, row.request)
		if err != nil {
			t.Fatalf("%s: %v", row.name, err)
		}
		if call.Headers["openai-intent"] != "conversation-agent" || call.Headers["x-initiator"] != row.initiator || call.Headers["copilot-vision-request"] != row.vision {
			t.Errorf("%s: headers %v", row.name, call.Headers)
		}
		if identifier := call.Headers["x-request-id"]; identifier == "" || identifiers[identifier] {
			t.Errorf("%s: request identifier %q is missing or repeated", row.name, identifier)
		} else {
			identifiers[identifier] = true
		}
		if row.request.Headers != nil && (call.Headers["x-fixture"] != "kept" || len(row.request.Headers) != 1) {
			t.Errorf("%s: caller headers %v became %v", row.name, row.request.Headers, call.Headers)
		}
	}
	binding.Provider.Authentication = protocol.AuthenticationApiKey
	call, err := BuildCall(binding, Request{Records: []canonical.Record{asked}})
	if err != nil || len(call.Headers) != 0 {
		t.Fatalf("an API-key request carries Copilot headers: %v, %v", call.Headers, err)
	}
}
