// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type PluginSettingsSchema struct {
	Schema               string                         `json:"$schema"`
	Kind                 PluginSettingsObject           `json:"type"`
	Title                *string                        `json:"title,omitempty"`
	Description          *string                        `json:"description,omitempty"`
	Locales              map[string]map[string]string   `json:"x-sailry-locales,omitempty"`
	Tabs                 []PluginSettingsTab            `json:"x-sailry-tabs,omitempty"`
	Properties           map[string]PluginSettingsField `json:"properties"`
	Required             []string                       `json:"required"`
	AdditionalProperties bool                           `json:"additionalProperties"`
}

func DefaultPluginSettingsSchema() PluginSettingsSchema {
	return PluginSettingsSchema{
		Schema:               "",
		Kind:                 PluginSettingsObject(""),
		Title:                nil,
		Description:          nil,
		Locales:              map[string]map[string]string{},
		Tabs:                 []PluginSettingsTab{},
		Properties:           map[string]PluginSettingsField{},
		Required:             []string{},
		AdditionalProperties: false,
	}
}
func (value *PluginSettingsSchema) UnmarshalJSON(data []byte) error {
	type plain PluginSettingsSchema
	decoded := plain{}
	decoded.Locales = map[string]map[string]string{}
	decoded.Tabs = []PluginSettingsTab{}
	decoded.Required = []string{}
	if err := DecodeObject(data, &decoded, []string{"$schema", "type", "properties", "additionalProperties"}, true); err != nil {
		return err
	}
	*value = PluginSettingsSchema(decoded)
	return nil
}
func (value PluginSettingsSchema) MarshalJSON() ([]byte, error) {
	type plain PluginSettingsSchema
	encoded := plain(value)
	encoded.Locales = normalizeCollections(encoded.Locales)
	encoded.Tabs = normalizeCollections(encoded.Tabs)
	encoded.Properties = normalizeCollections(encoded.Properties)
	encoded.Required = normalizeCollections(encoded.Required)
	return json.Marshal(encoded)
}

type PluginSettingsTab struct {
	ID      string   `json:"id"`
	Title   string   `json:"title"`
	Fields  []string `json:"fields"`
	Enabled *string  `json:"enabled,omitempty"`
}

func DefaultPluginSettingsTab() PluginSettingsTab {
	return PluginSettingsTab{
		ID:      "",
		Title:   "",
		Fields:  []string{},
		Enabled: nil,
	}
}
func (value *PluginSettingsTab) UnmarshalJSON(data []byte) error {
	type plain PluginSettingsTab
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "title", "fields"}, true); err != nil {
		return err
	}
	*value = PluginSettingsTab(decoded)
	return nil
}
func (value PluginSettingsTab) MarshalJSON() ([]byte, error) {
	type plain PluginSettingsTab
	encoded := plain(value)
	encoded.Fields = normalizeCollections(encoded.Fields)
	return json.Marshal(encoded)
}

type PluginSettingsObject string

const (
	PluginSettingsObjectObject PluginSettingsObject = "object"
)

func (value PluginSettingsObject) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginSettingsObjectObject:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginSettingsObject %q", value)
	}
}
func (value *PluginSettingsObject) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginSettingsObject(decoded) {
	case PluginSettingsObjectObject:
		*value = PluginSettingsObject(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginSettingsObject %q", decoded)
	}
}
func (value PluginSettingsObject) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginSettingsObject) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginSettingsField struct {
	Kind             PluginSettingsKind     `json:"type"`
	Time             bool                   `json:"x-sailry-time,omitempty"`
	ChildConcurrency bool                   `json:"x-sailry-child-concurrency,omitempty"`
	Import           *string                `json:"x-sailry-import,omitempty"`
	Model            bool                   `json:"x-sailry-model,omitempty"`
	Effort           *string                `json:"x-sailry-model-effort,omitempty"`
	Title            *string                `json:"title,omitempty"`
	Description      *string                `json:"description,omitempty"`
	Default          *json.RawMessage       `json:"default,omitempty"`
	Choices          *[]json.RawMessage     `json:"enum,omitempty"`
	MinLength        *uint64                `json:"minLength,omitempty"`
	MaxLength        *uint64                `json:"maxLength,omitempty"`
	Pattern          *string                `json:"pattern,omitempty"`
	Minimum          *json.Number           `json:"minimum,omitempty"`
	Maximum          *json.Number           `json:"maximum,omitempty"`
	Secret           *PluginSettingsBinding `json:"x-sailry-secret,omitempty"`
}

