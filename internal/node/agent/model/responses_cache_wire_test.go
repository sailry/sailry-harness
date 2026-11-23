package model

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestResponsesWirePreservesContentBreakpoints(t *testing.T) {
	t.Parallel()
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
	checkValue(t, requests[0].Body, "input.1.content.1.prompt_cache_breakpoint.ttl", "12h")
	checkValue(t, requests[0].Body, "input.1.content.2.prompt_cache_breakpoint", nil)
	checkValue(t, requests[0].Body, "prompt_cache_options", options)
}

func TestResponsesFinalCacheMode(t *testing.T) {
	t.Parallel()
	const key = "prompt_cache_options"
	const explicit = `{"mode":"explicit","ttl":"30m"}`
	for _, tc := range []struct {
		name, override string
		remove, marked bool
	}{
		{name: "raw explicit", marked: true},
		{name: "overridden with implicit", override: `{"mode":"implicit"}`},
		{name: "overridden with TTL alone", override: `{"ttl":"12h"}`},
		{name: "removed", remove: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			config := transportConfig{Kind: "responses", Model: "gpt-6.2-astra", JSON: map[string]any{key: json.RawMessage(explicit)}}
			call := requestCall(t, "responses")
			if tc.override != "" {
				call.ProviderOptions[parametersKey] = &callParameters{Body: map[string]any{key: json.RawMessage(tc.override)}}
			}
			if tc.remove {
				config.Remove = []string{key}
			}
			body, err := requestJSON(config, call)
			if err != nil {
				t.Fatal(err)
			}
			checkValue(t, body, "instructions", systemText)
			checkValue(t, body, "input.0.role", "user")
			var expected any
			if tc.marked {
				expected = map[string]any{"mode": "explicit"}
			}
			checkValue(t, body, "input.0.content.0.prompt_cache_breakpoint", expected)
		})
	}
}

