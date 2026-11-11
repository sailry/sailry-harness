package model

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"sort"
	"strings"

	"charm.land/fantasy"
	fantasyopenai "charm.land/fantasy/providers/openai"
	openaisdk "github.com/openai/openai-go/v3"
	"github.com/openai/openai-go/v3/option"
	"github.com/openai/openai-go/v3/responses"
)

func (model *sdkModel) openAIOptions(call fantasy.Call) []option.RequestOption {
	opts := []option.RequestOption{option.WithHTTPClient(model.client), option.WithBaseURL(strings.TrimSuffix(model.config.Endpoint, "/chat/completions")), option.WithAPIKey(model.config.Key), option.WithMaxRetries(0)}
	for key, value := range model.config.Headers {
		opts = append(opts, option.WithHeader(key, value))
	}
	for key, value := range call.Headers {
		opts = append(opts, option.WithHeader(key, value))
	}
	for key, value := range model.config.Query {
		opts = append(opts, option.WithQuery(key, value))
	}
	for _, key := range model.config.RemoveHeaders {
		opts = append(opts, option.WithHeaderDel(key))
	}
	if call.UserAgent != "" {
		opts = append(opts, option.WithHeader("User-Agent", call.UserAgent))
	}
	return opts
}

func (model *sdkModel) streamResponses(ctx context.Context, raw []byte, call fantasy.Call, emitter *sdkEmitter) {
	client := openaisdk.NewClient(model.openAIOptions(call)...)
	stream := client.Responses.NewStreaming(ctx, responses.ResponseNewParams{}, option.WithRequestBody("application/json", raw))
	defer stream.Close()
	texts, tools, reasoning := map[string]string{}, map[string]bool{}, map[string]bool{}
	callItems := map[string]map[string]any{}
	sourceItems := map[int64]map[string]any{}
	terminal, incomplete := false, false
	reason := fantasy.FinishReasonStop
	toolCount := 0
	emitTool := func(item map[string]any) bool {
		id := wireString(item["call_id"])
		if id == "" || wireString(item["name"]) == "" {
			emitter.fail(fmt.Errorf("model tool identity is incomplete"))
			return false
		}
		if tools[id] {
			return true
		}
		arguments := wireString(item["arguments"])
		if !json.Valid([]byte(arguments)) {
			// A call the response ended inside is not a call: the complete
			// calls and the partial answer stay.
			if incomplete || item["status"] == "incomplete" {
				return true
			}
			if strings.TrimSpace(arguments) != "" {
				emitter.fail(fmt.Errorf("model tool input is incomplete"))
				return false
			}
			arguments = "{}"
		}
		tools[id] = true
		toolCount++
		return emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: id, ToolCallName: wireString(item["name"]), ToolCallInput: arguments})
	}
	emitReasoning := func(item map[string]any) bool {
		id := wireString(item["id"])
		if reasoning[id] {
			return true
		}
		metadata := &fantasyopenai.ResponsesReasoningMetadata{ItemID: id}
		if encrypted, ok := item["encrypted_content"].(string); ok {
			metadata.EncryptedContent = &encrypted
		}
		for _, value := range wireArray(item["summary"]) {
			if text, ok := wireObject(value)["text"].(string); ok {
				metadata.Summary = append(metadata.Summary, text)
			}
		}
		if metadata.EncryptedContent == nil && len(metadata.Summary) == 0 {
			return true
		}
		reasoning[id] = true
		return emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeReasoningEnd, ID: id, ProviderMetadata: fantasy.ProviderMetadata{fantasyopenai.Name: metadata}})
	}
	for stream.Next() {
		value, err := decodeSDK(stream.Current().RawJSON())
		if err != nil {
			emitter.fail(err)
			return
		}
		if response := wireObject(value["response"]); response != nil {
			emitter.updateUsage(wireObject(response["usage"]), "responses")
		}
		switch wireString(value["type"]) {
		case "response.output_text.delta":
			id := wireString(value["item_id"])
			texts[id] += wireString(value["delta"])
			if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: id, Delta: wireString(value["delta"])}) {
				return
			}
		case "response.reasoning_summary_text.delta", "response.reasoning_text.delta":
			if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeReasoningDelta, ID: wireString(value["item_id"]), Delta: wireString(value["delta"])}) {
				return
			}
		case "response.function_call_arguments.delta":
			id := wireString(value["item_id"])
			name := ""
			if item := callItems[id]; item != nil {
				id, name = wireString(item["call_id"]), wireString(item["name"])
			}
			if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolInputDelta, ID: id, ToolCallName: name, Delta: wireString(value["delta"])}) {
				return
			}
		case "response.output_item.added":
			item := wireObject(value["item"])
			if item["type"] == "function_call" {
				callItems[wireString(item["id"])] = item
				if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolInputStart, ID: wireString(item["call_id"]), ToolCallName: wireString(item["name"])}) {
					return
				}
			}
		case "response.output_item.done":
			item := wireObject(value["item"])
			if index := wireInt(value["output_index"]); index != nil && item != nil {
				sourceItems[*index] = item
				indexes := make([]int64, 0, len(sourceItems))
				for index := range sourceItems {
					indexes = append(indexes, index)
				}
				sort.Slice(indexes, func(i, j int) bool { return indexes[i] < indexes[j] })
				items := make([]any, 0, len(indexes))
				for _, index := range indexes {
					items = append(items, sourceItems[index])
				}
				emitter.responseSources(items)
			}
			switch item["type"] {
			case "function_call":
				if !emitTool(item) {
					return
				}
			case "reasoning":
				if !emitReasoning(item) {
					return
				}
			}
		case "response.failed", "error":
			response := wireObject(value["response"])
			emitter.updateUsage(wireObject(response["usage"]), "responses")
			if output := wireArray(response["output"]); len(output) > 0 {
				emitter.responseSources(output)
			}
			emitter.facts.body, _ = json.Marshal(value)
			emitter.fail(fmt.Errorf("model response ended with %s", wireString(value["type"])))
			return
		case "response.completed", "response.incomplete":
			// An incomplete response is a terminal that keeps its partial
			// output and usage; its reason is how the answer ended.
			terminal = true
			response := wireObject(value["response"])
			if wireString(value["type"]) == "response.incomplete" {
				incomplete = true
				switch wireString(wireObject(response["incomplete_details"])["reason"]) {
				case "max_output_tokens":
					reason = fantasy.FinishReasonLength
				case "content_filter":
					reason = fantasy.FinishReasonContentFilter
				default:
					reason = fantasy.FinishReasonOther
				}
			}
			emitter.updateUsage(wireObject(response["usage"]), "responses")
			if output := wireArray(response["output"]); len(output) > 0 {
				emitter.responseSources(output)
			}
			for _, value := range wireArray(response["output"]) {
				item := wireObject(value)
				switch item["type"] {
				case "function_call":
					if !emitTool(item) {
						return
					}
				case "reasoning":
					if !emitReasoning(item) {
						return
					}
				case "message":
					id := wireString(item["id"])
					if _, exists := texts[id]; !exists {
						texts[id] = ""
					}
					for _, content := range wireArray(item["content"]) {
						block := wireObject(content)
						if block["type"] != "output_text" {
							continue
						}
						text := wireString(block["text"])
						if !strings.HasPrefix(text, texts[id]) {
							emitter.fail(fmt.Errorf("model completed text conflicts with streamed content"))
							return
						}
						if remaining := strings.TrimPrefix(text, texts[id]); remaining != "" {
							if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: id, Delta: remaining}) {
								return
							}
						}
						texts[id] = text
					}
				}
			}
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
	if toolCount > 0 && !incomplete {
		reason = fantasy.FinishReasonToolCalls
	}
	emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: reason})
}

