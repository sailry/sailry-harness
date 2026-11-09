package model

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"charm.land/fantasy/providers/google"
)

const rawRustRevision = "eaad1b7f18e448c8c4e9e9286e19560c8a351ae1"
const rawRustProvider = "33333333-3333-4333-8333-333333333333"
const rawRustRequest = "22222222-2222-4222-8222-222222222222"

type rawRustCapture struct {
	Case           string `json:"case"`
	API            string `json:"api"`
	Authentication string `json:"authentication"`
	Request        int    `json:"request"`
	Body           string `json:"body"`
	Metadata       string `json:"metadata"`
	Bytes          int    `json:"bytes"`
	SHA256         string `json:"sha256"`
}

type rawRustManifest struct {
	SourceRevision string           `json:"source_revision"`
	Session        string           `json:"session"`
	Request        string           `json:"request"`
	Provider       string           `json:"provider"`
	BodyOrigin     string           `json:"body_origin"`
	Captures       []rawRustCapture `json:"captures"`
}

type rawRustMetadata struct {
	Path    string            `json:"path"`
	Headers map[string]string `json:"headers"`
}

type rawRustCase struct {
	name, kind, model, suffix string
	authentication            string
	call                      fantasy.Call
	extra                     map[string]any
	order                     []string
	signedReplay              bool
}

// rawRustFixtures compares the independently captured Rust socket bytes.
// Missing goldens are failures, never skips.
// The SDK fallback has its own result group; the separate Fantasy spike's
// results do not establish this implementation's raw-byte acceptance.
func rawRustFixtures(t *testing.T) {
	t.Helper()
	t.Run("raw_rust_sdk", func(t *testing.T) {
		var manifest rawRustManifest
		readRawRustJSON(t, "manifest.json", &manifest)
		if manifest.SourceRevision != rawRustRevision || manifest.Session != sessionID || manifest.Request != rawRustRequest || manifest.Provider != rawRustProvider || manifest.BodyOrigin != "HTTP socket bytes before JSON parsing" {
			t.Fatalf("unexpected independent Rust capture provenance: %+v", manifest)
		}
		if len(manifest.Captures) != 23 {
			t.Fatalf("independent Rust capture count = %d; want 23", len(manifest.Captures))
		}
		captures := map[string]rawRustCapture{}
		for _, capture := range manifest.Captures {
			key := fmt.Sprintf("%s/%d", capture.Case, capture.Request)
			if _, duplicate := captures[key]; duplicate {
				t.Fatalf("duplicate independent Rust request %s", key)
			}
			captures[key] = capture
		}
		for _, row := range rawRustCases(t) {
			t.Run(row.name, func(t *testing.T) {
				responses := []string{string(fixture(t, row.kind+"-text.sse"))}
				if row.signedReplay {
					responses = append([]string{string(fixture(t, "google-tool.sse"))}, responses...)
				}
				server := serverFixture(t, responses...)
				config := rawRustConfig(row, server.URL)
				model, err := newSDK(t.Context(), config)
				if err != nil {
					t.Fatal(err)
				}
				parts := collect(t, model, row.call)
				if row.signedReplay {
					rawRustReplay(t, &row.call, parts)
					collect(t, model, row.call)
				}
				captured := server.captured()
				if len(captured) != len(responses) {
					t.Fatalf("HTTP requests = %d; source fixture sends %d", len(captured), len(responses))
				}
				for index, actual := range captured {
					key := fmt.Sprintf("%s/%d", row.name, index+1)
					golden, exists := captures[key]
					if !exists {
						t.Fatalf("missing independent Rust request %s", key)
					}
					if golden.Authentication != row.authentication {
						t.Fatalf("source authentication = %q; input fixture uses %q", golden.Authentication, row.authentication)
					}
					checkRawRustCapture(t, golden, actual)
				}
			})
		}
		productionRawRustFixtures(t, captures)
	})
}

