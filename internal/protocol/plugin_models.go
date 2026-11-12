// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type PluginModelsKind string

const (
	PluginModelsKindProvider PluginModelsKind = "provider"
)

func (value PluginModelsKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginModelsKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginModelsKind(decoded) {
	case PluginModelsKindProvider:
		*value = PluginModelsKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginModelsKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginModelsKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginModelsModel struct {
	Kind           PluginModelsKind      `json:"kind"`
	ID             string                `json:"id"`
	Provider       string                `json:"provider"`
	Model          string                `json:"model"`
	Api            *ConversationModelApi `json:"api,omitempty"`
	Authentication Authentication        `json:"authentication"`
	Default        bool                  `json:"default"`
	Reasoning      bool                  `json:"reasoning"`
	Efforts        []Effort              `json:"efforts"`
	DefaultEffort  Effort                `json:"default_effort"`
}

func DefaultPluginModelsModel() PluginModelsModel {
	return PluginModelsModel{
		Kind:           PluginModelsKind(""),
		ID:             "",
		Provider:       "",
		Model:          "",
		Api:            nil,
		Authentication: Authentication(""),
		Default:        false,
		Reasoning:      false,
		Efforts:        []Effort{},
		DefaultEffort:  Effort{Kind: "default"},
	}
}
func (value *PluginModelsModel) UnmarshalJSON(data []byte) error {
	type plain PluginModelsModel
	decoded := plain{}
	decoded.Api = nil
	if err := DecodeObject(data, &decoded, []string{"kind", "id", "provider", "model", "authentication", "default", "reasoning", "efforts", "default_effort"}, false); err != nil {
		return err
	}
	*value = PluginModelsModel(decoded)
	return nil
}
func (value PluginModelsModel) MarshalJSON() ([]byte, error) {
	type plain PluginModelsModel
	encoded := plain(value)
	encoded.Efforts = normalizeCollections(encoded.Efforts)
	return json.Marshal(encoded)
}

type PluginModelsCatalog struct {
	Models []PluginModelsModel `json:"models"`
}

func DefaultPluginModelsCatalog() PluginModelsCatalog {
	return PluginModelsCatalog{
		Models: []PluginModelsModel{},
	}
}
func (value *PluginModelsCatalog) UnmarshalJSON(data []byte) error {
	type plain PluginModelsCatalog
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"models"}, false); err != nil {
		return err
	}
	*value = PluginModelsCatalog(decoded)
	return nil
}
func (value PluginModelsCatalog) MarshalJSON() ([]byte, error) {
	type plain PluginModelsCatalog
	encoded := plain(value)
	encoded.Models = normalizeCollections(encoded.Models)
	return json.Marshal(encoded)
}
