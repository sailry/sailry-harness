// Chat schema adaptation derives from Goose 14741e9a through the reviewed
// Sailry adapter. Apache-2.0; see LICENSE.goose.
package model

import (
	"bytes"
	"encoding/json"
	"fmt"

	"charm.land/fantasy"
)

const toolSchemasKey = "sailry.tool_schemas"

type toolSchemas struct{ Values map[string]json.RawMessage }

func (*toolSchemas) Options() {}
func (value toolSchemas) MarshalJSON() ([]byte, error) {
	type plain toolSchemas
	return json.Marshal(plain(value))
}
func (value *toolSchemas) UnmarshalJSON(raw []byte) error {
	type plain toolSchemas
	return json.Unmarshal(raw, (*plain)(value))
}

// SetToolSchemas retains declaration order across Fantasy's map-only schema boundary.
func SetToolSchemas(call *fantasy.Call, schemas map[string]json.RawMessage) {
	if call.ProviderOptions == nil {
		call.ProviderOptions = fantasy.ProviderOptions{}
	}
	values := map[string]json.RawMessage{}
	for name, raw := range schemas {
		values[name] = append(json.RawMessage(nil), raw...)
	}
	call.ProviderOptions[toolSchemasKey] = &toolSchemas{Values: values}
}

func declarationSchema(call fantasy.Call, name string, schema any, chat bool) (json.RawMessage, error) {
	var raw json.RawMessage
	if source, ok := call.ProviderOptions[toolSchemasKey].(*toolSchemas); ok {
		raw = source.Values[name]
	}
	if len(raw) == 0 {
		var err error
		raw, err = json.Marshal(schema)
		if err != nil {
			return nil, err
		}
	}
	if chat {
		if len(bytes.TrimSpace(raw)) > 0 && bytes.TrimSpace(raw)[0] == '{' {
			fields, err := schemaMembers(raw)
			if err != nil {
				return nil, err
			}
			hasType := false
			for _, field := range fields {
				hasType = hasType || field.name == "type"
			}
			if !hasType {
				fields = append(fields, schemaMember{"type", json.RawMessage(`"object"`)})
			}
			raw = schemaJSON(fields)
		}
		return chatSchemaJSON(raw)
	}
	var compact bytes.Buffer
	if err := json.Compact(&compact, raw); err != nil {
		return nil, err
	}
	return compact.Bytes(), nil
}

// A schema member retains the declaration's lexical object order. This walker
// visits only the schema-bearing fields handled by the source chat adaptation;
// property names, extension keywords and literal defaults stay raw JSON.
type schemaMember struct {
	name string
	raw  json.RawMessage
}

func schemaMembers(raw []byte) ([]schemaMember, error) {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	token, err := decoder.Token()
	if err != nil || token != json.Delim('{') {
		return nil, fmt.Errorf("tool schema object is invalid")
	}
	var fields []schemaMember
	for decoder.More() {
		name, err := decoder.Token()
		if err != nil {
			return nil, err
		}
		var value json.RawMessage
		if err := decoder.Decode(&value); err != nil {
			return nil, err
		}
		fields = append(fields, schemaMember{name.(string), value})
	}
	_, err = decoder.Token()
	return fields, err
}
func schemaJSON(fields []schemaMember) []byte {
	var out bytes.Buffer
	out.WriteByte('{')
	for index, field := range fields {
		if index > 0 {
			out.WriteByte(',')
		}
		name, _ := json.Marshal(field.name)
		out.Write(name)
		out.WriteByte(':')
		var compact bytes.Buffer
		if err := json.Compact(&compact, field.raw); err != nil {
			panic(err)
		}
		out.Write(compact.Bytes())
	}
	out.WriteByte('}')
	return out.Bytes()
}
func chatSchemaJSON(raw []byte) (json.RawMessage, error) {
	if len(bytes.TrimSpace(raw)) == 0 || bytes.TrimSpace(raw)[0] != '{' {
		return append(json.RawMessage(nil), raw...), nil
	}
	fields, err := schemaMembers(raw)
	if err != nil {
		return nil, err
	}
	index := func(name string) int {
		for i := range fields {
			if fields[i].name == name {
				return i
			}
		}
		return -1
	}
	remove := func(i int) schemaMember {
		field := fields[i]
		fields[i] = fields[len(fields)-1]
		fields = fields[:len(fields)-1]
		return field
	}
	if index("anyOf") < 0 {
		if i := index("oneOf"); i >= 0 {
			field := remove(i)
			field.name = "anyOf"
			fields = append(fields, field)
		}
	}
	nullableType := false
	if i := index("type"); i >= 0 {
		var types []string
		if json.Unmarshal(fields[i].raw, &types) == nil {
			var nonnull []string
			for _, kind := range types {
				if kind != "null" {
					nonnull = append(nonnull, kind)
				}
			}
			if len(nonnull) == 1 {
				fields[i].raw, _ = json.Marshal(nonnull[0])
				nullableType = true
			}
		}
	}
	if i := index("anyOf"); !nullableType && i >= 0 {
		unionField := remove(i)
		var union []json.RawMessage
		replaced := false
		if json.Unmarshal(unionField.raw, &union) == nil && len(union) == 2 {
			for j, value := range union {
				var variant struct {
					Type string `json:"type"`
				}
				if json.Unmarshal(value, &variant) == nil && variant.Type == "null" && len(bytes.TrimSpace(union[1-j])) > 0 && bytes.TrimSpace(union[1-j])[0] == '{' {
					replacement, err := schemaMembers(union[1-j])
					if err != nil {
						return nil, err
					}
					for _, field := range replacement {
						if index(field.name) < 0 {
							fields = append(fields, field)
						}
					}
					replaced = true
					break
				}
			}
		}
		if !replaced {
			fields = append(fields, unionField)
		}
	}
	if i := index("type"); i >= 0 && string(fields[i].raw) == `"object"` {
		if index("properties") < 0 {
			fields = append(fields, schemaMember{"properties", json.RawMessage(`{}`)})
		}
		if index("required") < 0 {
			fields = append(fields, schemaMember{"required", json.RawMessage(`[]`)})
		}
	}
	for i := range fields {
		field := &fields[i]
		switch field.name {
		case "properties", "$defs", "definitions":
			children, err := schemaMembers(field.raw)
			if err != nil {
				return nil, err
			}
			for j := range children {
				children[j].raw, err = chatSchemaJSON(children[j].raw)
				if err != nil {
					return nil, err
				}
			}
			field.raw = schemaJSON(children)
		case "anyOf", "allOf", "prefixItems":
			var children []json.RawMessage
			if err := json.Unmarshal(field.raw, &children); err != nil {
				return nil, err
			}
			for j := range children {
				children[j], err = chatSchemaJSON(children[j])
				if err != nil {
					return nil, err
				}
			}
			field.raw, err = json.Marshal(children)
			if err != nil {
				return nil, err
			}
		case "items", "additionalProperties":
			field.raw, err = chatSchemaJSON(field.raw)
			if err != nil {
				return nil, err
			}
		}
	}
	return schemaJSON(fields), nil
}
