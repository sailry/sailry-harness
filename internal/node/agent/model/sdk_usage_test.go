package model

import (
	"encoding/json"
	"strings"
	"testing"

	"charm.land/fantasy"
)

func TestAnthropicUsageBreakdownSurvivesStreaming(t *testing.T) {
	body := string(fixture(t, "anthropic-text.sse"))
	body = strings.Replace(body, `"cache_creation_input_tokens":4`, `"cache_creation_input_tokens":4,"cache_creation":{"ephemeral_1h_input_tokens":4,"ephemeral_5m_input_tokens":0},"service_tier":"standard"`, 1)
	body = strings.Replace(body, `"usage":{"output_tokens":3}`, `"usage":{"output_tokens":3,"cache_creation":{"ephemeral_5m_input_tokens":0}}`, 1)
	server := serverFixture(t, body)
	model, err := newSDK(t.Context(), transportConfig{Kind: "anthropic", Model: "claude-fixture", Endpoint: server.URL, Key: "fixture-key"})
	if err != nil {
		t.Fatal(err)
	}
	finished := false
	for _, part := range collect(t, model, requestCall(t, "anthropic")) {
		if part.Type != fantasy.StreamPartTypeFinish {
			continue
		}
		finished = true
		usage := PartUsage(part)
		if usage.Input == nil || *usage.Input != 24 || usage.Output == nil || *usage.Output != 3 || usage.Total == nil || *usage.Total != 27 || usage.CacheWrite == nil || *usage.CacheWrite != 4 {
			t.Fatalf("normalized usage = %+v", usage)
		}
		var raw struct {
			CacheCreation struct {
				Hour *uint64 `json:"ephemeral_1h_input_tokens"`
				Five *uint64 `json:"ephemeral_5m_input_tokens"`
			} `json:"cache_creation"`
			ServiceTier string `json:"service_tier"`
		}
		if err := json.Unmarshal(usage.Provider, &raw); err != nil {
			t.Fatal(err)
		}
		if raw.CacheCreation.Hour == nil || *raw.CacheCreation.Hour != 4 || raw.CacheCreation.Five == nil || *raw.CacheCreation.Five != 0 || raw.ServiceTier != "standard" {
			t.Fatalf("streaming lost provider usage details: %s", usage.Provider)
		}
	}
	if !finished {
		t.Fatal("stream did not finish")
	}
}

func TestChatCacheUsageFields(t *testing.T) {
	for _, row := range []struct {
		name, fields string
		cached       uint64
	}{
		{"deepseek hit", `"prompt_cache_hit_tokens":8,"prompt_cache_miss_tokens":4`, 8},
		{"deepseek miss", `"prompt_cache_hit_tokens":0,"prompt_cache_miss_tokens":12`, 0},
		{"standard detail takes precedence", `"prompt_tokens_details":{"cached_tokens":0},"prompt_cache_hit_tokens":8`, 0},
	} {
		t.Run(row.name, func(t *testing.T) {
			body := `data: {"id":"reply","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Complete"},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":3,"total_tokens":15,` + row.fields + `}}` + "\n\n" + "data: [DONE]\n\n"
			server := serverFixture(t, body)
			model, err := newSDK(t.Context(), transportConfig{Kind: "chat", Model: "deepseek-fixture", Endpoint: server.URL, Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			finished := false
			for _, part := range collect(t, model, requestCall(t, "chat")) {
				if part.Type != fantasy.StreamPartTypeFinish {
					continue
				}
				finished = true
				usage := PartUsage(part)
				if usage.CacheRead == nil || *usage.CacheRead != row.cached || usage.Input == nil || *usage.Input != 12 || usage.Total == nil || *usage.Total != 15 {
					t.Fatalf("cached input was lost or counted twice: %+v", usage)
				}
			}
			if !finished {
				t.Fatal("stream did not finish")
			}
		})
	}
}
