package model

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/url"
	"slices"
	"strings"

	"charm.land/fantasy"
	fantasyanthropic "charm.land/fantasy/providers/anthropic"
	anthropicsdk "github.com/anthropics/anthropic-sdk-go"
	"github.com/anthropics/anthropic-sdk-go/option"
	"github.com/sailry/sailry-harness/internal/node/providers"
)

type anthropicBlock struct {
	kind, id, name, input, text, signature, redacted string
	inputDelta                                       bool
	// raw is a compaction block as the API sent it, kept for replay.
	raw map[string]any
}

func (model *sdkModel) streamAnthropic(ctx context.Context, raw []byte, call fantasy.Call, emitter *sdkEmitter) {
	parsed, err := url.Parse(model.config.Endpoint)
	if err != nil {
		emitter.fail(err)
		return
	}
	base := providers.AnthropicBaseURL(*parsed)
	endpoint := base.String() + "/"
	// WithoutEnvironmentDefaults keeps the SDK from adding ANTHROPIC_* variables
	// or a local profile of the machine to a request for the configured provider.
	opts := []option.RequestOption{option.WithoutEnvironmentDefaults(), option.WithHTTPClient(model.client), option.WithBaseURL(endpoint), option.WithMaxRetries(0)}
	bedrock := model.config.Bedrock()
	if !bedrock {
		opts = append(opts, option.WithAPIKey(model.config.Key))
	}
	for key, value := range model.config.Headers {
		if strings.EqualFold(key, "anthropic-beta") {
			continue
		}
		opts = append(opts, option.WithHeader(key, value))
	}
	for key, value := range call.Headers {
		if strings.EqualFold(key, "anthropic-beta") {
			continue
		}
		opts = append(opts, option.WithHeader(key, value))
	}
	beta := anthropicBetaHeader(endpoint, model.config.OAuth, model.config.AnthropicAPIKey, raw, model.config.Headers, call.Headers)
	if bedrock {
		beta = bedrockBeta(beta, raw)
	}
	if beta != "" {
		opts = append(opts, option.WithHeader("anthropic-beta", beta))
		if !model.config.OAuth && !bedrock {
			opts = append(opts, option.WithQuery("beta", "true"))
		}
	}
	if call.UserAgent != "" {
		opts = append(opts, option.WithHeader("User-Agent", call.UserAgent))
	}
	if model.config.OAuth {
		opts = append(opts, option.WithHeaderDel("X-Api-Key"))
	}
	for _, key := range model.config.RemoveHeaders {
		opts = append(opts, option.WithHeaderDel(key))
	}
	if bedrock {
		opts = append(opts, bedrockOptions(model.config, base.String())...)
	}
	client := anthropicsdk.NewClient(opts...)
	stream := client.Messages.NewStreaming(ctx, anthropicsdk.MessageNewParams{}, option.WithRequestBody("application/json", raw))
	defer stream.Close()
	blocks := map[int64]*anthropicBlock{}
	// cut holds the tool calls whose input did not parse when their block
	// ended, until the stop reason says why.
	cut := map[int64]*anthropicBlock{}
	terminal, reason := false, ""
	var compacted map[string]any
	// message_stop follows final usage; waiting for HTTP EOF afterward can
	// turn an already completed message into a retryable connection failure.
	for !terminal && stream.Next() {
		value, err := decodeSDK(stream.Current().RawJSON())
		if err != nil {
			emitter.fail(err)
			return
		}
		index := int64(0)
		if parsed := wireInt(value["index"]); parsed != nil {
			index = *parsed
		}
		block := blocks[index]
		switch wireString(value["type"]) {
		case "message_start":
			emitter.updateUsage(wireObject(wireObject(value["message"])["usage"]), "anthropic")
		case "message_delta":
			emitter.updateUsage(wireObject(value["usage"]), "anthropic")
			reason = wireString(wireObject(value["delta"])["stop_reason"])
		case "message_stop":
			terminal = true
		case "content_block_start":
			content := wireObject(value["content_block"])
			emitter.anthropicSources(content)
			block = &anthropicBlock{kind: wireString(content["type"]), id: wireString(content["id"]), name: wireString(content["name"]), text: wireString(content["text"]), signature: wireString(content["signature"]), redacted: wireString(content["data"])}
			if block.kind == "tool_use" && (block.id == "" || block.name == "") {
				emitter.fail(fmt.Errorf("model tool identity is incomplete"))
				return
			}
			if block.id == "" {
				block.id = fmt.Sprintf("block-%d", index)
			}
			if model.config.OAuth {
				block.name = strings.TrimPrefix(block.name, "_")
			}
			if input, ok := content["input"]; ok {
				encoded, _ := json.Marshal(input)
				block.input = string(encoded)
			}
			if block.kind == "compaction" {
				block.raw = content
			}
			blocks[index] = block
			if block.kind == "tool_use" {
				if !emitter.emitIndexed(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolInputStart, ID: block.id, ToolCallName: block.name}, index) {
					return
				}
			}
			if block.kind == "text" && block.text != "" {
				if !emitter.emitIndexed(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: block.id, Delta: block.text}, index) {
					return
				}
			}
			if block.kind == "thinking" {
				block.text = wireString(content["thinking"])
				if block.text != "" && !emitter.emitIndexed(fantasy.StreamPart{Type: fantasy.StreamPartTypeReasoningDelta, ID: block.id, Delta: block.text}, index) {
					return
				}
			}
		case "content_block_delta":
			if block == nil {
				emitter.fail(fmt.Errorf("model content delta has no open block"))
				return
			}
			delta := wireObject(value["delta"])
			switch wireString(delta["type"]) {
			case "compaction_delta":
				// Threshold compaction sends its summary in one delta; an
				// on-demand block arrives whole in its start event.
				if block.raw != nil {
					block.raw["content"] = delta["content"]
				}
			case "citations_delta":
				citation := wireObject(delta["citation"])
				if wireString(citation["type"]) == "web_search_result_location" {
					emitter.citation(citation, 0)
				}
			case "text_delta":
				if !emitter.emitIndexed(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: block.id, Delta: wireString(delta["text"])}, index) {
					return
				}
			case "thinking_delta":
				block.text += wireString(delta["thinking"])
				if !emitter.emitIndexed(fantasy.StreamPart{Type: fantasy.StreamPartTypeReasoningDelta, ID: block.id, Delta: wireString(delta["thinking"])}, index) {
					return
				}
			case "signature_delta":
				signature := wireString(delta["signature"])
				block.signature += signature
				if signature != "" {
					if !emitter.emitIndexed(fantasy.StreamPart{Type: fantasy.StreamPartTypeReasoningDelta, ID: block.id, ProviderMetadata: fantasy.ProviderMetadata{fantasyanthropic.Name: &fantasyanthropic.ReasoningOptionMetadata{Signature: signature}}}, index) {
						return
					}
				}
			case "input_json_delta":
				if !block.inputDelta {
					block.input = ""
					block.inputDelta = true
				}
				block.input += wireString(delta["partial_json"])
				if !emitter.emitIndexed(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolInputDelta, ID: block.id, ToolCallName: block.name, Delta: wireString(delta["partial_json"])}, index) {
					return
				}
			}
		case "content_block_stop":
			if block == nil {
				emitter.fail(fmt.Errorf("model content stop has no open block"))
				return
			}
			switch block.kind {
			case "compaction":
				compacted = block.raw
			case "thinking", "redacted_thinking":
				metadata := &fantasyanthropic.ReasoningOptionMetadata{Signature: block.signature, RedactedData: block.redacted}
				if !emitter.emitIndexed(fantasy.StreamPart{Type: fantasy.StreamPartTypeReasoningEnd, ID: block.id, ProviderMetadata: fantasy.ProviderMetadata{fantasyanthropic.Name: metadata}}, index) {
					return
				}
			case "tool_use":
				if !json.Valid([]byte(block.input)) {
					cut[index] = block
				} else if !emitter.emitIndexed(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: block.id, ToolCallName: block.name, ToolCallInput: block.input}, index) {
					return
				}
			}
			delete(blocks, index)
		case "error":
			emitter.facts.body, _ = json.Marshal(value)
			emitter.fail(fmt.Errorf("model stream rejected the request"))
			return
		}
	}
	if err := stream.Err(); err != nil {
		emitter.fail(err)
		return
	}
	if !terminal {
		emitter.fail(io.ErrUnexpectedEOF)
		return
	}
	// A completed message keeps its complete calls. A call the reply ended
	// inside reaches the engine with an error, so the model reads why it did
	// not run; one that only sent no input was called without arguments.
	for index, block := range blocks {
		if block.kind == "tool_use" {
			cut[index] = block
		}
	}
	indices := make([]int64, 0, len(cut))
	for index := range cut {
		indices = append(indices, index)
	}
	slices.Sort(indices)
	for _, index := range indices {
		block := cut[index]
		part := fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: block.id, ToolCallName: block.name, ToolCallInput: "{}"}
		if reason == "max_tokens" || strings.TrimSpace(block.input) != "" {
			part.ProviderMetadata = fantasy.ProviderMetadata{pluginMetadataKey: &pluginOutputMetadata{ToolError: cutToolError(reason == "max_tokens")}}
		}
		if !emitter.emitIndexed(part, index) {
			return
		}
	}
	finish := fantasy.FinishReasonStop
	switch reason {
	case "tool_use":
		finish = fantasy.FinishReasonToolCalls
	case "max_tokens":
		finish = fantasy.FinishReasonLength
	case "refusal":
		finish = fantasy.FinishReasonContentFilter
	case "end_turn", "stop_sequence", "compaction":
	default:
		finish = fantasy.FinishReasonUnknown
	}
	part := fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: finish}
	// A blocked or missing summary leaves no block or one with null content,
	// whatever the stop reason says; only a block with text is a summary.
	if summary, ok := compacted["content"].(string); ok && strings.TrimSpace(summary) != "" {
		items, err := json.Marshal([]any{compacted})
		if err != nil {
			emitter.fail(err)
			return
		}
		part.ProviderMetadata = fantasy.ProviderMetadata{nativeCompactionKey: &nativeOutput{Summary: summary, Items: items}}
	} else if compacted == nil && nativeRequest(call) != nil {
		part.ProviderMetadata = fantasy.ProviderMetadata{nativeCompactionKey: &nativeOutput{Ignored: true}}
	}
	emitter.emit(part)
}

