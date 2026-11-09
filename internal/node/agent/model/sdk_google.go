package model

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"

	"charm.land/fantasy"
	fantasygoogle "charm.land/fantasy/providers/google"
	"github.com/google/uuid"
	"google.golang.org/genai"
)

func (model *sdkModel) streamGoogle(ctx context.Context, body map[string]any, call fantasy.Call, emitter *sdkEmitter) {
	attempts := int32(1)
	headers := http.Header{}
	for key, value := range model.config.Headers {
		headers.Set(key, value)
	}
	for key, value := range call.Headers {
		headers.Set(key, value)
	}
	if call.UserAgent != "" {
		headers.Set("User-Agent", call.UserAgent)
	}
	client, err := genai.NewClient(ctx, &genai.ClientConfig{Backend: genai.BackendGeminiAPI, APIKey: model.config.Key, HTTPClient: model.client, HTTPOptions: genai.HTTPOptions{BaseURL: model.config.Endpoint, Headers: headers, RetryOptions: &genai.HTTPRetryOptions{Attempts: &attempts}}})
	if err != nil {
		emitter.fail(err)
		return
	}
	config := &genai.GenerateContentConfig{}
	decode := func(value any, target any) error {
		encoded, err := json.Marshal(value)
		if err != nil {
			return err
		}
		return json.Unmarshal(encoded, target)
	}
	var contents []*genai.Content
	if err := decode(body["contents"], &contents); err != nil {
		emitter.fail(err)
		return
	}
	if err := decode(body["generationConfig"], config); err != nil {
		emitter.fail(err)
		return
	}
	if system := body["systemInstruction"]; system != nil {
		if err := decode(system, &config.SystemInstruction); err != nil {
			emitter.fail(err)
			return
		}
	}
	if tools := body["tools"]; tools != nil {
		if err := decode(tools, &config.Tools); err != nil {
			emitter.fail(err)
			return
		}
	}
	if cached, ok := body["cachedContent"].(string); ok {
		config.CachedContent = cached
	}
	continuation := false
	config.AutomaticContinuation = &continuation
	config.HTTPOptions = &genai.HTTPOptions{RetryOptions: &genai.HTTPRetryOptions{Attempts: &attempts}}
	terminal, tools := false, false
	finish := fantasy.FinishReasonStop
	for chunk, err := range client.Models.GenerateContentStream(ctx, model.config.Model, contents, config) {
		if err != nil {
			emitter.fail(err)
			return
		}
		if chunk == nil {
			continue
		}
		raw := ""
		if chunk.SDKHTTPResponse != nil {
			raw = chunk.SDKHTTPResponse.Body
		}
		precise := raw != ""
		if !precise {
			encoded, err := json.Marshal(chunk)
			if err != nil {
				emitter.fail(err)
				return
			}
			raw = string(encoded)
		}
		value, err := decodeSDK(raw)
		if err != nil {
			emitter.fail(err)
			return
		}
		if precise {
			emitter.updateUsage(wireObject(value["usageMetadata"]), "google")
		} else if observer := emitter.facts.observer; observer != nil {
			emitter.usage = observer.snapshot()
		} else if chunk.UsageMetadata != nil {
			// This exact SDK leaves SDKHTTPResponse.Body empty. Nonzero typed
			// counters remain usable, but zero field presence is unavailable.
			// Preserve that gap explicitly; never manufacture known-zero counts.
			emitter.updateUsage(wireObject(value["usageMetadata"]), "google")
			emitter.usage.Precise = false
			emitter.usage.Raw = nil
		}
		for _, value := range wireArray(value["candidates"]) {
			candidate := wireObject(value)
			for _, value := range wireArray(wireObject(candidate["content"])["parts"]) {
				part := wireObject(value)
				id := "text"
				if part["thought"] == true {
					id = "reasoning"
				}
				function := wireObject(part["functionCall"])
				if function != nil {
					id = wireString(function["id"])
					if id == "" {
						id = uuid.NewString()
					}
				}
				metadata := fantasy.ProviderMetadata{}
				if signature, ok := part["thoughtSignature"].(string); ok {
					decoded, err := base64.StdEncoding.DecodeString(signature)
					if err != nil {
						emitter.fail(err)
						return
					}
					toolID := ""
					if function != nil {
						toolID = id
					}
					metadata[fantasygoogle.Name] = &fantasygoogle.ReasoningMetadata{Signature: string(decoded), ToolID: toolID}
					if function == nil && !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeReasoningEnd, ID: id, ProviderMetadata: metadata}) {
						return
					}
				}
				if text, ok := part["text"].(string); ok && text != "" {
					kind := fantasy.StreamPartTypeTextDelta
					if part["thought"] == true {
						kind = fantasy.StreamPartTypeReasoningDelta
					}
					if !emitter.emit(fantasy.StreamPart{Type: kind, ID: id, Delta: text}) {
						return
					}
				}
				if function != nil {
					arguments, err := json.Marshal(function["args"])
					if err != nil {
						emitter.fail(err)
						return
					}
					tools = true
					if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: id, ToolCallName: wireString(function["name"]), ToolCallInput: string(arguments), ProviderMetadata: metadata}) {
						return
					}
				}
			}
			if reason := wireString(candidate["finishReason"]); reason != "" {
				terminal = true
				switch reason {
				case "STOP":
					finish = fantasy.FinishReasonStop
				case "MAX_TOKENS":
					finish = fantasy.FinishReasonLength
				case "SAFETY", "RECITATION", "BLOCKLIST", "PROHIBITED_CONTENT", "SPII":
					finish = fantasy.FinishReasonContentFilter
				default:
					finish = fantasy.FinishReasonUnknown
				}
			}
		}
	}
	if !terminal {
		emitter.fail(io.ErrUnexpectedEOF)
		return
	}
	if tools {
		finish = fantasy.FinishReasonToolCalls
	}
	emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: finish})
}
