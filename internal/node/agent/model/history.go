package model

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"unicode/utf8"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"charm.land/fantasy/providers/google"
	"charm.land/fantasy/providers/openai"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
)

func BuildCall(binding Binding, request Request) (fantasy.Call, error) {
	if PluginBound(binding.Provider) {
		return BuildPluginCall(binding, request)
	}
	call := fantasy.Call{MaxOutputTokens: request.MaxOutputTokens, Temperature: request.Temperature, Headers: request.Headers, ProviderOptions: fantasy.ProviderOptions{}}
	if call.MaxOutputTokens == nil {
		if limit := OutputLimit(binding, false); limit > 0 {
			call.MaxOutputTokens = &limit
		}
	}
	SetToolSchemas(&call, request.ToolSchemas)
	params := requestParameters(binding, &call)
	call.ProviderOptions[parametersKey] = params
	if request.System != "" {
		call.Prompt = append(call.Prompt, fantasy.Message{Role: fantasy.MessageRoleSystem, Content: []fantasy.MessagePart{fantasy.TextPart{Text: request.System, ProviderOptions: cacheOptions(binding)}}})
	}
	for _, tool := range request.Tools {
		call.Tools = append(call.Tools, tool)
	}
	visibility := map[string]bool{}
	for _, record := range request.Records {
		if record.Kind != "operation" {
			continue
		}
		var op *canonical.Operation
		switch v := record.Data.(type) {
		case *canonical.Operation:
			op = v
		case canonical.Operation:
			op = &v
		}
		if op != nil && op.Kind == "visibility" {
			switch v := op.Data.(type) {
			case *canonical.Visibility:
				visibility[v.Message] = v.AgentVisible
			case canonical.Visibility:
				visibility[v.Message] = v.AgentVisible
			}
		}
	}
	records := request.Records
	// The latest summary replaces only records through its captured boundary.
	boundary := -1
	summary := -1
	for i, record := range records {
		if record.Kind == "compaction" {
			var c *canonical.Compaction
			switch v := record.Data.(type) {
			case *canonical.Compaction:
				c = v
			case canonical.Compaction:
				c = &v
			}
			if c != nil {
				for n := 0; n < i; n++ {
					if records[n].ID == c.Boundary {
						boundary, summary = n, i
						break
					}
				}
			}
		}
	}
	active := -1
	for i := range records {
		record := records[i]
		if i <= boundary {
			continue
		}
		message := record.Message()
		if message != nil && message.Role == canonical.User && agentVisible(record, visibility) {
			for _, block := range message.Content {
				if block.Kind != "tool_response" {
					active = i
					break
				}
			}
		}
	}
	for i, record := range records {
		if i <= boundary || record.Kind == "compaction" && i != summary || !agentVisible(record, visibility) {
			continue
		}
		record = output.Audience(record)
		message := record.MutableMessage()
		if message == nil {
			continue
		}
		role := fantasy.MessageRoleUser
		if message.Role == canonical.Assistant {
			role = fantasy.MessageRoleAssistant
		}
		current := fantasy.Message{Role: role}
		flush := func() {
			if len(current.Content) > 0 {
				call.Prompt = append(call.Prompt, current)
				current = fantasy.Message{Role: role}
			}
		}
		keep := keepReasoning(binding, record)
		blocks := append([]canonical.Block(nil), message.Content...)
		sort.SliceStable(blocks, func(i, j int) bool { return blocks[i].Index < blocks[j].Index })
		redacted := map[uint64][]string{}
		if keep {
			for _, block := range blocks {
				if block.Kind == "redacted_thinking" {
					var value canonical.RedactedThinking
					if decodeBlock(block.Data, &value) != nil {
						continue
					}
					for i, data := range value.Data {
						before := message.PartCount
						if i < len(value.Before) && value.Before[i] != nil {
							before = *value.Before[i]
						}
						redacted[before] = append(redacted[before], data)
					}
				}
			}
		}
		addRedacted := func(index uint64) {
			for _, data := range redacted[index] {
				current.Content = append(current.Content, fantasy.ReasoningPart{ProviderOptions: fantasy.ProviderOptions{anthropic.Name: &anthropic.ReasoningOptionMetadata{RedactedData: data}}})
			}
			delete(redacted, index)
		}
		for _, block := range blocks {
			addRedacted(block.Index)
			if record.Metadata.Input != nil && record.Metadata.Input.CapabilityPart != nil && *record.Metadata.Input.CapabilityPart == block.Index {
				continue
			}
			switch block.Kind {
			case "text":
				var value canonical.TextContent
				if err := decodeBlock(block.Data, &value); err != nil {
					return call, err
				}
				current.Content = append(current.Content, fantasy.TextPart{Text: value.Text})
			case "thinking":
				if !keep {
					continue
				}
				var value canonical.Thinking
				if err := decodeBlock(block.Data, &value); err != nil {
					return call, err
				}
				part := fantasy.ReasoningPart{Text: value.Text, ProviderOptions: fantasy.ProviderOptions{}}
				if value.Signature != nil {
					switch binding.Provider.Api {
					case "anthropic":
						part.ProviderOptions[anthropic.Name] = &anthropic.ReasoningOptionMetadata{Signature: *value.Signature}
					case "responses":
						part.ProviderOptions[openai.Name] = &openai.ResponsesReasoningMetadata{EncryptedContent: value.Signature, Summary: []string{value.Text}}
					case "gemini":
						decoded, err := base64.StdEncoding.DecodeString(*value.Signature)
						if err != nil {
							return call, &Failure{Kind: InvalidRequest, Message: "stored thought signature is invalid"}
						}
						if active < 0 || i >= active {
							part.ProviderOptions[google.Name] = &google.ReasoningMetadata{Signature: string(decoded)}
						}
					}
				}
				current.Content = append(current.Content, part)
			case "tool_request":
				var value canonical.ToolRequest
				if err := decodeBlock(block.Data, &value); err != nil {
					return call, err
				}
				if value.Error != nil && value.Name == "" {
					value.Name = "unparseable_tool_call"
					value.Arguments = json.RawMessage(`{}`)
				}
				if value.ID == "" || value.Name == "" || !json.Valid(value.Arguments) {
					return call, &Failure{Kind: InvalidRequest, Message: "stored tool call is invalid"}
				}
				options := fantasy.ProviderOptions{}
				if keep && binding.Provider.Api == "gemini" && (active < 0 || i >= active) && value.Provider != nil {
					if raw := (*value.Provider)["thoughtSignature"]; len(raw) > 0 {
						var signature string
						if json.Unmarshal(raw, &signature) == nil {
							decoded, err := base64.StdEncoding.DecodeString(signature)
							if err != nil {
								return call, err
							}
							options[google.Name] = &google.ReasoningMetadata{Signature: string(decoded), ToolID: value.ID}
						}
					}
				}
				current.Content = append(current.Content, fantasy.ToolCallPart{ToolCallID: value.ID, ToolName: value.Name, Input: string(value.Arguments), ProviderOptions: options})
			case "tool_response":
				var value canonical.ToolResponse
				if err := decodeBlock(block.Data, &value); err != nil {
					return call, err
				}
				flush()
				text, err := responseText(value, request.Assets)
				if err != nil {
					return call, err
				}
				call.Prompt = append(call.Prompt, fantasy.Message{Role: fantasy.MessageRoleTool, Content: []fantasy.MessagePart{fantasy.ToolResultPart{ToolCallID: value.ID, Output: fantasy.ToolResultOutputContentText{Text: text}}}})
			case "image", "document":
				var value canonical.AssetContent
				if err := decodeBlock(block.Data, &value); err != nil {
					return call, err
				}
				data, err := assetBytes(value.Asset, request.Assets)
				if err != nil {
					return call, err
				}
				name := ""
				if value.Asset.URI != nil {
					name = *value.Asset.URI
				}
				current.Content = append(current.Content, fantasy.FilePart{Filename: name, MediaType: value.Asset.Mime, Data: data})
			case "file":
				var value canonical.FileContent
				if err := decodeBlock(block.Data, &value); err != nil {
					return call, err
				}
				mime := ""
				if value.File.Mime != nil {
					mime = *value.File.Mime
				}
				data, err := base64.StdEncoding.DecodeString(value.File.URI)
				if err != nil {
					return call, &Failure{Kind: InvalidRequest, Message: "file content was not resolved"}
				}
				current.Content = append(current.Content, fantasy.FilePart{MediaType: mime, Data: data})
			}
		}
		addRedacted(message.PartCount)
		flush()
	}
	if binding.Provider.Api == "anthropic" {
		remaining := 2
		for i := len(call.Prompt) - 1; i >= 0 && remaining > 0; i-- {
			if call.Prompt[i].Role == fantasy.MessageRoleUser || call.Prompt[i].Role == fantasy.MessageRoleTool {
				call.Prompt[i].ProviderOptions = cacheOptions(binding)
				remaining--
			}
		}
	}
	return call, nil
}
func decodeBlock(value any, target any) error {
	raw, err := json.Marshal(value)
	if err != nil {
		return err
	}
	return json.Unmarshal(raw, target)
}
func agentVisible(record canonical.Record, changes map[string]bool) bool {
	if visible, ok := changes[record.ID]; ok {
		return visible
	}
	return record.Metadata.Runtime == nil || record.Metadata.Runtime.AgentVisible
}
func keepReasoning(binding Binding, record canonical.Record) bool {
	if runtime := record.Metadata.Runtime; runtime != nil && runtime.Inference != nil {
		inference := runtime.Inference
		return inference.Provider == "sailry:"+binding.Provider.ID.String() && (inference.RequestedModel == binding.Config.Model || inference.ResolvedModel != nil && *inference.ResolvedModel == binding.Config.Model)
	}
	if execution := record.Metadata.Execution; execution != nil && execution.Source != nil {
		return execution.Source.ID == binding.Provider.ID && execution.Source.Model == binding.Config.Model
	}
	return false
}
func cacheOptions(binding Binding) fantasy.ProviderOptions {
	if binding.Provider.Api == "anthropic" {
		return fantasy.ProviderOptions{anthropic.Name: &anthropic.ProviderCacheControlOptions{CacheControl: anthropic.CacheControl{Type: "ephemeral"}}}
	}
	return nil
}
func assetBytes(asset canonical.AssetRef, assets map[string][]byte) ([]byte, error) {
	if data, ok := assets[asset.Digest]; ok {
		return data, nil
	}
	if asset.URI != nil && strings.HasPrefix(*asset.URI, "data:") {
		if at := strings.Index(*asset.URI, ","); at >= 0 {
			return base64.StdEncoding.DecodeString((*asset.URI)[at+1:])
		}
	}
	return nil, &Failure{Kind: InvalidRequest, Message: "conversation asset was not loaded"}
}
func payloadText(payload canonical.Payload, assets map[string][]byte) (string, error) {
	if len(payload.Assets) == 0 {
		return string(payload.Value), nil
	}
	var value any
	if err := json.Unmarshal(payload.Value, &value); err != nil {
		return "", err
	}
	for _, slot := range payload.Assets {
		data, err := assetBytes(slot.Asset, assets)
		if err != nil {
			return "", err
		}
		var replacement any = data
		if slot.Encoding == canonical.Base64 {
			replacement = base64.StdEncoding.EncodeToString(data)
		} else {
			list := make([]uint8, len(data))
			copy(list, data)
			numbers := make([]any, len(list))
			for i, b := range list {
				numbers[i] = b
			}
			replacement = numbers
		}
		if err := replacePointer(value, slot.Pointer, replacement); err != nil {
			return "", err
		}
	}
	raw, err := json.Marshal(value)
	return string(raw), err
}
func replacePointer(value any, pointer string, replacement any) error {
	parts := strings.Split(strings.TrimPrefix(pointer, "/"), "/")
	for i, raw := range parts {
		part := strings.ReplaceAll(strings.ReplaceAll(raw, "~1", "/"), "~0", "~")
		switch object := value.(type) {
		case map[string]any:
			if i == len(parts)-1 {
				object[part] = replacement
				return nil
			}
			value = object[part]
		case []any:
			var index int
			if _, err := fmt.Sscan(part, &index); err != nil || index < 0 || index >= len(object) {
				return fmt.Errorf("stored payload asset pointer is invalid")
			}
			if i == len(parts)-1 {
				object[index] = replacement
				return nil
			}
			value = object[index]
		default:
			return fmt.Errorf("stored payload asset pointer is invalid")
		}
	}
	return nil
}