func anthropicBetaHeader(endpoint string, oauth, apiKey bool, raw []byte, configHeaders, callHeaders map[string]string) string {
	var betas []string
	seen := map[string]bool{}
	addList := func(value string) {
		for _, part := range strings.Split(value, ",") {
			token := strings.TrimSpace(part)
			if token == "" || seen[token] {
				continue
			}
			seen[token] = true
			betas = append(betas, token)
		}
	}
	if apiKey {
		addList("claude-code-20250219")
		var payload struct {
			Thinking struct {
				Type string `json:"type"`
			} `json:"thinking"`
			OutputConfig struct {
				Effort string `json:"effort"`
			} `json:"output_config"`
		}
		if json.Unmarshal(raw, &payload) == nil {
			if payload.Thinking.Type == "adaptive" || payload.Thinking.Type == "enabled" {
				addList("interleaved-thinking-2025-05-14")
			}
			if payload.OutputConfig.Effort != "" {
				addList("effort-2025-11-24")
			}
		}
	}
	for _, headers := range []map[string]string{configHeaders, callHeaders} {
		for key, value := range headers {
			if strings.EqualFold(key, "anthropic-beta") {
				addList(value)
			}
		}
	}
	if anthropicCompacts(raw) {
		addList(anthropicCompactBeta)
	}
	if !oauth && isOfficialAnthropicEndpoint(endpoint) {
		var payload struct {
			Thinking struct {
				Type string `json:"type"`
			} `json:"thinking"`
			Tools []json.RawMessage `json:"tools"`
			Speed string            `json:"speed"`
		}
		if json.Unmarshal(raw, &payload) == nil {
			if payload.Thinking.Type == "enabled" && len(payload.Tools) > 0 {
				addList("interleaved-thinking-2025-05-14")
			}
			if payload.Speed == "fast" {
				addList("fast-mode-2026-02-01")
			}
		}
	}
	return strings.Join(betas, ",")
}

// anthropicCompacts reports a request that asks for an on-demand summary or
// carries the signed block one returned; both need the compaction beta.
func anthropicCompacts(raw []byte) bool {
	var payload struct {
		Compaction json.RawMessage `json:"compaction"`
		Messages   []struct {
			Content json.RawMessage `json:"content"`
		} `json:"messages"`
	}
	if json.Unmarshal(raw, &payload) != nil {
		return false
	}
	if len(payload.Compaction) > 0 && string(payload.Compaction) != "null" {
		return true
	}
	for _, message := range payload.Messages {
		var blocks []struct {
			Type string `json:"type"`
		}
		if json.Unmarshal(message.Content, &blocks) == nil && slices.ContainsFunc(blocks, func(block struct {
			Type string `json:"type"`
		}) bool {
			return block.Type == "compaction"
		}) {
			return true
		}
	}
	return false
}

func isOfficialAnthropicEndpoint(endpoint string) bool {
	parsed, err := url.Parse(strings.TrimSpace(endpoint))
	if err != nil {
		return false
	}
	return strings.EqualFold(parsed.Hostname(), "api.anthropic.com")
}
