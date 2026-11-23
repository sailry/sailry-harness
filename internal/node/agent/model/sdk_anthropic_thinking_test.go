package model

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAnthropicInitialThinkingSurvivesReplay(t *testing.T) {
	for _, row := range []struct {
		name, initial, delta, signature, signatureDelta string
	}{
		{"initial and deltas", "Initial reasoning. ", "More reasoning.", "initial-", "signature"},
		{"initial only", "Complete reasoning.", "", "complete-signature", ""},
		{"signature without text", "", "", "complete-signature", ""},
	} {
		t.Run(row.name, func(t *testing.T) {
			var stream strings.Builder
			event := func(kind string, value map[string]any) {
				value["type"] = kind
				raw, err := json.Marshal(value)
				if err != nil {
					t.Fatal(err)
				}
				stream.WriteString("event: " + kind + "\ndata: " + string(raw) + "\n\n")
			}
			event("message_start", map[string]any{"message": map[string]any{"id": "reply", "type": "message", "role": "assistant", "model": "claude-fixture", "content": []any{}, "usage": map[string]int{"input_tokens": 1, "output_tokens": 0}}})
			event("content_block_start", map[string]any{"index": 0, "content_block": map[string]any{"type": "thinking", "thinking": row.initial, "signature": row.signature}})
			if row.delta != "" {
				event("content_block_delta", map[string]any{"index": 0, "delta": map[string]any{"type": "thinking_delta", "thinking": row.delta}})
			}
			if row.signatureDelta != "" {
				event("content_block_delta", map[string]any{"index": 0, "delta": map[string]any{"type": "signature_delta", "signature": row.signatureDelta}})
			}
			event("content_block_stop", map[string]any{"index": 0})
			event("content_block_start", map[string]any{"index": 1, "content_block": map[string]any{"type": "text", "text": "Complete"}})
			event("content_block_stop", map[string]any{"index": 1})
			event("message_delta", map[string]any{"delta": map[string]any{"stop_reason": "end_turn"}, "usage": map[string]int{"output_tokens": 2}})
			event("message_stop", map[string]any{})
			server := serverFixture(t, stream.String())
			config := transportConfig{Kind: "anthropic", Model: "claude-fixture", Endpoint: server.URL, Key: "fixture-key"}
			model, err := newSDK(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			block := canonical.Block{Kind: "thinking", Data: &canonical.Thinking{}}
			var text string
			finished := false
			for _, part := range collect(t, model, requestCall(t, "anthropic")) {
				switch part.Type {
				case fantasy.StreamPartTypeReasoningDelta:
					block.Data.(*canonical.Thinking).Text += part.Delta
				case fantasy.StreamPartTypeReasoningEnd:
					if err := ApplyStreamMetadata(&block, part); err != nil {
						t.Fatal(err)
					}
				case fantasy.StreamPartTypeTextDelta:
					text += part.Delta
				case fantasy.StreamPartTypeFinish:
					finished = true
				}
			}
			if !finished || text != "Complete" {
				t.Fatalf("stream finished=%v, text=%q", finished, text)
			}
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Api = protocol.NewProviderID(), protocol.ConversationModelApiAnthropic
			provider.Authentication = protocol.AuthenticationApiKey
			selection := protocol.DefaultSessionConfig()
			selection.Provider, selection.Model = provider.ID, config.Model
			record := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.Assistant, block, canonical.Text(1, text)), time.Time{})
			source := Scope(Binding{Provider: provider, Config: selection})
			record.Metadata.Execution = &canonical.Execution{Source: &source}
			call, err := BuildCall(Binding{Provider: provider, Config: selection}, Request{Records: []canonical.Record{record}})
			if err != nil {
				t.Fatal(err)
			}
			body, err := requestJSON(config, call)
			if err != nil {
				t.Fatal(err)
			}
			checkValue(t, body, "messages.0.content.0.thinking", row.initial+row.delta)
			checkValue(t, body, "messages.0.content.0.signature", row.signature+row.signatureDelta)
			checkValue(t, body, "messages.0.content.1.text", "Complete")
		})
	}
}

func TestAnthropicAPIKeyBetaHeaders(t *testing.T) {
	manualWithTools := []byte(`{"thinking":{"type":"enabled","budget_tokens":4096},"tools":[{"name":"read"}]}`)
	adaptiveWithTools := []byte(`{"thinking":{"type":"adaptive"},"tools":[{"name":"read"}]}`)
	fastBody := []byte(`{"thinking":{"type":"enabled","budget_tokens":4096},"tools":[{"name":"read"}],"speed":"fast"}`)

	if got := anthropicBetaHeader("https://api.anthropic.com", false, manualWithTools, nil, nil); got != "interleaved-thinking-2025-05-14" {
		t.Fatalf("official manual thinking beta = %q", got)
	}
	if got := anthropicBetaHeader("https://api.anthropic.com", false, adaptiveWithTools, nil, nil); got != "" {
		t.Fatalf("official adaptive thinking beta = %q, want empty", got)
	}
	if got := anthropicBetaHeader("https://api.anthropic.com", false, fastBody,
		map[string]string{"Anthropic-Beta": "custom-1, interleaved-thinking-2025-05-14"},
		map[string]string{"anthropic-beta": "custom-2,custom-1"}); got != "custom-1,interleaved-thinking-2025-05-14,custom-2,fast-mode-2026-02-01" {
		t.Fatalf("merged beta = %q", got)
	}
	if got := anthropicBetaHeader("https://gateway.example.test", false, manualWithTools, nil, nil); got != "" {
		t.Fatalf("third-party endpoint received unexpected auto beta: %q", got)
	}
	if got := anthropicBetaHeader("https://gateway.example.test", false, manualWithTools,
		map[string]string{"anthropic-beta": "custom-1"},
		map[string]string{"Anthropic-Beta": "custom-2,custom-1"}); got != "custom-1,custom-2" {
		t.Fatalf("third-party merged custom beta = %q", got)
	}
}