func cachePresetBinding(t *testing.T, endpoint string, body map[string]json.RawMessage) Binding {
	t.Helper()
	const modelID = "gpt-6.2-astra"
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled = protocol.NewProviderID(), true
	provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey, endpoint+"/v1"
	listed := protocol.DefaultConversationModel()
	listed.ID, listed.Tools, listed.Vision = modelID, true, true
	provider.Models = []protocol.ConversationModel{listed}
	provider.Preset = &protocol.ConversationPresetsPresetRef{Source: protocol.ConversationPresetsSource{Kind: "partner"}, ID: "fixture"}
	preset := protocol.DefaultConversationPresetsBinding()
	if len(body) > 0 {
		policy := protocol.DefaultConversationPresetsRequestPolicy()
		policy.Models, policy.Body = []string{modelID}, body
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
	return binding
}

func TestResponsesPresetCacheBreakpoints(t *testing.T) {
	t.Parallel()
	reserved := protocol.DefaultConversationPresetsRequestPolicy()
	reserved.Models = []string{"gpt-6.2-astra"}
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
			var body map[string]json.RawMessage
			if tc.template {
				body = map[string]json.RawMessage{
					"prompt_cache_options": json.RawMessage(`{"mode":"explicit","ttl":"12h"}`),
					"prompt":               json.RawMessage(`{"id":"pmpt_fixture","variables":{"prefix":{"type":"input_text","text":"stable","prompt_cache_breakpoint":{"mode":"explicit","ttl":"30m"}}}}`),
				}
			}
			binding := cachePresetBinding(t, server.URL, body)
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

func TestResponsesGeneratedCacheBreakpoints(t *testing.T) {
	t.Parallel()
	for _, mode := range []string{"explicit", "implicit"} {
		t.Run(mode, func(t *testing.T) {
			sse := string(fixture(t, "responses-text.sse"))
			server := serverFixture(t, sse, sse, sse)
			binding := cachePresetBinding(t, server.URL, map[string]json.RawMessage{
				"prompt_cache_options": json.RawMessage(`{"mode":"` + mode + `","ttl":"12h"}`),
			})
			m, err := New(t.Context(), binding)
			if err != nil {
				t.Fatal(err)
			}
			req := Request{
				System: strings.Repeat("Stable instructions. ", 600),
				Tools:  []Tool{{Name: "read", InputSchema: map[string]any{"type": "object"}}},
				Assets: map[string][]byte{"image": []byte("pixels")},
			}
			turn := protocol.NewTurnID()
			message := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
				return canonical.NewRecord(turn, "message", canonical.NewMessage(role, blocks...), time.Time{})
			}
			var previous capturedRequest
			send := func(records ...canonical.Record) capturedRequest {
				t.Helper()
				req.Records = append(req.Records, records...)
				call, err := BuildCall(binding, req)
				if err != nil {
					t.Fatal(err)
				}
				before := len(server.captured())
				collect(t, m, call)
				requests := server.captured()
				if len(requests) != before+1 || requests[before].Path != "/v1/responses" {
					t.Fatalf("expected one new Responses request, got %d", len(requests)-before)
				}
				wire := requests[before]
				if previous.Raw != nil {
					checkResponsesPrefix(t, mode, previous, wire, false, false, len(responsesInput(t, previous.Raw)))
				}
				previous = wire
				return wire
			}
			first := send(message(canonical.User, canonical.Text(0, "Read both files")))
			checkValue(t, first.Body, "model", "gpt-6.2-astra")
			checkValue(t, first.Body, "instructions", nil)
			checkValue(t, first.Body, "input.0.role", "developer")
			checkValue(t, first.Body, "input.0.content.0.text", req.System)
			checkValue(t, first.Body, "prompt_cache_options", map[string]any{"mode": mode, "ttl": "12h"})
			checkValue(t, first.Body, "tools.0.prompt_cache_breakpoint", nil)

			toolCall := func(index uint64, id string) canonical.Block {
				return canonical.Block{Index: index, Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: "read", Arguments: json.RawMessage(`{}`)}}
			}
			const result = `{"ok":true}`
			toolResult := func(id string, attachments ...canonical.Attachment) canonical.Record {
				return message(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{
					ID: id, Name: "read", Status: canonical.ToolSuccess, Format: canonical.JSON,
					Result: canonical.Payload{Value: json.RawMessage(result)}, Attachments: attachments,
				}})
			}
			second := send(message(canonical.Assistant, toolCall(0, "a"), toolCall(1, "b")), toolResult("a"),
				toolResult("b", canonical.Attachment{Asset: canonical.AssetRef{Digest: "image", Mime: "image/png", Size: 6}}))
			checkValue(t, second.Body, "input.4.call_id", "a")
			checkValue(t, second.Body, "input.5.call_id", "b")
			checkValue(t, second.Body, "input.5.output.0.text", result)
			checkValue(t, second.Body, "input.5.output.1.image_url", "data:image/png;base64,cGl4ZWxz")
			third := send(message(canonical.Assistant, canonical.Text(0, "Done")), message(canonical.User, canonical.Text(0, "Continue")))
			if mode == "implicit" {
				checkValue(t, third.Body, "input.4.output", result)
				if bytes.Contains(third.Raw, []byte(`"prompt_cache_breakpoint"`)) {
					t.Fatal("implicit caching added an explicit breakpoint")
				}
				return
			}
			checkValue(t, third.Body, "input.4.output.0.type", "input_text")
			checkValue(t, third.Body, "input.4.output.0.text", result)
			markers := []string{"input.0.content.0", "input.1.content.0", "input.4.output.0", "input.5.output.1", "input.7.content.0"}
			for _, path := range markers {
				checkValue(t, third.Body, path+".prompt_cache_breakpoint", map[string]any{"mode": "explicit"})
			}
			if got := bytes.Count(third.Raw, []byte(`"prompt_cache_breakpoint"`)); got != len(markers) {
				t.Fatalf("got %d breakpoints, want %d", got, len(markers))
			}
		})
	}
}
