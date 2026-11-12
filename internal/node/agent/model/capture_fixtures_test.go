package model

import (
	"charm.land/fantasy"
	"charm.land/fantasy/providers/google"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func capturedPolicyFixtures(t *testing.T) {
	for _, row := range []struct {
		name                      string
		api                       protocol.ConversationModelApi
		kind, model, effort, path string
	}{{"anthropic", protocol.ConversationModelApiAnthropic, "anthropic", "claude-opus-4-8", "high", "/v1/messages"}, {"responses", protocol.ConversationModelApiResponses, "responses", "foreign/reasoner", "high", "/responses"}, {"compatible", protocol.ConversationModelApiChatCompletions, "chat", "foreign/reasoner", "high", "/chat/completions"}, {"gemini", protocol.ConversationModelApiGemini, "google", "gemini-2.5-flash", "none", "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"}} {
		t.Run("captured_"+row.name, func(t *testing.T) {
			server := serverFixture(t, string(fixture(t, row.kind+"-text.sse")), string(fixture(t, row.kind+"-text.sse")))
			provider := protocol.DefaultConversationProvider()
			provider.ID = protocol.NewProviderID()
			provider.Api = row.api
			provider.Authentication = protocol.AuthenticationApiKey
			provider.Enabled = true
			provider.Name = "Fixture"
			provider.Endpoint = server.URL
			provider.DefaultModel = row.model
			metadata := protocol.DefaultConversationModel()
			metadata.ID = row.model
			metadata.Context = 200000
			metadata.Output = 8192
			metadata.Reasoning = true
			provider.Models = []protocol.ConversationModel{metadata}
			config := protocol.DefaultSessionConfig()
			config.Provider = provider.ID
			config.Model = row.model
			config.Mode = protocol.WorkModeCode
			config.Permission = protocol.PermissionAsk
			config.Effort = protocol.Effort{Kind: row.effort}
			secret := protocol.NewSecret("captured-key")
			binding, err := Capture(provider, config, &secret, protocol.NewSessionID())
			if err != nil {
				t.Fatal(err)
			}
			model, err := New(t.Context(), binding)
			if err != nil {
				t.Fatal(err)
			}
			record := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Read the file")), policyClock{}.now)
			request := Request{System: systemText, Records: []canonical.Record{record}, Tools: []Tool{{Name: "read", Description: "Read a file", InputSchema: objectFixture(t, "schema.json")}}}
			for attempt := 0; attempt < 2; attempt++ {
				call, err := BuildCall(binding, request)
				if err != nil {
					t.Fatal(err)
				}
				collect(t, model, call)
				if attempt == 0 {
					request.Records = append(request.Records, canonical.NewRecord(record.Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Continue")), record.Created))
				}
			}
			captured := server.captured()
			if len(captured) != 2 {
				t.Fatalf("ordinary steps = %d HTTP requests", len(captured))
			}
			for _, http := range captured {
				if http.Path != row.path {
					t.Errorf("route=%q; want %q", http.Path, row.path)
				}
			}
			first, second := captured[0].Body, captured[1].Body
			switch row.kind {
			case "anthropic":
				checkValue(t, first, "thinking.type", "adaptive")
				checkValue(t, first, "thinking.display", "summarized")
				checkValue(t, first, "output_config.effort", "high")
				if !reflect.DeepEqual(first["system"], second["system"]) {
					t.Error("captured system changed across steps")
				}
			case "responses":
				checkValue(t, first, "reasoning.effort", "high")
				checkValue(t, first, "reasoning.summary", "auto")
				checkValue(t, first, "prompt_cache_key", binding.Session.String())
				if first["instructions"] != systemText || second["instructions"] != systemText {
					t.Error("captured system bytes changed")
				}
			case "chat":
				checkValue(t, first, "reasoning_effort", "high")
				checkValue(t, first, "max_tokens", float64(8192))
				checkValue(t, first, "prompt_cache_key", binding.Session.String())
			case "google":
				checkValue(t, first, "generationConfig.thinkingConfig.thinkingBudget", float64(0))
				checkValue(t, first, "generationConfig.thinkingConfig.includeThoughts", false)
				if !reflect.DeepEqual(first["systemInstruction"], second["systemInstruction"]) {
					t.Error("captured system bytes changed")
				}
			}
		})
	}
	t.Run("scoped_history_and_summary", func(t *testing.T) {
		provider := protocol.DefaultConversationProvider()
		provider.ID = protocol.NewProviderID()
		provider.Api = protocol.ConversationModelApiAnthropic
		provider.Authentication = protocol.AuthenticationApiKey
		provider.Models = []protocol.ConversationModel{{ID: "fixture", Output: 4096, Reasoning: true, Efforts: []protocol.Effort{{Kind: "high"}, {Kind: "low"}, {Kind: "none"}}}}
		binding := Binding{Provider: provider, Config: protocol.SessionConfig{Provider: provider.ID, Model: "fixture", Effort: protocol.Effort{Kind: "high"}}}
		signature := "historical-signature"
		message := canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "thinking", Data: &canonical.Thinking{Text: "Historical reasoning", Signature: &signature}}, canonical.Text(1, "Historical answer"))
		record := canonical.Record{ID: "prior", Turn: protocol.NewTurnID(), Kind: "message", Data: message, Metadata: canonical.Metadata{Runtime: &canonical.Runtime{AgentVisible: true, Inference: &canonical.Inference{Provider: "sailry:" + provider.ID.String(), RequestedModel: "other"}}}}
		for _, match := range []bool{false, true} {
			if match {
				record.Metadata.Runtime.Inference.RequestedModel = "fixture"
			}
			call, err := BuildCall(binding, Request{Records: []canonical.Record{record}})
			if err != nil {
				t.Fatal(err)
			}
			raw, err := requestJSON(transportConfig{Kind: "anthropic", Model: "fixture"}, call)
			if err != nil {
				t.Fatal(err)
			}
			body, _ := json.Marshal(raw)
			if strings.Contains(string(body), "historical-signature") != match || strings.Contains(string(body), "Historical reasoning") != match {
				t.Errorf("signed history scope match=%v: %s", match, body)
			}
		}
		summary := Summary(binding)
		if summary.Config.Effort.Kind != "none" || binding.Config.Effort.Kind != "high" || OutputLimit(binding, true) != 4096 {
			t.Fatalf("summary policy=%+v", summary.Config.Effort)
		}
		// The summary output is twice the handoff cap of the window, within the
		// model's own limit; a 200,000 token window keeps 8,000.
		for _, row := range []struct {
			context, output uint32
			want            int64
		}{{200000, 64000, 8000}, {32000, 8000, 8000}, {400000, 64000, 16000}, {1000000, 64000, 24000}, {1000000, 10000, 10000}, {1000000, 0, 8000}} {
			sized := binding
			sized.Provider.Models = []protocol.ConversationModel{{ID: "fixture", Context: row.context, Output: row.output}}
			if got := OutputLimit(sized, true); got != row.want {
				t.Errorf("summary output of a %d window with %d output = %d, want %d", row.context, row.output, got, row.want)
			}
		}
	})
	geminiCarrierRoundtrip(t)
	providerResourceFixtures(t)
}
func providerResourceFixtures(t *testing.T) {
	t.Run("provider_catalog_and_efforts", func(t *testing.T) {
		download, err := providers.ParseCatalog([]byte(`{"fixture":{"models":{"reasoner":{"id":"reasoner","name":"Reference","limit":{"context":100000,"output":8192},"modalities":{"input":["text","image"],"output":["text"]},"tool_call":true,"reasoning":true,"reasoning_options":[{"type":"effort","values":["low","high","max"]},{"type":"toggle"}]}}}}`))
		if err != nil {
			t.Fatal(err)
		}
		if download.Providers != 1 || len(download.Entries) != 1 {
			t.Fatal("catalog entries changed")
		}
		reference, err := providers.CatalogModel(download.Entries[0].Body)
		if err != nil {
			t.Fatal(err)
		}
		completed := providers.CompleteModel(protocol.ConversationModel{ID: "reasoner", Context: 200000, Output: 8192, DefaultEffort: protocol.Effort{Kind: "default"}}, &reference, protocol.ConversationModelApiDeepSeek)
		if !completed.Reasoning || !completed.Vision || !completed.Tools || completed.Context != 200000 || completed.DefaultEffort.Kind != "high" {
			t.Fatalf("completed metadata=%+v", completed)
		}
		efforts := []string{}
		for _, effort := range completed.Efforts {
			efforts = append(efforts, effort.Kind)
		}
		if !reflect.DeepEqual(efforts, []string{"high", "max", "none"}) {
			t.Fatalf("catalog choices=%v", efforts)
		}
	})
	t.Run("provider_score_rows", func(t *testing.T) {
		dataset, err := providers.ParseScores([]byte(`{"snapshot":{"batch_id":"batch","published_at":"2026-01-01","entries":[{"candidate_id":"a","model_id":"model-a","reasoning_effort":"high","overall_score":90,"backend_score":90.2,"frontend_score":81.8,"knowledge_score":92,"aesthetic_score":80,"maintainability_score":88}]},"displayNames":{"model-a":"Model A"},"metrics":{"a":{"backend":{"elapsed_ms":5,"estimated_api_cost_usd":0.1},"frontend":{"elapsed_ms":7,"estimated_api_cost_usd":0.2}}}}`))
		if err != nil {
			t.Fatal(err)
		}
		if len(dataset.Entries) != 1 || dataset.Entries[0].Score.ElapsedMs == nil || *dataset.Entries[0].Score.ElapsedMs != 12 || dataset.Entries[0].Score.Axes != [5]uint8{90, 82, 92, 80, 88} {
			t.Fatalf("scores=%+v", dataset)
		}
		if providers.NormalizeScoreName("vendor/Model_A-2026-01-02") != "model-a" {
			t.Error("score snapshot alias normalization changed")
		}
	})
	t.Run("provider_discovery_pagination", func(t *testing.T) {
		requests := 0
		server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
			requests++
			if request.Header.Get("x-api-key") != "fixture-key" || request.Header.Get("anthropic-version") != "2023-06-01" {
				t.Error("discovery authorization changed")
			}
			writer.Header().Set("Content-Type", "application/json")
			if request.URL.Path != "/v1/models" {
				t.Errorf("discovery path=%s", request.URL.Path)
			}
			if requests == 1 {
				io.WriteString(writer, `{"data":[{"id":"a","max_input_tokens":100000,"max_tokens":8192}],"has_more":true,"last_id":"a"}`)
			} else {
				if request.URL.Query().Get("after_id") != "a" {
					t.Error("pagination cursor was not preserved")
				}
				io.WriteString(writer, `{"data":[{"id":"b"}],"has_more":false}`)
			}
		}))
		defer server.Close()
		provider := protocol.ConversationProvider{Api: protocol.ConversationModelApiAnthropic, Authentication: protocol.AuthenticationApiKey, Endpoint: server.URL + "/v1"}
		secret := protocol.NewSecret("fixture-key")
		catalog, err := providers.Discover(t.Context(), provider, &secret)
		if err != nil {
			t.Fatal(err)
		}
		if requests != 2 || len(catalog.Models) != 2 || catalog.Models[0].Context == nil || *catalog.Models[0].Context != 100000 || catalog.Models[1].Context != nil {
			t.Fatalf("discovery facts=%+v; requests=%d", catalog, requests)
		}
	})
}

