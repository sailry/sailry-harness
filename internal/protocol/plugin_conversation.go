// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type PluginConversationDeclaration struct {
	ID       string                     `json:"id"`
	Resource PluginConversationResource `json:"resource"`
	Tools    []PluginConversationTool   `json:"tools"`
	Context  string                     `json:"context"`
}

func DefaultPluginConversationDeclaration() PluginConversationDeclaration {
	return PluginConversationDeclaration{
		ID:       "",
		Resource: PluginConversationResource(""),
		Tools:    []PluginConversationTool{},
		Context:  "",
	}
}
func (value *PluginConversationDeclaration) UnmarshalJSON(data []byte) error {
	type plain PluginConversationDeclaration
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "resource", "tools", "context"}, true); err != nil {
		return err
	}
	*value = PluginConversationDeclaration(decoded)
	return nil
}
func (value PluginConversationDeclaration) MarshalJSON() ([]byte, error) {
	type plain PluginConversationDeclaration
	encoded := plain(value)
	encoded.Tools = normalizeCollections(encoded.Tools)
	return json.Marshal(encoded)
}

type PluginConversationResource string

const (
	PluginConversationResourcePlugin    PluginConversationResource = "plugin"
	PluginConversationResourceWorkspace PluginConversationResource = "workspace"
	PluginConversationResourceDatabase  PluginConversationResource = "database"
	PluginConversationResourceSsh       PluginConversationResource = "ssh"
)

func (value PluginConversationResource) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginConversationResourcePlugin, PluginConversationResourceWorkspace, PluginConversationResourceDatabase, PluginConversationResourceSsh:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginConversationResource %q", value)
	}
}
func (value *PluginConversationResource) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginConversationResource(decoded) {
	case PluginConversationResourcePlugin, PluginConversationResourceWorkspace, PluginConversationResourceDatabase, PluginConversationResourceSsh:
		*value = PluginConversationResource(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginConversationResource %q", decoded)
	}
}
func (value PluginConversationResource) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginConversationResource) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginConversationTool struct {
	Kind string
	Data any
}
type PluginConversationToolBuiltinPayload struct {
	Name string `json:"name"`
}

func DefaultPluginConversationToolBuiltinPayload() PluginConversationToolBuiltinPayload {
	return PluginConversationToolBuiltinPayload{
		Name: "",
	}
}
func (value *PluginConversationToolBuiltinPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConversationToolBuiltinPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, true); err != nil {
		return err
	}
	*value = PluginConversationToolBuiltinPayload(decoded)
	return nil
}
func (value PluginConversationToolBuiltinPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConversationToolBuiltinPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConversationToolPluginPayload struct {
	Name   string  `json:"name"`
	Server *string `json:"server"`
}

func DefaultPluginConversationToolPluginPayload() PluginConversationToolPluginPayload {
	return PluginConversationToolPluginPayload{
		Name:   "",
		Server: nil,
	}
}
func (value *PluginConversationToolPluginPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConversationToolPluginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, true); err != nil {
		return err
	}
	*value = PluginConversationToolPluginPayload(decoded)
	return nil
}
func (value PluginConversationToolPluginPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConversationToolPluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConversationToolPackagePayload struct {
	Package string `json:"package"`
	Name    string `json:"name"`
}

func DefaultPluginConversationToolPackagePayload() PluginConversationToolPackagePayload {
	return PluginConversationToolPackagePayload{
		Package: "",
		Name:    "",
	}
}
func (value *PluginConversationToolPackagePayload) UnmarshalJSON(data []byte) error {
	type plain PluginConversationToolPackagePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "name"}, true); err != nil {
		return err
	}
	*value = PluginConversationToolPackagePayload(decoded)
	return nil
}
func (value PluginConversationToolPackagePayload) MarshalJSON() ([]byte, error) {
	type plain PluginConversationToolPackagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginConversationToolVariants = variantFactories{"builtin": func() any { return new(PluginConversationToolBuiltinPayload) },
	"plugin":  func() any { return new(PluginConversationToolPluginPayload) },
	"package": func() any { return new(PluginConversationToolPackagePayload) }}

func (value PluginConversationTool) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginConversationToolVariants)
}
func (value *PluginConversationTool) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginConversationToolVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginConversationBinding struct {
	Package PluginReference `json:"package"`
	ID      string          `json:"id"`
}

func DefaultPluginConversationBinding() PluginConversationBinding {
	return PluginConversationBinding{
		Package: DefaultPluginReference(),
		ID:      "",
	}
}
func (value *PluginConversationBinding) UnmarshalJSON(data []byte) error {
	type plain PluginConversationBinding
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "id"}, true); err != nil {
		return err
	}
	*value = PluginConversationBinding(decoded)
	return nil
}
func (value PluginConversationBinding) MarshalJSON() ([]byte, error) {
	type plain PluginConversationBinding
	encoded := plain(value)
	return json.Marshal(encoded)
}
