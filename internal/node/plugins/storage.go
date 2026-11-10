package plugins

import (
	"bytes"
	"encoding/json"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const MaxValueBytes = 256 * 1024

func ValidateStorage(declaration protocol.PluginStorageDeclaration) error {
	if len(declaration.Collections) > 32 {
		return packageFault("too many plugin storage collections")
	}
	for index, collection := range declaration.Collections {
		if (collection.Key == nil) == (collection.Prefix == nil) || len(collection.Schema) > 64*1024 ||
			collection.Scope != protocol.PluginStorageScopeNode && collection.Scope != protocol.PluginStorageScopeConversation {
			return packageFault("invalid storage collection selector")
		}
		selector := collection.Key
		if selector == nil {
			selector = collection.Prefix
		}
		if selector == nil || *selector == "" || len(*selector) > 128 || strings.ContainsRune(*selector, 0) {
			return packageFault("invalid storage collection selector")
		}
		for _, other := range declaration.Collections[:index] {
			if other.Scope != collection.Scope {
				continue
			}
			otherSelector := other.Key
			if otherSelector == nil {
				otherSelector = other.Prefix
			}
			if collection.Key != nil && other.Key != nil && *selector == *otherSelector ||
				collection.Prefix != nil && strings.HasPrefix(*otherSelector, *selector) ||
				other.Prefix != nil && strings.HasPrefix(*selector, *otherSelector) {
				return packageFault("plugin storage collection selectors overlap")
			}
		}
		root, err := decodeValue(collection.Schema)
		if err != nil {
			return err
		}
		if err := inspectStorageSchema(root, root, "#", 0); err != nil {
			return err
		}
		if _, err := compileSchema(root); err != nil {
			return err
		}
	}
	return nil
}

func inspectStorageSchema(root, raw any, pointer string, depth int) error {
	if depth > 64 {
		return packageFault("storage schema exceeds its depth limit")
	}
	schema, ok := raw.(map[string]any)
	if !ok {
		return nil
	}
	if schema["additionalProperties"] == false || schema["unevaluatedProperties"] == false {
		return packageFault("closed storage objects cannot preserve unknown fields")
	}
	if dialect, exists := schema["$schema"]; exists && dialect != SettingsSchema {
		return packageFault("unsupported storage schema dialect")
	}
	if _, exists := schema["$dynamicRef"]; exists {
		return packageFault("dynamic storage references are unsupported")
	}
	if _, exists := schema["$id"]; exists && depth > 0 {
		return packageFault("nested storage schema identifiers are unsupported")
	}
	if reference, exists := schema["$ref"]; exists {
		value, ok := reference.(string)
		if !ok || value != "#" && !strings.HasPrefix(value, "#/") {
			return packageFault("storage schemas require local JSON pointer references")
		}
		if _, exists := jsonPointer(root, strings.TrimPrefix(value, "#")); !exists {
			return packageFault("storage schema reference is unavailable")
		}
	}
	if value, exists := schema["default"]; exists {
		compiler, err := compileSchemaAt(root, pointer)
		if err != nil || compiler.Validate(value) != nil {
			return packageFault("invalid storage schema default")
		}
	}
	for _, name := range []string{"properties", "patternProperties", "$defs", "dependentSchemas"} {
		children, _ := schema[name].(map[string]any)
		for key, child := range children {
			escaped := strings.ReplaceAll(strings.ReplaceAll(key, "~", "~0"), "/", "~1")
			if err := inspectStorageSchema(root, child, pointer+"/"+name+"/"+escaped, depth+1); err != nil {
				return err
			}
		}
	}
	for _, name := range []string{"items", "additionalProperties", "unevaluatedProperties", "contains", "not", "if", "then", "else", "propertyNames", "unevaluatedItems", "contentSchema"} {
		if child, exists := schema[name]; exists {
			if err := inspectStorageSchema(root, child, pointer+"/"+name, depth+1); err != nil {
				return err
			}
		}
	}
	for _, name := range []string{"prefixItems", "allOf", "anyOf", "oneOf"} {
		children, _ := schema[name].([]any)
		for index, child := range children {
			if err := inspectStorageSchema(root, child, pointer+"/"+name+"/"+strconv.Itoa(index), depth+1); err != nil {
				return err
			}
		}
	}
	return nil
}

func jsonPointer(value any, pointer string) (any, bool) {
	if pointer == "" {
		return value, true
	}
	if !strings.HasPrefix(pointer, "/") || len(pointer) > 8*1024 {
		return nil, false
	}
	for _, part := range strings.Split(pointer[1:], "/") {
		part = strings.ReplaceAll(strings.ReplaceAll(part, "~1", "/"), "~0", "~")
		switch current := value.(type) {
		case map[string]any:
			var exists bool
			value, exists = current[part]
			if !exists {
				return nil, false
			}
		case []any:
			index, err := strconv.Atoi(part)
			if err != nil || index < 0 || index >= len(current) {
				return nil, false
			}
			value = current[index]
		default:
			return nil, false
		}
	}
	return value, true
}

func StorageCollection(info protocol.PluginInfo, scope protocol.PluginStorageScope, key string) *protocol.PluginStorageCollection {
	if info.Extension != nil && info.Extension.Storage != nil {
		for _, collection := range info.Extension.Storage.Collections {
			if collection.Scope == scope && (collection.Key != nil && *collection.Key == key || collection.Prefix != nil && strings.HasPrefix(key, *collection.Prefix)) {
				return &collection
			}
		}
	}
	return nil
}

func NormalizeStorageValue(collection *protocol.PluginStorageCollection, previous, value json.RawMessage) (json.RawMessage, error) {
	if len(value) > MaxValueBytes || !json.Valid(value) {
		return nil, packageFault("plugin value exceeds its JSON size limit")
	}
	if collection == nil {
		return bytes.Clone(value), nil
	}
	root, err := decodeValue(collection.Schema)
	if err != nil {
		return nil, err
	}
	decoded, err := decodeValue(value)
	if err != nil {
		return nil, err
	}
	if len(previous) != 0 {
		before, err := decodeValue(previous)
		if err != nil {
			return nil, err
		}
		if err := preserveStorageFields(root, []any{root}, before, decoded, 0); err != nil {
			return nil, err
		}
	}
	if err := projectStorageDefaults(root, root, &decoded, 0, map[string]bool{}); err != nil {
		return nil, err
	}
	validator, err := compileSchema(root)
	if err != nil || validator.Validate(decoded) != nil {
		return nil, packageFault("plugin value does not match its storage schema")
	}
	data, err := json.Marshal(decoded)
	if len(data) > MaxValueBytes {
		return nil, packageFault("plugin value exceeds its JSON size limit")
	}
	return data, err
}

func ProjectStorageValue(collection *protocol.PluginStorageCollection, data json.RawMessage) (json.RawMessage, error) {
	if collection == nil {
		return bytes.Clone(data), nil
	}
	root, err := decodeValue(collection.Schema)
	if err != nil {
		return nil, err
	}
	value, err := decodeValue(data)
	if err != nil {
		return nil, err
	}
	if err := projectStorageDefaults(root, root, &value, 0, map[string]bool{}); err != nil {
		return nil, err
	}
	data, err = json.Marshal(value)
	if len(data) > MaxValueBytes {
		return nil, packageFault("plugin default projection exceeds its size limit")
	}
	return data, err
}

func projectStorageDefaults(root, raw any, value *any, depth int, visiting map[string]bool) error {
	data, err := json.Marshal(*value)
	if err != nil || len(data) > MaxValueBytes {
		return packageFault("plugin value exceeds its JSON size limit")
	}
	budget := MaxValueBytes - len(data)
	return projectStorageDefaultsWithin(root, raw, value, depth, visiting, &budget)
}

func projectStorageDefaultsWithin(root, raw any, value *any, depth int, visiting map[string]bool, budget *int) error {
	if depth > 64 {
		return packageFault("storage default projection exceeds its depth limit")
	}
	schema, ok := raw.(map[string]any)
	if !ok {
		return nil
	}
	if reference, ok := schema["$ref"].(string); ok && !visiting[reference] {
		target, exists := jsonPointer(root, strings.TrimPrefix(reference, "#"))
		if !exists {
			return packageFault("storage schema reference is unavailable")
		}
		visiting[reference] = true
		err := projectStorageDefaultsWithin(root, target, value, depth+1, visiting, budget)
		delete(visiting, reference)
		if err != nil {
			return err
		}
	}
	if object, ok := (*value).(map[string]any); ok {
		properties, _ := schema["properties"].(map[string]any)
		for name, rawField := range properties {
			field, _ := rawField.(map[string]any)
			current, exists := object[name]
			if !exists {
				fallback, hasDefault, err := storageDefault(root, field, map[string]bool{}, 0)
				if err != nil {
					return err
				}
				if hasDefault {
					data, err := json.Marshal(fallback)
					key, keyErr := json.Marshal(name)
					cost := len(data) + len(key) + 2
					if err != nil || keyErr != nil || cost > *budget {
						return packageFault("storage default exceeds its size limit")
					}
					*budget -= cost
					current, err = decodeValue(data)
					if err != nil {
						return err
					}
					exists = true
				}
			}
			if exists {
				if err := projectStorageDefaultsWithin(root, field, &current, depth+1, map[string]bool{}, budget); err != nil {
					return err
				}
				object[name] = current
			}
		}
	}
	if values, ok := (*value).([]any); ok {
		prefix, _ := schema["prefixItems"].([]any)
		for index := range values {
			itemSchema := schema["items"]
			if index < len(prefix) {
				itemSchema = prefix[index]
			}
			if err := projectStorageDefaultsWithin(root, itemSchema, &values[index], depth+1, map[string]bool{}, budget); err != nil {
				return err
			}
		}
	}
	return nil
}

func storageDefault(root, raw any, visiting map[string]bool, depth int) (any, bool, error) {
	if depth > 64 {
		return nil, false, packageFault("storage default reference exceeds its depth limit")
	}
	schema, _ := raw.(map[string]any)
	if fallback, exists := schema["default"]; exists {
		return fallback, true, nil
	}
	if reference, ok := schema["$ref"].(string); ok && !visiting[reference] {
		visiting[reference] = true
		if target, exists := jsonPointer(root, strings.TrimPrefix(reference, "#")); exists {
			return storageDefault(root, target, visiting, depth+1)
		}
	}
	return nil, false, nil
}

func storageObjectSchemas(root, raw any, depth int, visiting map[string]bool, nodes *[]map[string]any) error {
	if depth > 64 {
		return packageFault("storage normalization exceeds its depth limit")
	}
	schema, ok := raw.(map[string]any)
	if !ok {
		return nil
	}
	*nodes = append(*nodes, schema)
	if reference, ok := schema["$ref"].(string); ok && !visiting[reference] {
		visiting[reference] = true
		if target, exists := jsonPointer(root, strings.TrimPrefix(reference, "#")); exists {
			if err := storageObjectSchemas(root, target, depth+1, visiting, nodes); err != nil {
				return err
			}
		}
	}
	for _, keyword := range []string{"allOf", "anyOf", "oneOf"} {
		branches, _ := schema[keyword].([]any)
		for _, branch := range branches {
			if err := storageObjectSchemas(root, branch, depth+1, visiting, nodes); err != nil {
				return err
			}
		}
	}
	for _, keyword := range []string{"then", "else"} {
		if branch, exists := schema[keyword]; exists {
			if err := storageObjectSchemas(root, branch, depth+1, visiting, nodes); err != nil {
				return err
			}
		}
	}
	return nil
}

func preserveStorageFields(root any, schemas []any, previous, value any, depth int) error {
	if depth > 64 {
		return packageFault("storage normalization exceeds its depth limit")
	}
	before, beforeObject := previous.(map[string]any)
	after, afterObject := value.(map[string]any)
	if !beforeObject || !afterObject {
		return nil
	}
	nodes := []map[string]any{}
	for _, schema := range schemas {
		if err := storageObjectSchemas(root, schema, depth, map[string]bool{}, &nodes); err != nil {
			return err
		}
	}
	fields := map[string][]any{}
	dictionaries := []any{}
	declared, patterned := false, false
	for _, schema := range nodes {
		if properties, ok := schema["properties"].(map[string]any); ok {
			declared = true
			for name, field := range properties {
				fields[name] = append(fields[name], field)
			}
		}
		if _, exists := schema["patternProperties"]; exists {
			patterned = true
		}
		if dictionary, ok := schema["additionalProperties"].(map[string]any); ok {
			dictionaries = append(dictionaries, dictionary)
		}
	}
	for name, stored := range before {
		current, supplied := after[name]
		if schemas, known := fields[name]; known {
			if supplied {
				if err := preserveStorageFields(root, schemas, stored, current, depth+1); err != nil {
					return err
				}
			}
		} else if len(dictionaries) != 0 {
			if supplied {
				if err := preserveStorageFields(root, dictionaries, stored, current, depth+1); err != nil {
					return err
				}
			}
		} else if declared && !patterned && !supplied {
			after[name] = stored
		}
	}
	return nil
}

func ValidateStorageIndex(index protocol.PluginStorageIndex) error {
	data, err := json.Marshal(index)
	if err != nil || len(data) > 64*1024 || len(index.Tags) > 16 {
		return packageFault("plugin storage index exceeds its size limit")
	}
	seen := map[string]bool{}
	for _, tag := range index.Tags {
		if tag == "" || len(tag) > 128 || strings.ContainsRune(tag, 0) || seen[tag] {
			return packageFault("invalid plugin storage index tag")
		}
		seen[tag] = true
	}
	if index.Source != nil && (index.Source.Key == "" || len(index.Source.Key) > 128 || len(index.Source.Pointer) > 8*1024 ||
		index.Source.Pointer != "" && !strings.HasPrefix(index.Source.Pointer, "/")) {
		return packageFault("invalid plugin storage index source")
	}
	for _, field := range index.Fields {
		if strings.ContainsRune(field, 0) {
			return packageFault("plugin index field contains a null character")
		}
	}
	return nil
}