type chatTool struct {
	id, name, input string
	started         bool
}

func (model *sdkModel) streamChat(ctx context.Context, raw []byte, call fantasy.Call, emitter *sdkEmitter) {
	client := openaisdk.NewClient(model.openAIOptions(call)...)
	stream := client.Chat.Completions.NewStreaming(ctx, openaisdk.ChatCompletionNewParams{}, option.WithRequestBody("application/json", raw))
	defer stream.Close()
	tools := map[int64]*chatTool{}
	reason := ""
	for stream.Next() {
		value, err := decodeSDK(stream.Current().RawJSON())
		if err != nil {
			emitter.fail(err)
			return
		}
		emitter.updateUsage(wireObject(value["usage"]), "chat")
		for _, value := range wireArray(value["choices"]) {
			choice := wireObject(value)
			delta := wireObject(choice["delta"])
			emitter.chatSources(delta)
			if message := wireObject(choice["message"]); message != nil {
				emitter.chatSources(message)
			}
			if text, ok := delta["content"].(string); ok && text != "" {
				if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "text", Delta: text}) {
					return
				}
			}
			for _, field := range []string{"reasoning_content", "reasoning"} {
				if text, ok := delta[field].(string); ok && text != "" {
					if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeReasoningDelta, ID: "reasoning", Delta: text}) {
						return
					}
				}
			}
			for _, value := range wireArray(delta["tool_calls"]) {
				item := wireObject(value)
				index := int64(0)
				if parsed := wireInt(item["index"]); parsed != nil {
					index = *parsed
				}
				tool := tools[index]
				if tool == nil {
					tool = &chatTool{}
					tools[index] = tool
				}
				if id, ok := item["id"].(string); ok {
					tool.id = id
				}
				function := wireObject(item["function"])
				if name, ok := function["name"].(string); ok {
					tool.name += name
				}
				if !tool.started && tool.id != "" {
					tool.started = true
					if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolInputStart, ID: tool.id, ToolCallName: tool.name}) {
						return
					}
				}
				if input, ok := function["arguments"].(string); ok {
					tool.input += input
					if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolInputDelta, ID: tool.id, ToolCallName: tool.name, Delta: input}) {
						return
					}
				}
			}
			if finish, ok := choice["finish_reason"].(string); ok && finish != "" {
				reason = finish
			}
		}
	}
	if err := stream.Err(); err != nil {
		emitter.fail(err)
		return
	}
	if reason == "" {
		emitter.fail(io.ErrUnexpectedEOF)
		return
	}
	indices := make([]int64, 0, len(tools))
	for index := range tools {
		indices = append(indices, index)
	}
	sort.Slice(indices, func(i, j int) bool { return indices[i] < indices[j] })
	for _, index := range indices {
		tool := tools[index]
		if tool.id == "" || tool.name == "" || !json.Valid([]byte(tool.input)) {
			// The output limit cut this call: it is dropped, the complete
			// calls and the partial answer stay.
			if reason == "length" {
				continue
			}
			if tool.id == "" || tool.name == "" || strings.TrimSpace(tool.input) != "" {
				emitter.fail(fmt.Errorf("model tool input is incomplete"))
				return
			}
			tool.input = "{}"
		}
		if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: tool.id, ToolCallName: tool.name, ToolCallInput: tool.input}) {
			return
		}
	}
	finish := fantasy.FinishReasonStop
	switch reason {
	case "tool_calls", "function_call":
		finish = fantasy.FinishReasonToolCalls
	case "length":
		finish = fantasy.FinishReasonLength
	case "content_filter":
		finish = fantasy.FinishReasonContentFilter
	}
	emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: finish})
}