func DefaultPluginSettingsField() PluginSettingsField {
	return PluginSettingsField{
		Kind:             PluginSettingsKind(""),
		Time:             false,
		ChildConcurrency: false,
		Import:           nil,
		Model:            false,
		Effort:           nil,
		Title:            nil,
		Description:      nil,
		Default:          nil,
		Choices:          nil,
		MinLength:        nil,
		MaxLength:        nil,
		Pattern:          nil,
		Minimum:          nil,
		Maximum:          nil,
		Secret:           nil,
	}
}
func (value *PluginSettingsField) UnmarshalJSON(data []byte) error {
	type plain PluginSettingsField
	decoded := plain{}
	decoded.Time = false
	decoded.ChildConcurrency = false
	decoded.Import = nil
	decoded.Model = false
	if err := DecodeObject(data, &decoded, []string{"type"}, true); err != nil {
		return err
	}
	*value = PluginSettingsField(decoded)
	return nil
}
func (value PluginSettingsField) MarshalJSON() ([]byte, error) {
	type plain PluginSettingsField
	encoded := plain(value)
	encoded.Choices = normalizeCollections(encoded.Choices)
	return json.Marshal(encoded)
}

type PluginSettingsKind string

const (
	PluginSettingsKindString  PluginSettingsKind = "string"
	PluginSettingsKindInteger PluginSettingsKind = "integer"
	PluginSettingsKindNumber  PluginSettingsKind = "number"
	PluginSettingsKindBoolean PluginSettingsKind = "boolean"
)

func (value PluginSettingsKind) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginSettingsKindString, PluginSettingsKindInteger, PluginSettingsKindNumber, PluginSettingsKindBoolean:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginSettingsKind %q", value)
	}
}
func (value *PluginSettingsKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginSettingsKind(decoded) {
	case PluginSettingsKindString, PluginSettingsKindInteger, PluginSettingsKindNumber, PluginSettingsKindBoolean:
		*value = PluginSettingsKind(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginSettingsKind %q", decoded)
	}
}
func (value PluginSettingsKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginSettingsKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginSettingsState struct {
	Package    PluginReference            `json:"package"`
	Values     map[string]json.RawMessage `json:"values"`
	Configured []string                   `json:"configured"`
	Keepable   []string                   `json:"keepable"`
	Ready      bool                       `json:"ready"`
}

func DefaultPluginSettingsState() PluginSettingsState {
	return PluginSettingsState{
		Package:    DefaultPluginReference(),
		Values:     map[string]json.RawMessage{},
		Configured: []string{},
		Keepable:   []string{},
		Ready:      false,
	}
}
func (value *PluginSettingsState) UnmarshalJSON(data []byte) error {
	type plain PluginSettingsState
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "values", "configured", "keepable", "ready"}, false); err != nil {
		return err
	}
	*value = PluginSettingsState(decoded)
	return nil
}
func (value PluginSettingsState) MarshalJSON() ([]byte, error) {
	type plain PluginSettingsState
	encoded := plain(value)
	encoded.Values = normalizeCollections(encoded.Values)
	encoded.Configured = normalizeCollections(encoded.Configured)
	encoded.Keepable = normalizeCollections(encoded.Keepable)
	return json.Marshal(encoded)
}

type PluginSettingsSecretUpdate struct {
	Kind string
	Data any
}

var pluginSettingsSecretUpdateVariants = variantFactories{"keep": nil,
	"replace": func() any { return new(Secret) },
	"clear":   nil}

func (value PluginSettingsSecretUpdate) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "value", value.Kind, value.Data, pluginSettingsSecretUpdateVariants)
}
func (value *PluginSettingsSecretUpdate) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "value", pluginSettingsSecretUpdateVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