func sdkFailureFixtures(t *testing.T) {
	for _, row := range []struct {
		code string
		kind FailureKind
	}{{"context_length_exceeded", ContextOverflow}, {"rate_limit_exceeded", RateLimited}, {"server_error", Unavailable}, {"invalid_api_key", Authentication}} {
		t.Run("sdk_stream_"+row.code, func(t *testing.T) {
			raw, _ := json.Marshal(map[string]any{"type": "response.failed", "sequence_number": 1, "response": map[string]any{"id": "failed", "object": "response", "created_at": 1, "status": "failed", "model": "fixture", "output": []any{}, "error": map[string]any{"code": row.code, "message": "Rejected credential captured-key"}}})
			server := serverFixture(t, "data: "+string(raw)+"\n\n")
			languageModel, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "captured-key"})
			if err != nil {
				t.Fatal(err)
			}
			stream, err := languageModel.Stream(t.Context(), requestCall(t, "responses"))
			if err != nil {
				t.Fatal(err)
			}
			for part := range stream {
				if part.Type == "error" {
					err = part.Error
				}
			}
			failure := Classify(err, protocol.NewSecret("captured-key"))
			if failure == nil || failure.Kind != row.kind || strings.Contains(failure.Message, "captured-key") || !strings.Contains(failure.Message, "[redacted]") {
				t.Fatalf("SDK rejection=%+v", failure)
			}
			if len(server.captured()) != 1 {
				t.Fatal("SDK silently retried a rejected step")
			}
		})
	}
	t.Run("sdk_top_level_error", func(t *testing.T) {
		server := serverFixture(t, "data: {\"type\":\"error\",\"error\":{\"code\":\"invalid_api_key\",\"message\":\"Rejected captured-key\"}}\n\n")
		languageModel, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "captured-key"})
		if err != nil {
			t.Fatal(err)
		}
		stream, err := languageModel.Stream(t.Context(), requestCall(t, "responses"))
		if err != nil {
			t.Fatal(err)
		}
		for part := range stream {
			if part.Type == "error" {
				err = part.Error
			}
		}
		failure := Classify(err, protocol.NewSecret("captured-key"))
		if failure == nil || failure.Kind != Authentication || !strings.Contains(failure.Message, "[redacted]") {
			t.Fatalf("SDK top-level rejection=%+v", failure)
		}
	})
}

