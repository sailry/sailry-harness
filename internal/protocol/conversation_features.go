// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ConversationFeaturesFeature struct {
	ID      string                              `json:"id"`
	Name    *string                             `json:"name,omitempty"`
	Icon    *PluginDesktopIcon                  `json:"icon,omitempty"`
	Options []ConversationFeaturesFeatureOption `json:"options"`
}

func DefaultConversationFeaturesFeature() ConversationFeaturesFeature {
	return ConversationFeaturesFeature{
		ID:      "",
		Name:    nil,
		Icon:    nil,
		Options: []ConversationFeaturesFeatureOption{},
	}
}
func (value *ConversationFeaturesFeature) UnmarshalJSON(data []byte) error {
	type plain ConversationFeaturesFeature
	decoded := plain{}
	decoded.Name = nil
	decoded.Icon = nil
	if err := DecodeObject(data, &decoded, []string{"id", "options"}, false); err != nil {
		return err
	}
	*value = ConversationFeaturesFeature(decoded)
	return nil
}
func (value ConversationFeaturesFeature) MarshalJSON() ([]byte, error) {
	type plain ConversationFeaturesFeature
	encoded := plain(value)
	encoded.Options = normalizeCollections(encoded.Options)
	return json.Marshal(encoded)
}

type ConversationFeaturesFeatureOption struct {
	ID   string             `json:"id"`
	Name *string            `json:"name,omitempty"`
	Icon *PluginDesktopIcon `json:"icon,omitempty"`
}

func DefaultConversationFeaturesFeatureOption() ConversationFeaturesFeatureOption {
	return ConversationFeaturesFeatureOption{
		ID:   "",
		Name: nil,
		Icon: nil,
	}
}
func (value *ConversationFeaturesFeatureOption) UnmarshalJSON(data []byte) error {
	type plain ConversationFeaturesFeatureOption
	decoded := plain{}
	decoded.Name = nil
	decoded.Icon = nil
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = ConversationFeaturesFeatureOption(decoded)
	return nil
}
func (value ConversationFeaturesFeatureOption) MarshalJSON() ([]byte, error) {
	type plain ConversationFeaturesFeatureOption
	encoded := plain(value)
	return json.Marshal(encoded)
}
