package model

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"

	"charm.land/fantasy"
	fantasygoogle "charm.land/fantasy/providers/google"
	"github.com/MoeForge/moego/base64"
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
	clientConfig := &genai.ClientConfig{Backend: genai.BackendGeminiAPI, APIKey: model.config.Key, HTTPClient: model.client, HTTPOptions: genai.HTTPOptions{BaseURL: model.config.Endpoint, Headers: headers, RetryOptions: &genai.HTTPRetryOptions{Attempts: &attempts}}}
	if model.config.vertex != nil {
		// The client's transport supplies the access token; the key field
		// holds the service account, which never leaves the Node. An explicit
		// project, location, endpoint and client keep the SDK from reading
		// ambient credentials or environment settings.
		clientConfig.Backend, clientConfig.APIKey = genai.BackendVertexAI, ""
		clientConfig.Project, clientConfig.Location = model.config.Project, model.config.Location
		clientConfig.HTTPOptions.BaseURL = model.config.Endpoint + "/"
	}
	client, err := genai.NewClient(ctx, clientConfig)
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
	want := max(config.CandidateCount, 1)
	finished := map[int64]bool{}
	tools := false
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
			emitter.googleSources(candidate)
			for _, value := range wireArray(wireObject(candidate["content"])["parts"]) {
				part := wireObject(value)
				id := "text"
				if part["thought"] == true {
					id = "reasoning"
				}
				function := wireObject(part["functionCall"])
				if function != nil {
					if wireString(function["name"]) == "" || function["willContinue"] == true || len(wireArray(function["partialArgs"])) != 0 {
						emitter.fail(fmt.Errorf("model tool input is incomplete"))
						return
					}
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
			switch reason := genai.FinishReason(wireString(candidate["finishReason"])); reason {
			case "", genai.FinishReasonUnspecified:
				continue
			case genai.FinishReasonStop:
				finish = fantasy.FinishReasonStop
			case genai.FinishReasonMaxTokens:
				finish = fantasy.FinishReasonLength
			case genai.FinishReasonSafety, genai.FinishReasonRecitation, genai.FinishReasonBlocklist, genai.FinishReasonProhibitedContent, genai.FinishReasonSPII:
				finish = fantasy.FinishReasonContentFilter
			case genai.FinishReasonLanguage, genai.FinishReasonOther, genai.FinishReasonImageSafety, genai.FinishReasonTooManyToolCalls, genai.FinishReasonImageProhibitedContent, genai.FinishReasonNoImage, genai.FinishReasonImageRecitation, genai.FinishReasonImageOther:
				finish = fantasy.FinishReasonUnknown
			default:
				emitter.fail(fmt.Errorf("model generation did not complete: %s", reason))
				return
			}
			// The SDK omits a zero-valued candidate index when marshaling.
			index := int64(0)
			if n := wireInt(candidate["index"]); n != nil {
				index = *n
			}
			if index >= 0 && index < int64(want) {
				finished[index] = true
			}
		}
		if len(finished) == int(want) {
			break
		}
	}
	if err := ctx.Err(); err != nil {
		emitter.fail(err)
		return
	}
	if len(finished) != int(want) {
		emitter.fail(io.ErrUnexpectedEOF)
		return
	}
	if tools {
		finish = fantasy.FinishReasonToolCalls
	}
	emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: finish})
}
