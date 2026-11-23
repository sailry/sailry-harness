package model

import (
	"encoding/json"
	"fmt"
	"slices"
	"sort"
	"strings"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"charm.land/fantasy/providers/google"
	"charm.land/fantasy/providers/openai"
	"github.com/MoeForge/moego/base64"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type requestProjection uint8

const (
	rawHistory requestProjection = iota
	audienceProjected
	visibleProjected
)

// BuildProjectedCall consumes records filtered by output.Cache.Project and
// still selects which records are agent-visible.
func BuildProjectedCall(binding Binding, request Request) (fantasy.Call, error) {
	request.projection = audienceProjected
	return BuildCall(binding, request)
}

// BuildVisibleProjectedCall consumes records selected and paired by
// VisibleAnsweredRecords and audience-filtered by output.Cache.Project.
func BuildVisibleProjectedCall(binding Binding, request Request) (fantasy.Call, error) {
	request.projection = visibleProjected
	return BuildCall(binding, request)
}

func (request Request) visibleRecords() []canonical.Record {
	if request.projection == visibleProjected {
		return request.Records
	}
	return VisibleRecords(request.Records)
}

func (request Request) audienceRecord(record canonical.Record) canonical.Record {
	if request.projection != rawHistory {
		return record
	}
	return output.Audience(record)
}

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
	cache := cacheOptions(binding)
	if request.System != "" {
		call.Prompt = append(call.Prompt, fantasy.Message{Role: fantasy.MessageRoleSystem, Content: []fantasy.MessagePart{fantasy.TextPart{Text: request.System, ProviderOptions: cache}}})
	}
	for _, tool := range request.Tools {
		call.Tools = append(call.Tools, tool)
	}
	records := request.visibleRecords()
	origins := map[int]string{}
	appendBackground := func(text string) {
		if text != "" {
			call.Prompt = append(call.Prompt, fantasy.Message{Role: fantasy.MessageRoleAssistant, Content: []fantasy.MessagePart{fantasy.TextPart{Text: text}}})
		}
	}
	for _, record := range records {
		record = request.audienceRecord(record)
		message, background, err := record.ModelContent()
		if err != nil {
			return call, err
		}
		if message == nil {
			appendBackground(background)
			continue
		}
		role := fantasy.MessageRoleUser
		if message.Role == canonical.Assistant {
			role = fantasy.MessageRoleAssistant
		}
		current := fantasy.Message{Role: role}
		flush := func() {
			if len(current.Content) > 0 {
				if role == fantasy.MessageRoleAssistant {
					prefix := ""
					if runtime := record.Metadata.Runtime; runtime != nil && runtime.Inference != nil {
						prefix = runtime.Inference.ReplayPrefix
					}
					origins[len(call.Prompt)] = prefix
				}
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
						metadata := &openai.ResponsesReasoningMetadata{EncryptedContent: value.Signature}
						if value.Text != "" {
							metadata.Summary = []string{value.Text}
						}
						part.ProviderOptions[openai.Name] = metadata
					case "gemini":
						decoded, err := base64.StdEncoding.DecodeString(*value.Signature)
						if err != nil {
							return call, &Failure{Kind: InvalidRequest, Message: "stored thought signature is invalid"}
						}
						part.ProviderOptions[google.Name] = &google.ReasoningMetadata{Signature: string(decoded)}
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
				if keep && binding.Provider.Api == "gemini" && value.Provider != nil {
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
				text, media, err := responseContent(binding, value, request.Assets)
				if err != nil {
					return call, err
				}
				// A tool message is its result followed by the media it carries.
				parts := []fantasy.MessagePart{fantasy.ToolResultPart{ToolCallID: value.ID, Output: fantasy.ToolResultOutputContentText{Text: text}}}
				for _, file := range media {
					parts = append(parts, file)
				}
				call.Prompt = append(call.Prompt, fantasy.Message{Role: fantasy.MessageRoleTool, Content: parts})
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
		appendBackground(background)
	}
	if cache != nil {
		remaining := 2
		for i := len(call.Prompt) - 1; i >= 0 && remaining > 0; i-- {
			if call.Prompt[i].Role == fantasy.MessageRoleUser || call.Prompt[i].Role == fantasy.MessageRoleTool {
				call.Prompt[i].ProviderOptions = cache
				remaining--
				// Treat a parallel tool-response batch as one logical turn so
				// both breakpoints are not spent on the newest tool results.
				if call.Prompt[i].Role == fantasy.MessageRoleTool {
					for i > 0 && call.Prompt[i-1].Role == fantasy.MessageRoleTool {
						i--
					}
				}
			}
		}
	}
	if binding.Provider.Authentication == protocol.AuthenticationCopilot {
		call.Headers = copilotHeaders(request, records, call.Prompt)
	}
	prefix, err := scopeReplay(binding, &call, origins)
	if err != nil {
		return call, err
	}
	if prefix != "" {
		call.ProviderOptions[replayContextKey] = &replayContext{Prefix: prefix}
	}
	return call, nil
}

// copilotHeaders are what Copilot reads on each generation request: who
// started it, its own identifier and whether it carries an image. A request is
// the user's when its last message, turn context aside, is one the user wrote
// in a session that is not delegated; a tool's reply and a delegated run are
// the agent's.
func copilotHeaders(request Request, records []canonical.Record, prompt fantasy.Prompt) map[string]string {
	headers := map[string]string{}
	for name, value := range request.Headers {
		headers[name] = value
	}
	headers["openai-intent"], headers["x-initiator"], headers["x-request-id"] = "conversation-agent", "agent", protocol.NewRequestID().String()
	for index := len(records) - 1; index >= 0; index-- {
		record := records[index]
		if record.Metadata.Runtime != nil && record.Metadata.Runtime.TurnContext {
			continue
		}
		if value := record.Background(); value != nil {
			if value.Initial {
				break
			}
			continue
		}
		message, evidence, _ := record.ModelContent()
		if message == nil {
			if evidence != "" {
				break
			}
			continue
		}
		if message.Role == canonical.User && (record.Provenance.Author == "user" || record.Provenance.Author == "") && !request.Delegated && !slices.ContainsFunc(message.Content, func(block canonical.Block) bool { return block.Kind == "tool_response" }) {
			headers["x-initiator"] = "user"
		}
		break
	}
	for _, message := range prompt {
		for _, part := range message.Content {
			if file, ok := part.(fantasy.FilePart); ok && strings.HasPrefix(file.MediaType, "image/") {
				headers["copilot-vision-request"] = "true"
			}
		}
	}
	return headers
}

// VisibleRecords selects what a request carries without changing its messages:
// the latest summary followed by agent-visible records after its boundary.
// It also accepts an already projected record list.
func VisibleRecords(records []canonical.Record) []canonical.Record {
	visibility := map[string]bool{}
	boundary, summary := -1, -1
	boundaryID := ""
	for index, record := range records {
		var operation *canonical.Operation
		var compaction *canonical.Compaction
		switch value := record.Data.(type) {
		case *canonical.Operation:
			operation = value
		case canonical.Operation:
			operation = &value
		case *canonical.Compaction:
			compaction = value
		case canonical.Compaction:
			compaction = &value
		}
		if record.Kind == "operation" && operation != nil && operation.Kind == "visibility" {
			switch change := operation.Data.(type) {
			case *canonical.Visibility:
				visibility[change.Message] = change.AgentVisible
			case canonical.Visibility:
				visibility[change.Message] = change.AgentVisible
			}
		}
		if record.Kind == "compaction" && compaction != nil {
			summary, boundaryID = index, compaction.Boundary
		}
	}
	for index := 0; index < summary; index++ {
		if records[index].ID == boundaryID {
			boundary = index
			break
		}
	}
	result := []canonical.Record{}
	appendRecord := func(record canonical.Record) {
		if agentVisible(record, visibility) {
			if record.Metadata.Runtime != nil && !record.Metadata.Runtime.AgentVisible {
				runtime := *record.Metadata.Runtime
				runtime.AgentVisible = true
				record.Metadata.Runtime = &runtime
			}
			result = append(result, record)
		}
	}
	if summary >= 0 {
		appendRecord(records[summary])
	}
	for index, record := range records {
		if index > boundary && record.Kind != "compaction" {
			appendRecord(record)
		}
	}
	return result
}

// VisibleAnsweredRecords selects the visible history and repairs its tool pairs.
func VisibleAnsweredRecords(records []canonical.Record) []canonical.Record {
	visible := VisibleRecords(records)
	return repairToolPairs(visible, visible)
}

// Answered repairs tool pairs among visible records without removing records
// from the input. It drops a result with no preceding unanswered call and any
// call with no result, so interrupted turns cannot invalidate later requests.
func Answered(records []canonical.Record) []canonical.Record {
	return repairToolPairs(records, VisibleRecords(records))
}

func repairToolPairs(records, visible []canonical.Record) []canonical.Record {
	type position struct {
		record string
		block  int
	}
	pending := map[string][]position{}
	used := map[string]bool{}
	// An empty repair drops the unpaired block. A nonempty repair keeps the
	// pair but gives this occurrence a distinct provider-facing ID.
	repairs := map[string]map[int]string{}
	repair := func(where position, id string) {
		if repairs[where.record] == nil {
			repairs[where.record] = map[int]string{}
		}
		repairs[where.record][where.block] = id
	}
	for _, record := range visible {
		message := record.Message()
		if message == nil {
			continue
		}
		for index, block := range message.Content {
			where := position{record.ID, index}
			switch value := block.Data.(type) {
			case *canonical.ToolRequest:
				pending[value.ID] = append(pending[value.ID], where)
			case *canonical.ToolResponse:
				matches := pending[value.ID]
				if len(matches) == 0 {
					repair(where, "")
					continue
				}
				call := matches[0]
				pending[value.ID] = matches[1:]
				if used[value.ID] {
					id := canonical.ToolCallOccurrenceID(call.record, call.block)
					repair(call, id)
					repair(where, id)
				} else {
					used[value.ID] = true
				}
			}
		}
	}
	for _, matches := range pending {
		for _, call := range matches {
			repair(call, "")
		}
	}
	if len(repairs) == 0 {
		return records
	}
	repaired := slices.Clone(records)
	for index := range repaired {
		record := &repaired[index]
		changes := repairs[record.ID]
		if len(changes) == 0 {
			continue
		}
		message := record.Message()
		kept := *message
		kept.Content = make([]canonical.Block, 0, len(message.Content))
		for index, block := range message.Content {
			id, change := changes[index]
			if !change {
				kept.Content = append(kept.Content, block)
				continue
			}
			if id == "" {
				continue
			}
			switch value := block.Data.(type) {
			case *canonical.ToolRequest:
				updated := *value
				updated.ID, block.Data = id, &updated
			case *canonical.ToolResponse:
				updated := *value
				if updated.OriginalID == "" {
					updated.OriginalID = value.ID
				}
				updated.ID, block.Data = id, &updated
			}
			kept.Content = append(kept.Content, block)
		}
		record.Data = &kept
	}
	return repaired
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
	// Foundry can resolve an aliased deployment to native Claude Messages
	// after capture. Only the Messages serializer reads these options.
	switch binding.Provider.Api {
	case protocol.ConversationModelApiAnthropic, protocol.ConversationModelApiAzureAi:
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