func geminiCarrierRoundtrip(t *testing.T) {
	for _, tool := range []bool{false, true} {
		t.Run(fmt.Sprintf("gemini_canonical_carrier_tool_%v", tool), func(t *testing.T) {
			response := string(fixture(t, "google-tool.sse"))
			if !tool {
				response = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hidden thought\",\"thought\":true,\"thoughtSignature\":\"c2lnbmVkLXRob3VnaHQ=\"}]},\"finishReason\":\"STOP\",\"index\":0}]}\n\n"
			}
			server := serverFixture(t, response)
			languageModel, err := newSDK(t.Context(), transportConfig{Kind: "google", Model: "fixture", Endpoint: server.URL, Key: "captured-key"})
			if err != nil {
				t.Fatal(err)
			}
			parts := collect(t, languageModel, requestCall(t, "google"))
			message := canonical.NewMessage(canonical.Assistant)
			var thinking *canonical.Block
			for _, part := range parts {
				switch part.Type {
				case fantasy.StreamPartTypeReasoningEnd:
					if metadata, ok := part.ProviderMetadata[google.Name].(*google.ReasoningMetadata); ok && metadata.ToolID != "" {
						t.Fatal("tool signature was emitted as a separate thinking carrier")
					}
					if tool {
						continue
					}
					if thinking == nil {
						value := canonical.Block{Index: 0, Kind: "thinking", Data: &canonical.Thinking{}}
						thinking = &value
					}
					if err := ApplyStreamMetadata(thinking, part); err != nil {
						t.Fatal(err)
					}
				case fantasy.StreamPartTypeReasoningDelta:
					if tool {
						continue
					}
					if thinking == nil {
						value := canonical.Block{Index: 0, Kind: "thinking", Data: &canonical.Thinking{}}
						thinking = &value
					}
					thinking.Data.(*canonical.Thinking).Text += part.Delta
				case fantasy.StreamPartTypeToolCall:
					block := canonical.Block{Index: uint64(len(message.Content)), Kind: "tool_request", Data: &canonical.ToolRequest{ID: part.ID, Name: part.ToolCallName, Arguments: json.RawMessage(part.ToolCallInput)}}
					if err := ApplyStreamMetadata(&block, part); err != nil {
						t.Fatal(err)
					}
					message.Content = append(message.Content, block)
				}
			}
			if thinking != nil {
				message.Content = append(message.Content, *thinking)
			}
			message.PartCount = uint64(len(message.Content))
			provider := protocol.ConversationProvider{ID: protocol.NewProviderID(), Api: protocol.ConversationModelApiGemini, Authentication: protocol.AuthenticationApiKey}
			binding := Binding{Provider: provider, Config: protocol.SessionConfig{Provider: provider.ID, Model: "fixture", Effort: protocol.Effort{Kind: "default"}}}
			record := canonical.NewRecord(protocol.NewTurnID(), "message", message, policyClock{}.now)
			record.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, Inference: &canonical.Inference{Provider: "sailry:" + provider.ID.String(), RequestedModel: "fixture"}}
			call, err := BuildCall(binding, Request{Records: []canonical.Record{record}})
			if err != nil {
				t.Fatal(err)
			}
			body, err := requestJSON(transportConfig{Kind: "google", Model: "fixture"}, call)
			if err != nil {
				t.Fatal(err)
			}
			checkValue(t, body, "contents.0.parts.0.thoughtSignature", base64.StdEncoding.EncodeToString([]byte("signed-thought")))
			if tool {
				checkValue(t, body, "contents.0.parts.0.functionCall.id", "stable-call")
				if len(wireArray(wireObject(wireArray(body["contents"])[0])["parts"])) != 1 {
					t.Fatal("tool signature was duplicated on replay")
				}
			} else {
				checkValue(t, body, "contents.0.parts.0.text", "Hidden thought")
				checkValue(t, body, "contents.0.parts.0.thought", true)
			}
		})
	}
}
