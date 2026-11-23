package model

import "fmt"

// API-key Messages uses the same client identity as its headers. Keep this
// separate from claudeTransport: key requests retain their authentication,
// system blocks, tool names and cache markers rather than taking OAuth policy.
func prepareAnthropicAPIKeyBody(config transportConfig, body map[string]any) error {
	if !config.AnthropicAPIKey {
		return nil
	}
	var blocks []any
	switch value := body["system"].(type) {
	case nil:
	case string:
		if value != "" {
			blocks = []any{map[string]any{"type": "text", "text": value}}
		}
	case []any:
		blocks = value
	default:
		return fmt.Errorf("Anthropic system content must be a string or block array")
	}
	if len(blocks) == 0 || wireObject(blocks[0])["text"] != claudeIdentity {
		blocks = append([]any{map[string]any{"type": "text", "text": claudeIdentity}}, blocks...)
	}
	body["system"] = blocks
	if body["metadata"] == nil && config.JSON["metadata"] != nil {
		body["metadata"] = config.JSON["metadata"]
	}
	return nil
}
