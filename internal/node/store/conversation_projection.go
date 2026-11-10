package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type rawAgentTool struct {
	ID           *string
	Name         string
	Arguments    json.RawMessage
	Result       *json.RawMessage
	Format       canonical.ToolFormat
	Display      *protocol.ToolDisplay
	Presentation protocol.ToolPresentation
	Grouping     protocol.ToolGrouping
	Images       []protocol.ConversationImage
	Failure      *protocol.ToolFailure
	Error        *string
	Delegation   bool
}

func rawTool(record canonical.Record, block canonical.Block) *rawAgentTool {
	raw := &rawAgentTool{Arguments: json.RawMessage("null"), Presentation: protocol.ToolPresentationSummary, Grouping: protocol.ToolGroupingSequence, Images: []protocol.ConversationImage{}}
	switch value := block.Data.(type) {
	case *canonical.ToolRequest:
		raw.ID = &value.ID
		raw.Name = value.Name
		raw.Arguments = value.Arguments
	case *canonical.ToolResponse:
		raw.ID = &value.ID
		raw.Name = value.Name
		raw.Result = &value.Result.Value
		raw.Format = value.Format
	default:
		return nil
	}
	if p := record.Metadata.Presentation; p != nil {
		for _, name := range p.Delegations {
			if name == raw.Name {
				raw.Delegation = true
			}
		}
		if v, ok := p.Displays[raw.Name]; ok {
			raw.Display = &v
		}
		if v, ok := p.Tools[raw.Name]; ok {
			raw.Presentation = v
		}
		if v, ok := p.Groupings[raw.Name]; ok {
			raw.Grouping = v
		}
		for _, v := range p.Failures {
			if v.Index == block.Index && raw.ID != nil && v.ID == *raw.ID && v.Name == raw.Name {
				raw.Failure = &v.Failure
				if v.Message != nil {
					raw.Error = v.Message
				}
			}
		}
	}
	return raw
}
func (raw *rawAgentTool) revision() (string, error) {
	var result any
	if raw.Result != nil {
		result = agentJSON(*raw.Result)
	}
	body, err := encode([]any{raw.ID, raw.Name, agentJSON(raw.Arguments), result, raw.Display, raw.Presentation, raw.Grouping, raw.Images, raw.Failure, raw.Error})
	if err != nil {
		return "", err
	}
	sum := blake3.Sum256(body)
	return hex.EncodeToString(sum[:]), nil
}
func (raw *rawAgentTool) part() (protocol.ConversationPart, error) {
	summary, err := raw.summary()
	if err != nil {
		return protocol.ConversationPart{}, err
	}
	if raw.Result == nil {
		return protocol.ConversationPart{Kind: "tool_call", Data: &protocol.ConversationPartToolCallPayload{ID: raw.ID, Name: raw.Name, Summary: summary, Presentation: raw.Presentation, Grouping: raw.Grouping}}, nil
	}
	return protocol.ConversationPart{Kind: "tool_result", Data: &protocol.ConversationPartToolResultPayload{ID: raw.ID, Name: raw.Name, Summary: summary, Images: raw.Images}}, nil
}
func CanonicalEntry(record canonical.Record, sequence uint64) (protocol.ConversationEntry, error) {
	return canonicalEntry(record, sequence, nil)
}
func canonicalEntry(record canonical.Record, sequence uint64, resolve func(canonical.Block) (*rawAgentTool, error)) (protocol.ConversationEntry, error) {
	value := protocol.DefaultConversationEntry()
	value.Sequence = sequence
	value.ID = record.ID
	value.Turn = record.Turn
	value.Author = record.Provenance.Author
	value.Branch = record.Provenance.Branch
	value.TimestampMs = record.Created.UnixMilli()
	if record.Provenance.Author == "system" && (record.Kind == "configuration" || record.Metadata.Context != nil && record.Metadata.Context.Captured) {
		return value, nil
	}
	message := record.Message()
	if message != nil {
		skip := uint64(0)
		if input := record.Metadata.Input; input != nil {
			if len(input.References) > 0 {
				skip++
			}
			if input.NoticePart != nil {
				skip++
			}
		}
		for i := skip; i < message.PartCount; i++ {
			if input := record.Metadata.Input; input != nil && input.CapabilityPart != nil && i == *input.CapabilityPart {
				continue
			}
			var block *canonical.Block
			for index := range message.Content {
				if message.Content[index].Index == i {
					block = &message.Content[index]
					break
				}
			}
			if block == nil {
				value.Parts = append(value.Parts, resourcePart(map[string]any{"type": "provider_context"}))
				continue
			}
			var part protocol.ConversationPart
			switch v := block.Data.(type) {
			case *canonical.TextContent:
				part = protocol.ConversationPart{Kind: "text", Data: v.Text}
			case *canonical.Thinking:
				part = protocol.ConversationPart{Kind: "thinking", Data: v.Text}
			case *canonical.RedactedThinking:
				part = resourcePart(map[string]any{"type": "provider_context"})
			case *canonical.AssetContent:
				part = resourcePart(map[string]any{"type": block.Kind, "mime_type": v.Asset.Mime, "size": v.Asset.Size, "uri": v.Asset.URI})
			case *canonical.FileContent:
				part = resourcePart(map[string]any{"file_uri": v.File.URI, "mime_type": v.File.Mime})
			case *canonical.ToolRequest, *canonical.ToolResponse:
				raw := rawTool(record, *block)
				if resolve != nil {
					var err error
					raw, err = resolve(*block)
					if err != nil {
						return value, err
					}
				}
				var err error
				part, err = raw.part()
				if err != nil {
					return value, err
				}
			default:
				continue
			}
			value.Parts = append(value.Parts, part)
		}
		if message.Usage != nil {
			u := message.Usage
			value.Usage = &protocol.ConversationUsage{Input: optionalUint(u.Input), Output: optionalUint(u.Output), CachedInput: optionalUint(u.CacheRead), Reasoning: optionalUint(u.Reasoning)}
		}
		value.Citations = projectSources(message)
		value.SearchSuggestions = message.Sources.SearchSuggestions
	}
	if record.Kind == "compaction" {
		if c, ok := record.Data.(*canonical.Compaction); ok {
			text := []string{}
			for _, b := range c.Summary.Content {
				if t, ok := b.Data.(*canonical.TextContent); ok {
					text = append(text, t.Text)
				}
			}
			value.Parts = append(value.Parts, protocol.ConversationPart{Kind: "compaction", Data: strings.Join(text, "\n")})
		}
	}
	if record.Metadata.Input != nil {
		for _, ref := range record.Metadata.Input.References {
			value.Parts = append(value.Parts, protocol.ConversationPart{Kind: "reference", Data: ref})
		}
	}
	if p := record.Metadata.Presentation; p != nil {
		for _, failure := range p.Failures {
			value.Parts = append(value.Parts, resourcePart(map[string]any{"type": "tool_failed", "index": failure.Index, "id": failure.ID, "name": failure.Name, "failure": failure.Failure}))
		}
	}
	if c := record.Metadata.Compaction; c != nil && c.Failure != nil {
		body, _ := encode(c.Failure)
		var object map[string]json.RawMessage
		_ = json.Unmarshal(body, &object)
		object["type"] = json.RawMessage(`"compaction_failed"`)
		value.Parts = append(value.Parts, resourcePart(object))
	}
	if execution := record.Metadata.Execution; execution != nil {
		if execution.Retry != nil {
			body, _ := encode(execution.Retry)
			var object map[string]json.RawMessage
			_ = json.Unmarshal(body, &object)
			object["type"] = json.RawMessage(`"model_retry"`)
			value.Parts = append(value.Parts, resourcePart(object))
		}
		if execution.ToolStarted != nil {
			value.Parts = append(value.Parts, resourcePart(map[string]any{"type": "tool_started", "id": execution.ToolStarted.ID, "name": execution.ToolStarted.Name}))
		}
	}
	parts, err := reviewParts(record)
	if err != nil {
		return value, err
	}
	value.Parts = append(value.Parts, parts...)
	return value, nil
}
func optionalUint(v *uint64) uint64 {
	if v == nil {
		return 0
	}
	return *v
}
func resourcePart(v any) protocol.ConversationPart {
	b, _ := encode(v)
	return protocol.ConversationPart{Kind: "resource", Data: json.RawMessage(b)}
}
func projectStoredEntry(ctx context.Context, db *sql.Conn, record canonical.Record, sequence uint64) (protocol.ConversationEntry, error) {
	entry, err := canonicalEntry(record, sequence, func(block canonical.Block) (*rawAgentTool, error) {
		return storedAgentTool(ctx, db, record, sequence, block)
	})
	if err != nil {
		return entry, err
	}
	message := record.Message()
	if message == nil || len(entry.Parts) == 0 {
		return entry, nil
	}
	skip := uint64(0)
	if input := record.Metadata.Input; input != nil {
		if len(input.References) > 0 {
			skip++
		}
		if input.NoticePart != nil {
			skip++
		}
	}
	position := 0
	for i := skip; i < message.PartCount; i++ {
		if input := record.Metadata.Input; input != nil && input.CapabilityPart != nil && i == *input.CapabilityPart {
			continue
		}
		for _, block := range message.Content {
			if block.Index != i {
				continue
			}
			if file, ok := block.Data.(*canonical.FileContent); ok && strings.HasPrefix(file.File.URI, "sailry-attachment://") {
				id, err := protocol.ParseAttachmentID(strings.TrimPrefix(file.File.URI, "sailry-attachment://"))
				if err != nil {
					return entry, err
				}
				attachment, err := GetTurnAttachment(ctx, db, record.Turn, id)
				if err != nil {
					return entry, err
				}
				entry.Parts[position] = protocol.ConversationPart{Kind: "attachment", Data: attachment}
				continue
			}
			if block.Kind != "image" {
				continue
			}
			asset, ok := block.Data.(*canonical.AssetContent)
			if !ok || !supportedAgentImage(asset.Asset) {
				return entry, invalid("invalid image in canonical history")
			}
			run, e := ReadAgentRun(ctx, db, record.Turn)
			if e != nil {
				return entry, e
			}
			image, e := describeAgentImage(record.Turn, run.Worktree, record.ID, i, 0, asset.Asset)
			if e != nil {
				return entry, e
			}
			entry.Parts[position] = protocol.ConversationPart{Kind: "image", Data: &image}
		}
		position++
	}
	return entry, nil
}
