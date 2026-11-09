package model

// Chat schema adaptation follows Goose 14741e9a formats/openai.rs through
// Sailry's reviewed adapter. See LICENSE and the source manifest in README.md.
func chatSchema(value any) {
	object, ok := value.(map[string]any)
	if !ok {
		return
	}
	if _, exists := object["anyOf"]; !exists {
		if union, exists := object["oneOf"]; exists {
			delete(object, "oneOf")
			object["anyOf"] = union
		}
	}
	if types, ok := object["type"].([]any); ok {
		values := []any{}
		for _, kind := range types {
			if kind != "null" {
				values = append(values, kind)
			}
		}
		if len(values) == 1 {
			object["type"] = values[0]
		}
	}
	if union, ok := object["anyOf"].([]any); ok && len(union) == 2 {
		for index, variant := range union {
			if variant, ok := variant.(map[string]any); ok && variant["type"] == "null" {
				if replacement, ok := union[1-index].(map[string]any); ok {
					delete(object, "anyOf")
					for key, value := range replacement {
						if _, exists := object[key]; !exists {
							object[key] = value
						}
					}
					break
				}
			}
		}
	}
	if object["type"] == "object" {
		if _, exists := object["properties"]; !exists {
			object["properties"] = map[string]any{}
		}
		if _, exists := object["required"]; !exists {
			object["required"] = []any{}
		}
	}
	for _, key := range []string{"properties", "$defs", "definitions"} {
		if children, ok := object[key].(map[string]any); ok {
			for _, child := range children {
				chatSchema(child)
			}
		}
	}
	for _, key := range []string{"anyOf", "allOf", "prefixItems"} {
		if children, ok := object[key].([]any); ok {
			for _, child := range children {
				chatSchema(child)
			}
		}
	}
	chatSchema(object["items"])
	chatSchema(object["additionalProperties"])
}