// Inputs translate the independent Rust source fixture literally. They do not
// inspect a captured request or rewrite model output to match a golden body.
func rawRustCases(t *testing.T) []rawRustCase {
	t.Helper()
	base := func(name, kind, model, suffix string) rawRustCase {
		maximum := int64(512)
		row := rawRustCase{
			name: name, kind: kind, model: model, suffix: suffix,
			authentication: "api_key", extra: map[string]any{},
			call: fantasy.Call{
				Prompt: fantasy.Prompt{
					{Role: fantasy.MessageRoleSystem, Content: []fantasy.MessagePart{fantasy.TextPart{Text: systemText}}},
					{Role: fantasy.MessageRoleUser, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "Read the file"}}},
				},
				MaxOutputTokens: &maximum,
				Tools: []fantasy.Tool{fantasy.FunctionTool{Name: "read", Description: "Read a file", InputSchema: map[string]any{
					"type": "object", "properties": map[string]any{"path": map[string]any{"type": "string"}}, "required": []any{"path"},
				}}},
			},
		}
		SetToolSchemas(&row.call, map[string]json.RawMessage{"read": json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}`)})
		if kind == "responses" {
			row.extra["prompt_cache_key"] = sessionID
			row.order = []string{"store", "prompt_cache_key"}
		}
		if kind == "anthropic" {
			cache := anthropic.NewProviderCacheControlOptions(&anthropic.ProviderCacheControlOptions{CacheControl: anthropic.CacheControl{Type: "ephemeral"}})
			row.call.Prompt[0].ProviderOptions = cache
			row.call.Prompt[1].ProviderOptions = cache
			if model == "claude-opus-4-8" {
				row.extra["thinking"] = map[string]any{"type": "disabled"}
			}
		}
		return row
	}
	fullSchema := func(row *rawRustCase) {
		tool := row.call.Tools[0].(fantasy.FunctionTool)
		tool.InputSchema = objectFixture(t, "schema.json")
		row.call.Tools[0] = tool
		SetToolSchemas(&row.call, map[string]json.RawMessage{"read": fixture(t, "schema.json")})
	}
	image := func(row *rawRustCase) {
		row.call.Prompt[1].Content = append(row.call.Prompt[1].Content, fantasy.FilePart{MediaType: "image/png", Data: []byte("image")})
	}
	rows := []rawRustCase{}
	for _, input := range []struct{ name, kind, model string }{
		{"schema-responses", "responses", "fixture"},
		{"schema-chat", "chat", "fixture"},
		{"schema-anthropic", "anthropic", "claude-opus-4-8"},
		{"schema-gemini", "google", "gemini-3-flash-preview"},
	} {
		row := base(input.name, input.kind, input.model, "")
		fullSchema(&row)
		rows = append(rows, row)
	}
	cache := base("anthropic-cache", "anthropic", "claude-opus-4-8", "")
	cache.call.Prompt = cache.call.Prompt[:1]
	for index := 0; index < 5; index++ {
		role := fantasy.MessageRoleUser
		if index%2 == 1 {
			role = fantasy.MessageRoleAssistant
		}
		message := fantasy.Message{Role: role, Content: []fantasy.MessagePart{fantasy.TextPart{Text: fmt.Sprintf("Message %d", index)}}}
		if index == 2 || index == 4 {
			message.ProviderOptions = cache.call.Prompt[0].ProviderOptions
		}
		cache.call.Prompt = append(cache.call.Prompt, message)
	}
	rows = append(rows, cache)
	adaptive := base("anthropic-adaptive", "anthropic", "claude-opus-4-8", "/proxy/v1/messages")
	maximum := int64(16000)
	adaptive.call.MaxOutputTokens = &maximum
	adaptive.extra["thinking"] = map[string]any{"type": "adaptive", "display": "summarized"}
	adaptive.extra["output_config"] = map[string]any{"effort": "xhigh"}
	adaptive.order = []string{"thinking", "output_config"}
	rows = append(rows, adaptive)
	manual := base("anthropic-manual", "anthropic", "fixture", "")
	manualMaximum := int64(8192)
	manual.call.MaxOutputTokens = &manualMaximum
	manual.extra["thinking"] = map[string]any{"type": "enabled", "budget_tokens": 4096}
	rows = append(rows, manual)
	rows = append(rows, base("anthropic-disabled", "anthropic", "claude-opus-4-8", ""))
	claude := base("anthropic-oauth", "anthropic", "claude-opus-4-8", "")
	claude.authentication = "claude_oauth"
	fullSchema(&claude)
	claude.extra["metadata"] = claudeMetadata(rawRustProvider, sessionID, "")
	claude.extra["sailry_claude_first_user"] = "Read the file"
	claude.extra["sailry_claude_version"] = "2.1.280"
	rows = append(rows, claude)
	for _, input := range []struct{ name, tier string }{{"responses-priority", "priority"}, {"responses-ultrafast", "ultrafast"}} {
		row := base(input.name, "responses", "fixture", "/gateway/v1")
		row.extra["service_tier"] = input.tier
		row.extra["prompt_cache_key"] = "captured-session"
		row.extra["reasoning"] = map[string]any{"effort": "xhigh", "summary": "auto"}
		row.order = []string{"service_tier", "prompt_cache_key", "reasoning", "store"}
		image(&row)
		rows = append(rows, row)
	}
	foreign := base("responses-foreign-reasoner", "responses", "custom/reasoner", "")
	temperature := 0.7
	foreign.call.Temperature = &temperature
	foreign.extra["include"] = []string{"reasoning.encrypted_content"}
	foreign.order = []string{"store", "prompt_cache_key", "include"}
	rows = append(rows, foreign)
	codex := base("codex-oauth", "responses", "fixture", "/backend-api/codex")
	codex.authentication = "chat_gpt_oauth"
	codex.call.MaxOutputTokens = nil
	codex.extra["include"] = []string{"reasoning.encrypted_content"}
	codex.extra["instructions"] = systemText
	codex.call.Prompt = codex.call.Prompt[1:]
	codex.order = []string{"store", "prompt_cache_key", "include", "instructions"}
	rows = append(rows, codex)
	for _, input := range []struct{ name, kind string }{{"copilot-chat", "chat"}, {"copilot-responses", "responses"}} {
		row := base(input.name, input.kind, "fixture", "/captured")
		row.authentication = "copilot"
		rows = append(rows, row)
	}
	for _, input := range []struct{ name, suffix string }{{"compatible-base", "/gateway/v1"}, {"compatible-full", "/gateway/v1/chat/completions"}} {
		row := base(input.name, "chat", "uncatalogued", input.suffix)
		row.order = []string{"reasoning_effort", "prompt_cache_key"}
		row.extra["reasoning_effort"] = "high"
		row.extra["prompt_cache_key"] = "captured-session"
		rows = append(rows, row)
	}
	deepseek := base("deepseek-reasoning", "chat", "deepseek/deepseek-r1", "/v1")
	deepseek.call.Temperature = &temperature
	rows = append(rows, deepseek)
	gemini := base("gemini-disabled", "google", "gemini-2.5-flash", "/proxy/v1beta")
	fullSchema(&gemini)
	image(&gemini)
	gemini.call.ProviderOptions[parametersKey] = &callParameters{Body: map[string]any{
		"generationConfig": map[string]any{"thinkingConfig": map[string]any{"thinkingBudget": 0, "includeThoughts": false}},
	}}
	rows = append(rows, gemini)
	signed := base("gemini-signed-replay", "google", "gemini-3-flash-preview", "/v1beta")
	signed.signedReplay = true
	rows = append(rows, signed)
	return rows
}

func rawRustConfig(row rawRustCase, server string) transportConfig {
	config := transportConfig{
		Kind: row.kind, Model: row.model, Endpoint: server + row.suffix, Key: "fixed-access",
		Headers: map[string]string{"user-agent": "Sailry/0.1.0"},
		JSON:    row.extra, Order: row.order, Remove: []string{"tool_choice"},
	}
	if row.kind == "google" {
		config.Endpoint = strings.TrimSuffix(config.Endpoint, "/v1beta")
	}
	switch row.authentication {
	case "api_key":
		switch row.kind {
		case "anthropic":
			config.Headers["x-api-key"] = "fixed-access"
			config.Headers["anthropic-version"] = "2023-06-01"
		case "google":
			config.Headers["x-goog-api-key"] = "fixed-access"
		default:
			config.Headers["authorization"] = "Bearer fixed-access"
		}
	case "claude_oauth":
		config.Key, config.OAuth = "", true
		config.Headers["authorization"] = "Bearer fixed-access"
		config.Headers["x-claude-code-session-id"] = sessionID
	case "chat_gpt_oauth":
		config.Key = ""
		for key, value := range map[string]string{
			"authorization": "Bearer fixed-access", "user-agent": "CapturedAgent/1", "originator": "codex_cli_rs",
			"version": "0.161.0", "chatgpt-account-id": "fixed-account", "session-id": sessionID,
			"x-codex-routing-hint": "model=fixture",
		} {
			config.Headers[key] = value
		}
		config.Remove = append(config.Remove, "max_tokens", "max_output_tokens", "max_tool_calls", "temperature", "top_p", "previous_response_id", "prompt_cache_retention", "stream_options", "user")
	case "copilot":
		config.Key = ""
		for key, value := range map[string]string{
			"authorization": "Bearer fixed-access", "user-agent": "CapturedAgent/1", "editor-version": "CapturedEditor/1",
			"editor-plugin-version": "CapturedPlugin/1", "copilot-integration-id": "vscode-chat", "x-github-api-version": "2025-04-01",
			"openai-intent": "conversation-agent", "x-initiator": "user", "x-request-id": rawRustRequest, "copilot-vision-request": "true",
		} {
			config.Headers[key] = value
		}
	}
	return config
}

func rawRustReplay(t *testing.T, call *fantasy.Call, parts []fantasy.StreamPart) {
	t.Helper()
	for _, part := range parts {
		if part.Type != fantasy.StreamPartTypeToolCall {
			continue
		}
		metadata, ok := part.ProviderMetadata[google.Name].(*google.ReasoningMetadata)
		if !ok || metadata == nil || metadata.ToolID != "stable-call" || metadata.Signature != "signed-thought" {
			t.Fatalf("source signed Gemini tool carrier changed: %#v", part)
		}
		call.Prompt = append(call.Prompt,
			fantasy.Message{Role: fantasy.MessageRoleAssistant, Content: []fantasy.MessagePart{fantasy.ToolCallPart{
				ToolCallID: part.ID, ToolName: part.ToolCallName, Input: part.ToolCallInput,
				ProviderOptions: fantasy.ProviderOptions{google.Name: metadata},
			}}},
			fantasy.Message{Role: fantasy.MessageRoleTool, Content: []fantasy.MessagePart{fantasy.ToolResultPart{
				ToolCallID: part.ID, Output: fantasy.ToolResultOutputContentText{Text: "File contents"},
			}}},
		)
		return
	}
	t.Fatal("source signed Gemini fixture emitted no complete tool call")
}

func checkRawRustCapture(t *testing.T, golden rawRustCapture, actual capturedRequest) {
	t.Helper()
	stem := fmt.Sprintf("%s/request-%d", golden.Case, golden.Request)
	if golden.Body != stem+".body" || golden.Metadata != stem+".metadata.json" {
		t.Fatalf("independent Rust capture filenames do not match %s", stem)
	}
	expected := fixture(t, filepath.Join("raw-rust", golden.Body))
	hash := fmt.Sprintf("%x", sha256.Sum256(expected))
	if len(expected) != golden.Bytes || hash != golden.SHA256 {
		t.Fatalf("independent Rust bytes fail manifest integrity: %s", golden.Body)
	}
	if !bytes.Equal(actual.Raw, expected) {
		offset := 0
		for offset < len(actual.Raw) && offset < len(expected) && actual.Raw[offset] == expected[offset] {
			offset++
		}
		t.Errorf("raw request %s differs at byte %d: Go bytes=%d SHA256=%x; Rust bytes=%d SHA256=%s\nGo raw: %s\nRust raw: %s", stem, offset, len(actual.Raw), sha256.Sum256(actual.Raw), len(expected), hash, actual.Raw, expected)
	}
	var metadata rawRustMetadata
	readRawRustJSON(t, golden.Metadata, &metadata)
	if actual.Path != metadata.Path {
		t.Errorf("request %s path = %q; Rust received %q", stem, actual.Path, metadata.Path)
	}
	names := []string{
		"authorization", "x-api-key", "x-goog-api-key", "api-key", "user-agent", "anthropic-version", "anthropic-beta",
		"anthropic-dangerous-direct-browser-access", "x-app", "x-claude-code-session-id", "session-id", "chatgpt-account-id",
		"originator", "version", "x-codex-routing-hint", "editor-version", "editor-plugin-version", "copilot-integration-id",
		"x-github-api-version", "openai-intent", "x-initiator", "x-request-id", "copilot-vision-request",
	}
	if golden.Authentication == "claude_oauth" {
		names = append(names, "accept", "content-type", "connection", "accept-encoding", "x-stainless-lang", "x-stainless-package-version", "x-stainless-retry-count", "x-stainless-runtime", "x-stainless-runtime-version", "x-stainless-timeout")
	}
	for _, name := range names {
		if received, want := actual.Headers.Get(name), metadata.Headers[name]; received != want {
			t.Errorf("request %s header %s = %q; Rust received %q", stem, name, received, want)
		}
	}
}

func readRawRustJSON(t *testing.T, name string, target any) {
	t.Helper()
	body, err := os.ReadFile(filepath.Join("testdata", "raw-rust", name))
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(body, target); err != nil {
		t.Fatal(err)
	}
}
