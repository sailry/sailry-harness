package model

import (
	"encoding/json"
	"fmt"
	"strings"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"charm.land/fantasy/providers/google"
	"charm.land/fantasy/providers/openai"

	"github.com/MoeForge/moego/base64"
)

func requestJSON(config transportConfig, call fantasy.Call) (map[string]any, error) {
	body := map[string]any{"model": config.Model, "stream": true}
	switch config.Kind {
	case "anthropic":
		messages, system := []any{}, []any{}
		for _, message := range call.Prompt {
			blocks := []any{}
			for index, part := range message.Content {
				var block map[string]any
				switch part.GetType() {
				case fantasy.ContentTypeText:
					value, _ := fantasy.AsMessagePart[fantasy.TextPart](part)
					block = map[string]any{"type": "text", "text": value.Text}
				case fantasy.ContentTypeFile:
					value, _ := fantasy.AsMessagePart[fantasy.FilePart](part)
					kind := "image"
					if !strings.HasPrefix(value.MediaType, "image/") {
						kind = "document"
					}
					block = map[string]any{"type": kind, "source": map[string]any{"type": "base64", "media_type": value.MediaType, "data": base64.StdEncoding.EncodeToString(value.Data)}}
				case fantasy.ContentTypeReasoning:
					value, _ := fantasy.AsMessagePart[fantasy.ReasoningPart](part)
					if metadata, ok := part.Options()[anthropic.Name].(*anthropic.ReasoningOptionMetadata); ok {
						if metadata.Signature != "" {
							block = map[string]any{"type": "thinking", "thinking": value.Text, "signature": metadata.Signature}
						} else if metadata.RedactedData != "" {
							block = map[string]any{"type": "redacted_thinking", "data": metadata.RedactedData}
						}
					}
				case fantasy.ContentTypeToolCall:
					value, _ := fantasy.AsMessagePart[fantasy.ToolCallPart](part)
					var input any
					if err := json.Unmarshal([]byte(value.Input), &input); err != nil {
						return nil, err
					}
					block = map[string]any{"type": "tool_use", "id": value.ToolCallID, "name": value.ToolName, "input": json.RawMessage(value.Input)}
				case fantasy.ContentTypeToolResult:
					value, _ := fantasy.AsMessagePart[fantasy.ToolResultPart](part)
					block = map[string]any{"type": "tool_result", "tool_use_id": value.ToolCallID, "content": toolText(value.Output)}
				}
				if block != nil {
					cache := anthropic.GetCacheControl(part.Options())
					if cache == nil && index == len(message.Content)-1 {
						cache = anthropic.GetCacheControl(message.ProviderOptions)
					}
					if cache != nil {
						block["cache_control"] = map[string]any{"type": cache.Type}
					}
					blocks = append(blocks, block)
				}
			}
			if message.Role == fantasy.MessageRoleSystem {
				system = append(system, blocks...)
			} else {
				role := string(message.Role)
				if message.Role == fantasy.MessageRoleTool {
					role = "user"
				}
				messages = append(messages, map[string]any{"role": role, "content": blocks})
			}
		}
		body["messages"], body["system"] = messages, system
		if call.MaxOutputTokens != nil {
			body["max_tokens"] = *call.MaxOutputTokens
		}
		tools := []any{}
		for index, tool := range call.Tools {
			if value, ok := tool.(fantasy.FunctionTool); ok {
				declaration := map[string]any{"name": value.Name, "description": value.Description, "input_schema": value.InputSchema}
				if index == len(call.Tools)-1 {
					declaration["cache_control"] = map[string]any{"type": "ephemeral"}
				}
				tools = append(tools, declaration)
			}
		}
		if len(tools) > 0 {
			body["tools"] = tools
		}
		if options, ok := call.ProviderOptions[anthropic.Name].(*anthropic.ProviderOptions); ok {
			if options.Thinking != nil {
				body["thinking"] = map[string]any{"type": "enabled", "budget_tokens": options.Thinking.BudgetTokens}
			}
			if options.Effort != nil {
				body["output_config"] = map[string]any{"effort": string(*options.Effort)}
			}
			for key, value := range options.ExtraBody {
				body[key] = value
			}
		}
	case "responses":
		input := []any{}
		for _, message := range call.Prompt {
			for _, part := range message.Content {
				switch part.GetType() {
				case fantasy.ContentTypeText:
					value, _ := fantasy.AsMessagePart[fantasy.TextPart](part)
					if message.Role == fantasy.MessageRoleSystem {
						body["instructions"] = value.Text
						continue
					}
					kind := "input_text"
					if message.Role == fantasy.MessageRoleAssistant {
						kind = "output_text"
					}
					input = append(input, map[string]any{"type": "message", "role": string(message.Role), "content": []any{map[string]any{"type": kind, "text": value.Text}}})
				case fantasy.ContentTypeFile:
					value, _ := fantasy.AsMessagePart[fantasy.FilePart](part)
					content := map[string]any{"type": "input_image", "detail": "auto", "image_url": "data:" + value.MediaType + ";base64," + base64.StdEncoding.EncodeToString(value.Data)}
					if !strings.HasPrefix(value.MediaType, "image/") {
						content = map[string]any{"type": "input_file", "filename": value.Filename, "file_data": "data:" + value.MediaType + ";base64," + base64.StdEncoding.EncodeToString(value.Data)}
					}
					input = append(input, map[string]any{"type": "message", "role": string(message.Role), "content": []any{content}})
				case fantasy.ContentTypeReasoning:
					if metadata, ok := part.Options()[openai.Name].(*openai.ResponsesReasoningMetadata); ok && metadata.EncryptedContent != nil {
						summary := []any{}
						for _, text := range metadata.Summary {
							summary = append(summary, map[string]any{"type": "summary_text", "text": text})
						}
						input = append(input, map[string]any{"type": "reasoning", "id": metadata.ItemID, "encrypted_content": *metadata.EncryptedContent, "summary": summary})
					}
				case fantasy.ContentTypeToolCall:
					value, _ := fantasy.AsMessagePart[fantasy.ToolCallPart](part)
					input = append(input, map[string]any{"type": "function_call", "call_id": value.ToolCallID, "name": value.ToolName, "arguments": value.Input})
				case fantasy.ContentTypeToolResult:
					value, _ := fantasy.AsMessagePart[fantasy.ToolResultPart](part)
					input = append(input, map[string]any{"type": "function_call_output", "call_id": value.ToolCallID, "output": toolText(value.Output)})
				}
			}
		}
		body["input"], body["store"] = input, false
		if call.MaxOutputTokens != nil {
			body["max_output_tokens"] = *call.MaxOutputTokens
		}
		if options, ok := call.ProviderOptions[openai.Name].(*openai.ResponsesProviderOptions); ok {
			if options.Instructions != nil {
				body["instructions"] = *options.Instructions
			}
			if options.PromptCacheKey != nil {
				body["prompt_cache_key"] = *options.PromptCacheKey
			}
			if options.ServiceTier != nil {
				body["service_tier"] = string(*options.ServiceTier)
			}
			if options.ReasoningEffort != nil {
				body["reasoning"] = map[string]any{"effort": string(*options.ReasoningEffort)}
			}
			if options.Include != nil {
				body["include"] = options.Include
			}
		}
		body["tools"] = genericTools(call, true)
	case "google":
		delete(body, "model")
		delete(body, "stream")
		contents := []any{}
		for _, message := range call.Prompt {
			parts := []any{}
			var pending *google.ReasoningMetadata
			for _, part := range message.Content {
				var block map[string]any
				switch part.GetType() {
				case fantasy.ContentTypeText:
					value, _ := fantasy.AsMessagePart[fantasy.TextPart](part)
					block = map[string]any{"text": value.Text}
					if pending != nil && pending.ToolID == "" {
						block["thoughtSignature"] = base64.StdEncoding.EncodeToString([]byte(pending.Signature))
						pending = nil
					}
				case fantasy.ContentTypeFile:
					value, _ := fantasy.AsMessagePart[fantasy.FilePart](part)
					block = map[string]any{"inlineData": map[string]any{"mimeType": value.MediaType, "data": base64.StdEncoding.EncodeToString(value.Data)}}
				case fantasy.ContentTypeReasoning:
					value, _ := fantasy.AsMessagePart[fantasy.ReasoningPart](part)
					pending, _ = part.Options()[google.Name].(*google.ReasoningMetadata)
					if value.Text != "" {
						block = map[string]any{"text": value.Text, "thought": true}
						if pending != nil && pending.ToolID == "" {
							block["thoughtSignature"] = base64.StdEncoding.EncodeToString([]byte(pending.Signature))
							pending = nil
						}
					}
				case fantasy.ContentTypeToolCall:
					value, _ := fantasy.AsMessagePart[fantasy.ToolCallPart](part)
					var args any
					if err := json.Unmarshal([]byte(value.Input), &args); err != nil {
						return nil, err
					}
					block = map[string]any{"functionCall": map[string]any{"id": value.ToolCallID, "name": value.ToolName, "args": json.RawMessage(value.Input)}}
					if metadata, ok := part.Options()[google.Name].(*google.ReasoningMetadata); ok && (metadata.ToolID == "" || metadata.ToolID == value.ToolCallID) {
						block["thoughtSignature"] = base64.StdEncoding.EncodeToString([]byte(metadata.Signature))
					}
					if pending != nil && (pending.ToolID == "" || pending.ToolID == value.ToolCallID) {
						block["thoughtSignature"] = base64.StdEncoding.EncodeToString([]byte(pending.Signature))
						pending = nil
					}
				case fantasy.ContentTypeToolResult:
					value, _ := fantasy.AsMessagePart[fantasy.ToolResultPart](part)
					block = map[string]any{"functionResponse": map[string]any{"id": value.ToolCallID, "name": toolName(call.Prompt, value.ToolCallID), "response": map[string]any{"content": map[string]any{"text": toolText(value.Output)}}}}
				}
				if block != nil {
					parts = append(parts, block)
				}
			}
			if message.Role == fantasy.MessageRoleSystem {
				body["systemInstruction"] = map[string]any{"parts": parts}
			} else {
				role := "user"
				if message.Role == fantasy.MessageRoleAssistant {
					role = "model"
				}
				contents = append(contents, map[string]any{"role": role, "parts": parts})
			}
		}
		body["contents"] = contents
		generation := map[string]any{}
		if call.MaxOutputTokens != nil {
			generation["maxOutputTokens"] = *call.MaxOutputTokens
		}
		if call.Temperature != nil {
			generation["temperature"] = *call.Temperature
		}
		if options, ok := call.ProviderOptions[google.Name].(*google.ProviderOptions); ok {
			if options.CachedContent != "" {
				body["cachedContent"] = options.CachedContent
			}
			if options.ThinkingConfig != nil {
				thinking := map[string]any{}
				if options.ThinkingConfig.ThinkingBudget != nil {
					thinking["thinkingBudget"] = *options.ThinkingConfig.ThinkingBudget
				}
				if options.ThinkingConfig.ThinkingLevel != nil {
					thinking["thinkingLevel"] = strings.ToLower(*options.ThinkingConfig.ThinkingLevel)
				}
				if options.ThinkingConfig.IncludeThoughts != nil {
					thinking["includeThoughts"] = *options.ThinkingConfig.IncludeThoughts
				}
				generation["thinkingConfig"] = thinking
			}
		}
		body["generationConfig"] = generation
		declarations := []any{}
		for _, tool := range call.Tools {
			if value, ok := tool.(fantasy.FunctionTool); ok {
				declarations = append(declarations, map[string]any{"name": value.Name, "description": value.Description, "parametersJsonSchema": value.InputSchema})
			}
		}
		if len(declarations) > 0 {
			body["tools"] = []any{map[string]any{"functionDeclarations": declarations}}
		}
	default:
		messages := []any{}
		for _, message := range call.Prompt {
			wire := map[string]any{"role": string(message.Role)}
			content := []any{}
			tools := []any{}
			for _, part := range message.Content {
				switch part.GetType() {
				case fantasy.ContentTypeText:
					value, _ := fantasy.AsMessagePart[fantasy.TextPart](part)
					content = append(content, map[string]any{"type": "text", "text": value.Text})
				case fantasy.ContentTypeReasoning:
					value, _ := fantasy.AsMessagePart[fantasy.ReasoningPart](part)
					wire["reasoning_content"] = value.Text
				case fantasy.ContentTypeFile:
					value, _ := fantasy.AsMessagePart[fantasy.FilePart](part)
					content = append(content, map[string]any{"type": "image_url", "image_url": map[string]any{"url": "data:" + value.MediaType + ";base64," + base64.StdEncoding.EncodeToString(value.Data)}})
				case fantasy.ContentTypeToolCall:
					value, _ := fantasy.AsMessagePart[fantasy.ToolCallPart](part)
					tools = append(tools, map[string]any{"type": "function", "id": value.ToolCallID, "function": map[string]any{"name": value.ToolName, "arguments": value.Input}})
				case fantasy.ContentTypeToolResult:
					value, _ := fantasy.AsMessagePart[fantasy.ToolResultPart](part)
					messages = append(messages, map[string]any{"role": "tool", "tool_call_id": value.ToolCallID, "content": toolText(value.Output)})
				}
			}
			if len(content) == 1 && wireObject(content[0])["type"] == "text" {
				wire["content"] = wireObject(content[0])["text"]
			} else if len(content) > 0 {
				wire["content"] = content
			}
			if len(tools) > 0 {
				wire["tool_calls"] = tools
			}
			if message.Role != fantasy.MessageRoleTool {
				messages = append(messages, wire)
			}
		}
		body["messages"] = messages
		if call.MaxOutputTokens != nil {
			body["max_tokens"] = *call.MaxOutputTokens
		}
		body["tools"] = genericTools(call, false)
		body["stream_options"] = map[string]any{"include_usage": true}
	}
	if config.Kind != "google" && call.Temperature != nil {
		body["temperature"] = *call.Temperature
	}
	for key, value := range config.JSON {
		body[key] = value
	}
	if params, ok := call.ProviderOptions[parametersKey].(*callParameters); ok {
		if params.Developer {
			for _, message := range wireArray(body["messages"]) {
				if object := wireObject(message); object["role"] == "system" {
					object["role"] = "developer"
				}
			}
		}
		for key, value := range params.Body {
			if key == "generationConfig" {
				generation := wireObject(body[key])
				if generation == nil {
					generation = map[string]any{}
				}
				for field, setting := range wireObject(value) {
					generation[field] = setting
				}
				body[key] = generation
			} else {
				body[key] = value
			}
		}
		for _, key := range params.Remove {
			delete(body, key)
		}
	}
	for _, key := range config.Remove {
		delete(body, key)
	}
	if config.Kind == "anthropic" && len(wireArray(body["tools"])) > 0 && body["tool_choice"] == nil {
		body["tool_choice"] = map[string]any{"type": "auto"}
	}
	return body, nil
}

