package model

import "encoding/json"

func responsesExplicitCache(options any) bool {
	if raw, ok := options.(json.RawMessage); ok {
		var object map[string]any
		return json.Unmarshal(raw, &object) == nil && object["mode"] == "explicit"
	}
	return wireObject(options)["mode"] == "explicit"
}

// Keep each eligible endpoint on replay. The service limits writes per request
// and can still look up older marked prefixes on continuations and forks.
func markResponsesCacheBreakpoints(input []any) {
	developerPrefix := true
	for _, value := range input {
		item := wireObject(value)
		developerPrefix = developerPrefix && item["type"] == "message" && item["role"] == "developer"
		var blocks []any
		switch item["type"] {
		case "message":
			if !developerPrefix && item["role"] != "user" {
				continue
			}
			blocks = wireArray(item["content"])
		case "function_call_output":
			if text, ok := item["output"].(string); ok {
				blocks = []any{map[string]any{"type": "input_text", "text": text}}
				item["output"] = blocks
			} else {
				blocks = wireArray(item["output"])
			}
		default:
			continue
		}
		if len(blocks) == 0 {
			continue
		}
		last := wireObject(blocks[len(blocks)-1])
		switch last["type"] {
		case "input_text", "input_image", "input_file":
			if _, marked := last["prompt_cache_breakpoint"]; !marked {
				last["prompt_cache_breakpoint"] = map[string]any{"mode": "explicit"}
			}
		}
	}
}
