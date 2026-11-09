package output

import (
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

// Audience returns an independent record with user-only image and MCP content
// removed. Record membership and the latest compaction boundary stay with callers.
func Audience(record canonical.Record) canonical.Record {
	record = clone(record)
	audience(&record)
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
func audience(record *canonical.Record) {
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
		if response := response(&block); response != nil && response.Format == canonical.MCP {
			value := object(decode(response.Result.Value))
			if value != nil {
				parts := array(value["content"])
				kept := make([]any, 0, len(parts))
				for _, part := range parts {
					fields := object(part)
					switch fields["type"] {
					case "text", "image", "audio", "resource", "resource_link":
						if !permitted(fields["annotations"]) {
							continue
						}
					}
					kept = append(kept, part)
				}
				value["content"] = kept
				response.Result.Value = json.RawMessage(encoded(value))
			}
		}
		content = append(content, block)
	}
	message.Content = content
}