func genericTools(call fantasy.Call, responses bool) []any {
	tools := []any{}
	for _, tool := range call.Tools {
		value, ok := tool.(fantasy.FunctionTool)
		if !ok {
			continue
		}
		schemaJSON, _ := json.Marshal(value.InputSchema)
		schema := map[string]any{}
		json.Unmarshal(schemaJSON, &schema)
		function := map[string]any{"name": value.Name, "description": value.Description, "parameters": schema, "strict": false}
		if responses {
			function["type"] = "function"
			tools = append(tools, function)
		} else {
			// The wire encoder adapts the chat schema from its declaration bytes.
			tools = append(tools, map[string]any{"type": "function", "function": function})
		}
	}
	return tools
}

func toolText(output fantasy.ToolResultOutputContent) string {
	switch value := output.(type) {
	case fantasy.ToolResultOutputContentText:
		return value.Text
	case fantasy.ToolResultOutputContentError:
		if value.Error != nil {
			return value.Error.Error()
		}
	}
	return ""
}

func toolName(messages fantasy.Prompt, id string) string {
	for index := len(messages) - 1; index >= 0; index-- {
		for _, part := range messages[index].Content {
			if value, ok := fantasy.AsMessagePart[fantasy.ToolCallPart](part); ok && value.ToolCallID == id {
				return value.ToolName
			}
		}
	}
	return fmt.Sprintf("%s", id)
}
