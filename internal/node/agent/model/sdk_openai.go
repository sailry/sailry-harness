package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"mime"
	"net"
	"net/http"
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

// responsesSSEBody makes a compatible gateway's final JSON event visible to
// the SDK when the HTTP body ends cleanly without the SSE event separator.
// The adapter only accepts an actual response terminal from the extra bytes.
type responsesSSEBody struct {
	io.ReadCloser
	eof, padded bool
	separator   string
}

func (body *responsesSSEBody) Read(buffer []byte) (int, error) {
	if len(buffer) == 0 {
		return 0, nil
	}
	if !body.eof {
		count, err := body.ReadCloser.Read(buffer)
		if err != io.EOF {
			return count, err
		}
		body.eof, body.separator = true, "\n\n"
		if count > 0 {
			return count, nil
		}
	}
	if body.separator == "" {
		return 0, io.EOF
	}
	count := copy(buffer, body.separator)
	body.separator = body.separator[count:]
	body.padded = true
	return count, nil
}

func (model *sdkModel) streamResponses(ctx context.Context, raw []byte, call fantasy.Call, emitter *sdkEmitter) {
	// The service is built without openaisdk.NewClient, which would add the
	// OPENAI_* variables of the machine to a request for the configured provider.
	service := responses.NewResponseService(model.openAIOptions(call)...)
	tail := &responsesSSEBody{}
	stream := service.NewStreaming(ctx, responses.ResponseNewParams{}, option.WithRequestBody("application/json", raw),
		option.WithMiddleware(func(request *http.Request, next option.MiddlewareNext) (*http.Response, error) {
			request = withPreparedRequestObservation(request, raw)
			response, err := next(request)
			if err == nil && response != nil && response.Body != nil && response.StatusCode >= 200 && response.StatusCode < 300 {
				mediaType, _, _ := mime.ParseMediaType(response.Header.Get("Content-Type"))
				if mediaType == "text/event-stream" {
					tail.ReadCloser = response.Body
					response.Body = tail
				}
			}
			return response, err
		}))
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
	// The terminal response carries final output and usage. Its success does
	// not depend on the server closing the HTTP body afterward.
	for !terminal && stream.Next() {
		value, err := decodeSDK(stream.Current().RawJSON())
		if err != nil {
			emitter.fail(err)
			return
		}
		eventType := wireString(value["type"])
		response := wireObject(value["response"])
		if tail.padded {
			status := wireString(response["status"])
			_, hasOutput := response["output"].([]any)
			if !hasOutput || !(eventType == "response.completed" && status == "completed" || eventType == "response.incomplete" && status == "incomplete") {
				emitter.fail(io.ErrUnexpectedEOF)
				return
			}
		}
		if response != nil {
			emitter.updateUsage(wireObject(response["usage"]), "responses")
		}
		switch eventType {
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
			emitter.updateUsage(wireObject(response["usage"]), "responses")
			if output := wireArray(response["output"]); len(output) > 0 {
				emitter.responseSources(output)
			}
			emitter.facts.body, _ = json.Marshal(value)
			emitter.fail(fmt.Errorf("model response ended with %s", eventType))
			return
		case "response.completed", "response.incomplete":
			// An incomplete response is a terminal that keeps its partial
			// output and usage; its reason is how the answer ended.
			terminal = true
			if eventType == "response.incomplete" {
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
		var malformed *json.SyntaxError
		if tail.padded && errors.As(err, &malformed) {
			err = io.ErrUnexpectedEOF
		}
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
	id, name, input   string
	started, hasInput bool
}

func (model *sdkModel) streamChat(ctx context.Context, raw []byte, call fantasy.Call, emitter *sdkEmitter) {
	var options struct {
		N             int `json:"n"`
		StreamOptions struct {
			IncludeUsage bool `json:"include_usage"`
		} `json:"stream_options"`
	}
	if err := json.Unmarshal(raw, &options); err != nil {
		emitter.fail(err)
		return
	}
	// Built without openaisdk.NewClient for the reason given in streamResponses.
	service := openaisdk.NewChatCompletionService(model.openAIOptions(call)...)
	stream := service.NewStreaming(ctx, openaisdk.ChatCompletionNewParams{}, option.WithRequestBody("application/json", raw))
	defer stream.Close()
	tools := map[int64]*chatTool{}
	finished := map[int64]bool{}
	want := max(options.N, 1)
	reason := ""
	// Reasoning a model writes inline in its content is reasoning. A provider
	// that also reports reasoning in its own field is read from that field.
	var inline thinkingFilter
	structured := false
	write := func(parts []thinkingPart) bool {
		for _, part := range parts {
			kind, id := fantasy.StreamPartTypeTextDelta, "text"
			if part.thinking {
				if structured {
					continue
				}
				kind, id = fantasy.StreamPartTypeReasoningDelta, "reasoning"
			}
			if !emitter.emit(fantasy.StreamPart{Type: kind, ID: id, Delta: part.text}) {
				return false
			}
		}
		return true
	}
	for stream.Next() {
		value, err := decodeSDK(stream.Current().RawJSON())
		if err != nil {
			emitter.fail(err)
			return
		}
		usage := wireObject(value["usage"])
		emitter.updateUsage(usage, "chat")
		for _, value := range wireArray(value["choices"]) {
			choice := wireObject(value)
			delta := wireObject(choice["delta"])
			emitter.chatSources(delta)
			if message := wireObject(choice["message"]); message != nil {
				emitter.chatSources(message)
			}
			if text, ok := delta["content"].(string); ok && text != "" {
				if !write(inline.push(text)) {
					return
				}
			}
			for _, field := range []string{"reasoning_content", "reasoning"} {
				if text, ok := delta[field].(string); ok && text != "" {
					structured = true
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
					tool.hasInput = true
					tool.input += input
					if !emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolInputDelta, ID: tool.id, ToolCallName: tool.name, Delta: input}) {
						return
					}
				}
			}
			switch finish := wireString(choice["finish_reason"]); finish {
			case "stop", "tool_calls", "function_call", "length", "content_filter":
				index := wireInt(choice["index"])
				if index == nil && choice["index"] == nil && want == 1 {
					finished[0] = true
				} else if index != nil && *index >= 0 && *index < int64(want) {
					finished[*index] = true
				}
				reason = finish
			case "":
			default:
				emitter.fail(fmt.Errorf("unsupported model finish reason"))
				return
			}
		}
		// Chat can send the requested usage in a separate, choices-empty chunk.
		if len(finished) == want && (usage != nil || !options.StreamOptions.IncludeUsage) {
			break
		}
	}
	// A tag the stream left unfinished is what it was: text, or open reasoning.
	if !write(inline.finish()) {
		return
	}
	// Losing only the usage trailer cannot undo the completed choices. Never
	// suppress cancellation, a provider error event or malformed SSE data.
	if err := ctx.Err(); err != nil {
		emitter.fail(err)
		return
	}
	if err := stream.Err(); err != nil {
		var network net.Error
		lostTrailer := errors.Is(err, io.ErrUnexpectedEOF) || errors.As(err, &network) || http2ProtocolFailure(err)
		if len(finished) != want || !lostTrailer {
			emitter.fail(err)
			return
		}
	}
	if len(finished) != want {
		emitter.fail(io.ErrUnexpectedEOF)
		return
	}
	if (reason == "tool_calls" || reason == "function_call") && len(tools) == 0 {
		emitter.fail(fmt.Errorf("model tool identity is incomplete"))
		return
	}
	indices := make([]int64, 0, len(tools))
	for index := range tools {
		indices = append(indices, index)
	}
	sort.Slice(indices, func(i, j int) bool { return indices[i] < indices[j] })
	for _, index := range indices {
		tool := tools[index]
		input := strings.TrimSpace(tool.input)
		if tool.id == "" || tool.name == "" || !strings.HasPrefix(input, "{") || !json.Valid([]byte(input)) {
			// The output limit cut this call: it is dropped, the complete
			// calls and the partial answer stay.
			if reason == "length" {
				continue
			}
			if tool.id == "" || tool.name == "" || !tool.hasInput || input != "" {
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