func responseText(response canonical.ToolResponse, assets map[string][]byte) (string, error) {
	raw, err := payloadText(response.Result, assets)
	if err != nil {
		return "", err
	}
	if response.Format == canonical.JSON {
		return raw, nil
	}
	var value map[string]json.RawMessage
	if err := json.Unmarshal([]byte(raw), &value); err != nil {
		return "", err
	}
	if response.Format == canonical.MCPError {
		var message string
		json.Unmarshal(value["message"], &message)
		var code any
		json.Unmarshal(value["code"], &code)
		failure := fmt.Sprintf("Mcp error: %v: %s", code, message)
		if data := value["data"]; len(data) > 0 && string(data) != "null" {
			failure += " data: " + string(data)
		}
		return "The tool call returned the following error:\n" + failure, nil
	}
	var parts []map[string]json.RawMessage
	if err := json.Unmarshal(value["content"], &parts); err != nil {
		return "", err
	}
	texts := []string{}
	for _, part := range parts {
		var kind string
		json.Unmarshal(part["type"], &kind)
		if kind == "text" {
			var text string
			json.Unmarshal(part["text"], &text)
			texts = append(texts, text)
		} else if kind == "resource" {
			var resource map[string]json.RawMessage
			json.Unmarshal(part["resource"], &resource)
			if resource["text"] != nil {
				var text string
				json.Unmarshal(resource["text"], &text)
				texts = append(texts, text)
			} else if resource["blob"] != nil {
				var blob, mime string
				json.Unmarshal(resource["blob"], &blob)
				json.Unmarshal(resource["mimeType"], &mime)
				data, err := base64.StdEncoding.DecodeString(blob)
				if err != nil {
					texts = append(texts, blob)
				} else if utf8.Valid(data) {
					texts = append(texts, string(data))
				} else {
					if mime == "" {
						mime = "application/octet-stream"
					}
					texts = append(texts, fmt.Sprintf("[Binary content (%s) - %d bytes]", mime, len(data)))
				}
			}
		}
	}
	if len(texts) == 0 {
		if structured := value["structuredContent"]; len(structured) > 0 {
			return string(structured), nil
		}
	}
	return strings.Join(texts, " "), nil
}
