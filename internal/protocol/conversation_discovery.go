// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ConversationDiscoverySource struct {
	Kind string
	Data any
}
type ConversationDiscoverySourceSavedPayload struct {
	Provider         ProviderID                `json:"provider"`
	ExpectedRevision uint64                    `json:"expected_revision"`
	Oauth            *ConversationOauthOptions `json:"oauth,omitempty"`
}

func DefaultConversationDiscoverySourceSavedPayload() ConversationDiscoverySourceSavedPayload {
	return ConversationDiscoverySourceSavedPayload{
		Provider:         ProviderID{},
		ExpectedRevision: 0,
		Oauth:            nil,
	}
}
func (value *ConversationDiscoverySourceSavedPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationDiscoverySourceSavedPayload
	decoded := plain{}
	decoded.Oauth = nil
	if err := DecodeObject(data, &decoded, []string{"provider", "expected_revision"}, false); err != nil {
		return err
	}
	*value = ConversationDiscoverySourceSavedPayload(decoded)
	return nil
}
func (value ConversationDiscoverySourceSavedPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationDiscoverySourceSavedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationDiscoverySourceVariants = variantFactories{"draft": func() any { return new(ConversationDiscoveryDraft) },
	"saved": func() any { return new(ConversationDiscoverySourceSavedPayload) }}

func (value ConversationDiscoverySource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationDiscoverySourceVariants)
}
func (value *ConversationDiscoverySource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationDiscoverySourceVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationDiscoveryDraft struct {
	Provider   ProviderID                    `json:"provider"`
	Api        ConversationModelApi          `json:"api"`
	Preset     *ConversationPresetsPresetRef `json:"preset,omitempty"`
	Endpoint   string                        `json:"endpoint"`
	Credential *CredentialRef                `json:"credential"`
	Secret     *Secret                       `json:"secret"`
}

func DefaultConversationDiscoveryDraft() ConversationDiscoveryDraft {
	return ConversationDiscoveryDraft{
		Provider:   ProviderID{},
		Api:        ConversationModelApi(""),
		Preset:     nil,
		Endpoint:   "",
		Credential: nil,
		Secret:     nil,
	}
}
func (value *ConversationDiscoveryDraft) UnmarshalJSON(data []byte) error {
	type plain ConversationDiscoveryDraft
	decoded := plain{}
	decoded.Preset = nil
	if err := DecodeObject(data, &decoded, []string{"provider", "api", "endpoint"}, false); err != nil {
		return err
	}
	*value = ConversationDiscoveryDraft(decoded)
	return nil
}
func (value ConversationDiscoveryDraft) MarshalJSON() ([]byte, error) {
	type plain ConversationDiscoveryDraft
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationDiscoveryCatalog struct {
	Endpoint string                       `json:"endpoint"`
	Models   []ConversationDiscoveryModel `json:"models"`
}

func DefaultConversationDiscoveryCatalog() ConversationDiscoveryCatalog {
	return ConversationDiscoveryCatalog{
		Endpoint: "",
		Models:   []ConversationDiscoveryModel{},
	}
}
func (value *ConversationDiscoveryCatalog) UnmarshalJSON(data []byte) error {
	type plain ConversationDiscoveryCatalog
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"endpoint", "models"}, false); err != nil {
		return err
	}
	*value = ConversationDiscoveryCatalog(decoded)
	return nil
}
func (value ConversationDiscoveryCatalog) MarshalJSON() ([]byte, error) {
	type plain ConversationDiscoveryCatalog
	encoded := plain(value)
	encoded.Models = normalizeCollections(encoded.Models)
	return json.Marshal(encoded)
}

type ConversationDiscoveryModel struct {
	ID           string                             `json:"id"`
	DisplayName  *string                            `json:"display_name,omitempty"`
	Context      *uint32                            `json:"context"`
	Output       *uint32                            `json:"output"`
	Capabilities *ConversationDiscoveryCapabilities `json:"capabilities"`
}

func DefaultConversationDiscoveryModel() ConversationDiscoveryModel {
	return ConversationDiscoveryModel{
		ID:           "",
		DisplayName:  nil,
		Context:      nil,
		Output:       nil,
		Capabilities: nil,
	}
}
func (value *ConversationDiscoveryModel) UnmarshalJSON(data []byte) error {
	type plain ConversationDiscoveryModel
	decoded := plain{}
	decoded.DisplayName = nil
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = ConversationDiscoveryModel(decoded)
	return nil
}
func (value ConversationDiscoveryModel) MarshalJSON() ([]byte, error) {
	type plain ConversationDiscoveryModel
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationDiscoveryCapabilities struct {
	Features      []ConversationFeaturesFeature `json:"features,omitempty"`
	Vision        *bool                         `json:"vision"`
	Tools         *bool                         `json:"tools"`
	Reasoning     *bool                         `json:"reasoning"`
	Efforts       *[]Effort                     `json:"efforts"`
	DefaultEffort *Effort                       `json:"default_effort"`
	// Options are what a plugin adapter's discovery recorded about the model
	// for its own prepare, which receives them in its request options.
	Options map[string]json.RawMessage `json:"options,omitempty"`
}

func DefaultConversationDiscoveryCapabilities() ConversationDiscoveryCapabilities {
	return ConversationDiscoveryCapabilities{
		Features:      []ConversationFeaturesFeature{},
		Vision:        nil,
		Tools:         nil,
		Reasoning:     nil,
		Efforts:       nil,
		DefaultEffort: nil,
		Options:       nil,
	}
}
func (value *ConversationDiscoveryCapabilities) UnmarshalJSON(data []byte) error {
	type plain ConversationDiscoveryCapabilities
	decoded := plain{}
	decoded.Features = []ConversationFeaturesFeature{}
	decoded.Options = nil
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = ConversationDiscoveryCapabilities(decoded)
	return nil
}
func (value ConversationDiscoveryCapabilities) MarshalJSON() ([]byte, error) {
	type plain ConversationDiscoveryCapabilities
	encoded := plain(value)
	encoded.Features = normalizeCollections(encoded.Features)
	encoded.Efforts = normalizeCollections(encoded.Efforts)
	return json.Marshal(encoded)
}
