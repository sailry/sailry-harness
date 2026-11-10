package model

import (
	"encoding/base64"
	"encoding/json"
	"maps"
	"net/url"
	"slices"
	"sort"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const pluginRequestKey = "sailry.plugin.request"

type pluginCallRequest struct{ Body json.RawMessage }

func (*pluginCallRequest) Options() {}

func (value pluginCallRequest) MarshalJSON() ([]byte, error) {
	type plain pluginCallRequest
	return json.Marshal(plain(value))
}

func (value *pluginCallRequest) UnmarshalJSON(raw []byte) error {
	type plain pluginCallRequest
	return json.Unmarshal(raw, (*plain)(value))
}

func BuildPluginCall(binding Binding, request Request) (fantasy.Call, error) {
	input, err := pluginRequest(binding, request)
	if err != nil {
		return fantasy.Call{}, err
	}
	raw, err := json.Marshal(input)
	if err != nil {
		return fantasy.Call{}, &Failure{Kind: InvalidRequest, Message: "invalid model adapter request"}
	}
	call := fantasy.Call{MaxOutputTokens: request.MaxOutputTokens, Temperature: request.Temperature, Headers: maps.Clone(request.Headers),
		ProviderOptions: fantasy.ProviderOptions{pluginRequestKey: &pluginCallRequest{Body: raw}}}
	for _, tool := range request.Tools {
		call.Tools = append(call.Tools, tool)
	}
	SetToolSchemas(&call, request.ToolSchemas)
	return call, nil
}

func (model *pluginModel) request(call fantasy.Call) (protocol.PluginModelAdapterWireRequest, error) {
	carrier, ok := call.ProviderOptions[pluginRequestKey].(*pluginCallRequest)
	if !ok || carrier == nil {
		return protocol.PluginModelAdapterWireRequest{}, &Failure{Kind: InvalidRequest, Message: "captured plugin model request is unavailable"}
	}
	var request protocol.PluginModelAdapterWireRequest
	if json.Unmarshal(carrier.Body, &request) != nil {
		return request, &Failure{Kind: InvalidRequest, Message: "captured plugin model request is invalid"}
	}
	capabilities := model.capture.Declaration.Capabilities
	if err := validatePluginCapabilities(request, capabilities); err != nil {
		return request, err
	}
	if model.capture.Declaration.SchemaDialect == protocol.PluginModelAdapterDialectOpenAi {
		names := map[string]bool{}
		for _, tool := range request.Tools {
			if names[tool.Name] {
				return request, &Failure{Kind: InvalidRequest, Message: "model tool schema is invalid"}
			}
			names[tool.Name] = true
		}
	}
	return request, nil
}

func pluginRequest(binding Binding, request Request) (protocol.PluginModelAdapterWireRequest, error) {
	input := protocol.DefaultPluginModelAdapterWireRequest()
	input.Version, input.Model, input.Stream, input.Session, input.Endpoint = "v1", binding.Config.Model, true, binding.Session.String(), binding.Provider.Endpoint
	input.Features = maps.Clone(binding.Config.Features)
	input.ToolChoice = protocol.PluginModelAdapterWireChoice{Kind: "auto"}
	input.Reasoning.Effort = binding.Config.Effort
	if input.Reasoning.Effort.Kind == "default" {
		if entry := metadata(binding); entry != nil {
			input.Reasoning.Effort = entry.DefaultEffort
		}
	}
	if PluginBound(binding.Provider) {
		input.Reasoning.Replay = (**binding.Provider.Binding).Adapter.Capabilities.Reasoning
	}
	input.Temperature = request.Temperature
	if request.MaxOutputTokens != nil {
		if *request.MaxOutputTokens < 0 {
			return input, &Failure{Kind: InvalidRequest, Message: "model output limit is invalid"}
		}
		value := uint64(*request.MaxOutputTokens)
		input.MaxOutputTokens = &value
	} else if limit := OutputLimit(binding, false); limit > 0 {
		value := uint64(limit)
		input.MaxOutputTokens = &value
	}
	if request.System != "" {
		value := request.System
		input.System = &value
	}
	for _, tool := range request.Tools {
		schema := slices.Clone(request.ToolSchemas[tool.Name])
		if len(schema) == 0 {
			var err error
			schema, err = json.Marshal(tool.InputSchema)
			if err != nil {
				return input, &Failure{Kind: InvalidRequest, Message: "model tool schema is invalid"}
			}
		}
		description := tool.Description
		input.Tools = append(input.Tools, protocol.PluginModelAdapterWireTool{Name: tool.Name, Description: &description, Parameters: schema})
	}
	if binding.Provider.Binding != nil && *binding.Provider.Binding != nil {
		for _, policy := range (**binding.Provider.Binding).Policies {
			if slices.Contains(policy.Models, binding.Config.Model) {
				for key, raw := range policy.Body {
					input.Options[key] = slices.Clone(raw)
				}
			}
		}
	}
	for _, key := range []string{"thinking_effort", "disable_prompt_cache", "cache_ttl", "emit_clear_thinking", "preserve_thinking_context", "preserve_unsigned_thinking", "thinking_budget", "budget_tokens", "enable_thinking", "sailry_effort"} {
		delete(input.Options, key)
	}
	if raw, exists := input.Options["top_p"]; exists {
		if json.Unmarshal(raw, &input.TopP) != nil {
			return input, &Failure{Kind: InvalidRequest, Message: "model request options are invalid"}
		}
		delete(input.Options, "top_p")
	}
	if raw, exists := input.Options["stop"]; exists {
		if json.Unmarshal(raw, &input.Stop) != nil {
			return input, &Failure{Kind: InvalidRequest, Message: "model request options are invalid"}
		}
		delete(input.Options, "stop")
	}
	records := pluginVisibleRecords(request.Records)
	calls := map[string]string{}
	for _, record := range records {
		record = output.Audience(record)
		message := record.MutableMessage()
		if message == nil {
			continue
		}
		role := "user"
		if message.Role == canonical.Assistant {
			role = "assistant"
		}
		parts := []protocol.PluginModelAdapterWirePart{}
		flush := func() {
			if len(parts) != 0 {
				var data any = &protocol.PluginModelAdapterWireMessageUserPayload{Parts: parts}
				if role == "assistant" {
					data = &protocol.PluginModelAdapterWireMessageAssistantPayload{Parts: parts}
				}
				input.Messages = append(input.Messages, protocol.PluginModelAdapterWireMessage{Kind: role, Data: data})
				parts = []protocol.PluginModelAdapterWirePart{}
			}
		}
		blocks := slices.Clone(message.Content)
		sort.SliceStable(blocks, func(before, after int) bool { return blocks[before].Index < blocks[after].Index })
		redacted := map[uint64][]string{}
		if keepReasoning(binding, record) {
			for _, block := range blocks {
				if content, ok := block.Data.(*canonical.RedactedThinking); ok {
					for index, data := range content.Data {
						before := message.PartCount
						if index < len(content.Before) && content.Before[index] != nil {
							before = *content.Before[index]
						}
						redacted[before] = append(redacted[before], data)
					}
				}
			}
		}
		appendRedacted := func(index uint64) {
			for _, replay := range redacted[index] {
				value := replay
				parts = append(parts, protocol.PluginModelAdapterWirePart{Kind: "reasoning", Data: &protocol.PluginModelAdapterWirePartReasoningPayload{Replay: &value}})
			}
			delete(redacted, index)
		}
		for _, block := range blocks {
			appendRedacted(block.Index)
			if record.Metadata.Input != nil && record.Metadata.Input.CapabilityPart != nil && *record.Metadata.Input.CapabilityPart == block.Index {
				continue
			}
			switch content := block.Data.(type) {
			case *canonical.TextContent:
				parts = append(parts, protocol.PluginModelAdapterWirePart{Kind: "text", Data: &protocol.PluginModelAdapterWirePartTextPayload{Text: content.Text}})
			case *canonical.Thinking:
				if !keepReasoning(binding, record) {
					continue
				}
				text := content.Text
				parts = append(parts, protocol.PluginModelAdapterWirePart{Kind: "reasoning", Data: &protocol.PluginModelAdapterWirePartReasoningPayload{Text: &text, Replay: content.Signature}})
			case *canonical.ToolRequest:
				if content.Error != nil {
					continue
				}
				calls[content.ID] = content.Name
				parts = append(parts, protocol.PluginModelAdapterWirePart{Kind: "tool_call", Data: &protocol.PluginModelAdapterWirePartToolCallPayload{ID: content.ID, Name: content.Name, Arguments: content.Arguments}})
			case *canonical.ToolResponse:
				flush()
				result, err := payloadText(content.Result, request.Assets)
				if err != nil {
					return input, err
				}
				raw := json.RawMessage(result)
				if !json.Valid(raw) {
					raw, _ = json.Marshal(result)
				}
				if content.Format == canonical.JSON {
					var value any
					_ = json.Unmarshal(raw, &value)
					raw, _ = json.Marshal(map[string]any{"content": []any{map[string]any{"type": "text", "text": result}}, "structuredContent": value, "isError": content.Status == canonical.ToolErrorStatus})
				}
				name := content.Name
				if captured := calls[content.ID]; captured != "" {
					name = captured
				}
				input.Messages = append(input.Messages, protocol.PluginModelAdapterWireMessage{Kind: "tool", Data: &protocol.PluginModelAdapterWireMessageToolPayload{
					CallID: content.ID, Name: name, IsError: content.Status == canonical.ToolErrorStatus,
					Parts: []protocol.PluginModelAdapterWirePart{{Kind: "json", Data: &protocol.PluginModelAdapterWirePartJsonPayload{Value: raw}}},
				}})
			case *canonical.AssetContent:
				if block.Kind == "image" {
					var image protocol.PluginModelAdapterWireImage
					address := &url.URL{}
					if content.Asset.URI != nil {
						address, _ = url.Parse(*content.Asset.URI)
					}
					if address != nil && address.Host != "" && (address.Scheme == "https" || address.Scheme == "http") {
						image = protocol.PluginModelAdapterWireImage{Kind: "url", Data: &protocol.PluginModelAdapterWireImageUrlPayload{Url: *content.Asset.URI}}
					} else {
						data, err := assetBytes(content.Asset, request.Assets)
						if err != nil {
							return input, err
						}
						image = protocol.PluginModelAdapterWireImage{Kind: "inline", Data: &protocol.PluginModelAdapterWireImageInlinePayload{DataBase64: base64.StdEncoding.EncodeToString(data)}}
					}
					parts = append(parts, protocol.PluginModelAdapterWirePart{Kind: "image", Data: &protocol.PluginModelAdapterWirePartImagePayload{MediaType: content.Asset.Mime, Data: image}})
				} else {
					data, err := assetBytes(content.Asset, request.Assets)
					if err != nil {
						return input, err
					}
					parts = append(parts, protocol.PluginModelAdapterWirePart{Kind: "file", Data: &protocol.PluginModelAdapterWirePartFilePayload{MediaType: content.Asset.Mime, DataBase64: base64.StdEncoding.EncodeToString(data)}})
				}
			case *canonical.FileContent:
				if _, err := base64.StdEncoding.DecodeString(content.File.URI); err != nil {
					return input, &Failure{Kind: InvalidRequest, Message: "file content was not resolved"}
				}
				mediaType := ""
				if content.File.Mime != nil {
					mediaType = *content.File.Mime
				}
				parts = append(parts, protocol.PluginModelAdapterWirePart{Kind: "file", Data: &protocol.PluginModelAdapterWirePartFilePayload{MediaType: mediaType, DataBase64: content.File.URI}})
			}
		}
		appendRedacted(message.PartCount)
		flush()
	}
	return input, nil
}

func pluginVisibleRecords(records []canonical.Record) []canonical.Record {
	visibility := map[string]bool{}
	boundary, summary := -1, -1
	for index, record := range records {
		if record.Kind == "operation" {
			if operation, ok := record.Data.(*canonical.Operation); ok && operation.Kind == "visibility" {
				if value, ok := operation.Data.(*canonical.Visibility); ok {
					visibility[value.Message] = value.AgentVisible
				}
			}
		}
		if record.Kind == "compaction" {
			if compaction, ok := record.Data.(*canonical.Compaction); ok {
				for earlier := 0; earlier < index; earlier++ {
					if records[earlier].ID == compaction.Boundary {
						boundary, summary = earlier, index
					}
				}
			}
		}
	}
	result := []canonical.Record{}
	for index, record := range records {
		if index > boundary && (record.Kind != "compaction" || index == summary) && agentVisible(record, visibility) {
			result = append(result, record)
		}
	}
	return result
}

func validatePluginCapabilities(request protocol.PluginModelAdapterWireRequest, capabilities protocol.PluginModelAdapterCapabilities) error {
	unsupported := func() error {
		return &Failure{Kind: InvalidRequest, Message: "model adapter does not support the requested capability"}
	}
	if !capabilities.Tools && len(request.Tools) > 0 || !capabilities.Reasoning && request.Reasoning.Effort.Kind != "default" && request.Reasoning.Effort.Kind != "none" {
		return unsupported()
	}
	for _, message := range request.Messages {
		var parts []protocol.PluginModelAdapterWirePart
		switch value := message.Data.(type) {
		case *protocol.PluginModelAdapterWireMessageUserPayload:
			parts = value.Parts
		case *protocol.PluginModelAdapterWireMessageAssistantPayload:
			parts = value.Parts
		case *protocol.PluginModelAdapterWireMessageToolPayload:
			if !capabilities.Tools {
				return unsupported()
			}
			parts = value.Parts
		}
		for _, part := range parts {
			if part.Kind == "reasoning" && !capabilities.Reasoning || part.Kind == "image" && !capabilities.Vision || part.Kind == "file" && !capabilities.Files || part.Kind == "tool_call" && !capabilities.Tools {
				return unsupported()
			}
		}
	}
	return nil
}
