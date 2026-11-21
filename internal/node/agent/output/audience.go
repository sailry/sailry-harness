package output

import (
	"encoding/json"
	"slices"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

// Audience returns an independent record with user-only image and MCP content
// removed. Record membership and the latest compaction boundary stay with callers.
func Audience(record canonical.Record) canonical.Record {
	record = clone(record)
	filterAudience(&record, true)
	return record
}
func permitted(annotations any) bool {
	roles, ok := object(annotations)["audience"].([]any)
	if !ok {
		return true
	}
	for _, role := range roles {
		if role == "assistant" {
			return true
		}
	}
	return false
}
func filterAudience(record *canonical.Record, filterResults bool) {
	message := message(record)
	if message == nil {
		return
	}
	content := message.Content[:0]
	for _, block := range message.Content {
		if block.Kind == "image" {
			var image canonical.AssetContent
			raw, _ := json.Marshal(block.Data)
			if json.Unmarshal(raw, &image) == nil && len(image.Asset.Annotations) > 0 && !permitted(decode(image.Asset.Annotations)) {
				continue
			}
		}
		if filterResults {
			if tool := response(&block); tool != nil {
				slots := slotProjection{audience: filterResultAudience(tool)}
				if projectToolFailure(tool, false) {
					slots.clear, slots.clearAttachments = true, true
				}
				slots.apply(tool)
			}
		}
		content = append(content, block)
	}
	message.Content = content
}

func filterResultAudience(response *canonical.ToolResponse) (removed []int) {
	if response.Format != canonical.MCP {
		return
	}
	value := object(decode(response.Result.Value))
	if value != nil {
		parts := array(value["content"])
		kept := make([]any, 0, len(parts))
		for index, part := range parts {
			fields := object(part)
			switch fields["type"] {
			case "text", "image", "audio", "resource", "resource_link":
				if !permitted(fields["annotations"]) {
					removed = append(removed, index)
					continue
				}
			}
			kept = append(kept, part)
		}
		value["content"] = kept
		response.Result.Value = json.RawMessage(encoded(value))
	}
	return
}

// The cached edits describe positions only. Each projection applies them to the
// current record's slots so that fresh asset references are preserved.
type slotProjection struct {
	audience, bounded []int
	removedField      string
	clear             bool
	clearAttachments  bool
}

func (p slotProjection) apply(tool *canonical.ToolResponse) {
	if p.clearAttachments {
		tool.Attachments = nil
	}
	if p.clear {
		tool.Result.Assets = nil
		return
	}
	if len(tool.Result.Assets) == 0 {
		return
	}
	kept := tool.Result.Assets[:0]
	for _, slot := range tool.Result.Assets {
		if p.removedField != "" && (slot.Pointer == p.removedField || strings.HasPrefix(slot.Pointer, p.removedField+"/")) {
			continue
		}
		pointer, ok := p.contentPointer(slot.Pointer)
		if !ok {
			continue
		}
		slot.Pointer = pointer
		kept = append(kept, slot)
	}
	tool.Result.Assets = kept
}

func (p slotProjection) contentPointer(pointer string) (string, bool) {
	rest, ok := strings.CutPrefix(pointer, "/content/")
	if !ok || len(p.audience)+len(p.bounded) == 0 {
		return pointer, true
	}
	number, _, _ := strings.Cut(rest, "/")
	index, err := strconv.Atoi(number)
	if err != nil || index < 0 || strconv.Itoa(index) != number {
		return pointer, true
	}
	skipped, hidden := slices.BinarySearch(p.audience, index)
	trimmed, dropped := slices.BinarySearch(p.bounded, index-skipped)
	if hidden || dropped {
		return "", false
	}
	if skipped+trimmed == 0 {
		return pointer, true
	}
	return "/content/" + strconv.Itoa(index-skipped-trimmed) + rest[len(number):], true
}
