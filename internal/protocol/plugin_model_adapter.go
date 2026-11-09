// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type PluginModelAdapterBinding struct {
	Adapter      string                         `json:"adapter"`
	Capabilities PluginModelAdapterCapabilities `json:"capabilities"`
}

func DefaultPluginModelAdapterBinding() PluginModelAdapterBinding {
	return PluginModelAdapterBinding{
		Adapter:      "",
		Capabilities: DefaultPluginModelAdapterCapabilities(),
	}
}
func (value *PluginModelAdapterBinding) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterBinding
	decoded := plain{}
	decoded.Capabilities = DefaultPluginModelAdapterCapabilities()
	if err := DecodeObject(data, &decoded, []string{"adapter"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterBinding(decoded)
	return nil
}
func (value PluginModelAdapterBinding) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterBinding
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAdapterScript struct {
	Entry     string   `json:"entry"`
	Resources []string `json:"resources"`
}

func DefaultPluginModelAdapterScript() PluginModelAdapterScript {
	return PluginModelAdapterScript{
		Entry:     "",
		Resources: []string{},
	}
}
func (value *PluginModelAdapterScript) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterScript
	decoded := plain{}
	decoded.Resources = []string{}
	if err := DecodeObject(data, &decoded, []string{"entry"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterScript(decoded)
	return nil
}
func (value PluginModelAdapterScript) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterScript
	encoded := plain(value)
	encoded.Resources = normalizeCollections(encoded.Resources)
	return json.Marshal(encoded)
}

type PluginModelAdapterDeclaration struct {
	ID            string                         `json:"id"`
	Script        PluginModelAdapterScript       `json:"script"`
	Origins       []string                       `json:"origins"`
	Framing       PluginModelAdapterFraming      `json:"framing"`
	SchemaDialect PluginModelAdapterDialect      `json:"schema_dialect"`
	Capabilities  PluginModelAdapterCapabilities `json:"capabilities"`
	Discovery     bool                           `json:"discovery"`
}

func DefaultPluginModelAdapterDeclaration() PluginModelAdapterDeclaration {
	return PluginModelAdapterDeclaration{
		ID:            "",
		Script:        DefaultPluginModelAdapterScript(),
		Origins:       []string{},
		Framing:       PluginModelAdapterFraming(""),
		SchemaDialect: PluginModelAdapterDialectGeneric,
		Capabilities:  DefaultPluginModelAdapterCapabilities(),
		Discovery:     false,
	}
}
func (value *PluginModelAdapterDeclaration) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterDeclaration
	decoded := plain{}
	decoded.SchemaDialect = PluginModelAdapterDialectGeneric
	decoded.Capabilities = DefaultPluginModelAdapterCapabilities()
	decoded.Discovery = false
	if err := DecodeObject(data, &decoded, []string{"id", "script", "origins", "framing"}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterDeclaration(decoded)
	return nil
}
func (value PluginModelAdapterDeclaration) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterDeclaration
	encoded := plain(value)
	encoded.Origins = normalizeCollections(encoded.Origins)
	return json.Marshal(encoded)
}

type PluginModelAdapterFraming string

const (
	PluginModelAdapterFramingSse    PluginModelAdapterFraming = "sse"
	PluginModelAdapterFramingNdjson PluginModelAdapterFraming = "ndjson"
	PluginModelAdapterFramingJson   PluginModelAdapterFraming = "json"
)

func (value PluginModelAdapterFraming) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginModelAdapterFramingSse, PluginModelAdapterFramingNdjson, PluginModelAdapterFramingJson:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginModelAdapterFraming %q", value)
	}
}
func (value *PluginModelAdapterFraming) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginModelAdapterFraming(decoded) {
	case PluginModelAdapterFramingSse, PluginModelAdapterFramingNdjson, PluginModelAdapterFramingJson:
		*value = PluginModelAdapterFraming(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginModelAdapterFraming %q", decoded)
	}
}
func (value PluginModelAdapterFraming) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginModelAdapterFraming) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginModelAdapterDialect string

const (
	PluginModelAdapterDialectGeneric PluginModelAdapterDialect = "generic"
	PluginModelAdapterDialectOpenAi  PluginModelAdapterDialect = "open_ai"
	PluginModelAdapterDialectGemini  PluginModelAdapterDialect = "gemini"
)

func (value PluginModelAdapterDialect) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginModelAdapterDialectGeneric, PluginModelAdapterDialectOpenAi, PluginModelAdapterDialectGemini:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginModelAdapterDialect %q", value)
	}
}
func (value *PluginModelAdapterDialect) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginModelAdapterDialect(decoded) {
	case PluginModelAdapterDialectGeneric, PluginModelAdapterDialectOpenAi, PluginModelAdapterDialectGemini:
		*value = PluginModelAdapterDialect(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginModelAdapterDialect %q", decoded)
	}
}
func (value PluginModelAdapterDialect) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginModelAdapterDialect) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginModelAdapterCapabilities struct {
	Vision    bool `json:"vision"`
	Files     bool `json:"files"`
	Tools     bool `json:"tools"`
	Reasoning bool `json:"reasoning"`
}

func DefaultPluginModelAdapterCapabilities() PluginModelAdapterCapabilities {
	return PluginModelAdapterCapabilities{
		Vision:    false,
		Files:     false,
		Tools:     false,
		Reasoning: false,
	}
}
func (value *PluginModelAdapterCapabilities) UnmarshalJSON(data []byte) error {
	type plain PluginModelAdapterCapabilities
	decoded := plain{}
	decoded.Vision = false
	decoded.Files = false
	decoded.Tools = false
	decoded.Reasoning = false
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginModelAdapterCapabilities(decoded)
	return nil
}
func (value PluginModelAdapterCapabilities) MarshalJSON() ([]byte, error) {
	type plain PluginModelAdapterCapabilities
	encoded := plain(value)
	return json.Marshal(encoded)
}
