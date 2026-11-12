// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type NetworkSelection struct {
	Kind string
	Data any
}
type NetworkSelectionCustomPayload struct {
	Relays []string `json:"relays"`
}

func DefaultNetworkSelectionCustomPayload() NetworkSelectionCustomPayload {
	return NetworkSelectionCustomPayload{
		Relays: []string{},
	}
}
func (value *NetworkSelectionCustomPayload) UnmarshalJSON(data []byte) error {
	type plain NetworkSelectionCustomPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"relays"}, true); err != nil {
		return err
	}
	*value = NetworkSelectionCustomPayload(decoded)
	return nil
}
func (value NetworkSelectionCustomPayload) MarshalJSON() ([]byte, error) {
	type plain NetworkSelectionCustomPayload
	encoded := plain(value)
	encoded.Relays = normalizeCollections(encoded.Relays)
	return json.Marshal(encoded)
}

var networkSelectionVariants = variantFactories{"iroh": nil,
	"custom": func() any { return new(NetworkSelectionCustomPayload) }}

func (value NetworkSelection) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, networkSelectionVariants)
}
func (value *NetworkSelection) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", networkSelectionVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type NetworkSettings struct {
	Revision  uint64            `json:"revision"`
	Selection *NetworkSelection `json:"selection"`
	Effective *NetworkSelection `json:"effective"`
	Address   string            `json:"address"`
}

func DefaultNetworkSettings() NetworkSettings {
	return NetworkSettings{
		Revision:  0,
		Selection: nil,
		Effective: nil,
		Address:   "",
	}
}
func (value *NetworkSettings) UnmarshalJSON(data []byte) error {
	type plain NetworkSettings
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "address"}, true); err != nil {
		return err
	}
	*value = NetworkSettings(decoded)
	return nil
}
func (value NetworkSettings) MarshalJSON() ([]byte, error) {
	type plain NetworkSettings
	encoded := plain(value)
	return json.Marshal(encoded)
}

type NetworkPreset struct {
	ID        string                  `json:"id"`
	Label     PluginDesktopNavigation `json:"label"`
	Selection NetworkSelection        `json:"selection"`
}

func DefaultNetworkPreset() NetworkPreset {
	return NetworkPreset{
		ID:        "",
		Label:     DefaultPluginDesktopNavigation(),
		Selection: NetworkSelection{},
	}
}
func (value *NetworkPreset) UnmarshalJSON(data []byte) error {
	type plain NetworkPreset
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "label", "selection"}, true); err != nil {
		return err
	}
	*value = NetworkPreset(decoded)
	return nil
}
func (value NetworkPreset) MarshalJSON() ([]byte, error) {
	type plain NetworkPreset
	encoded := plain(value)
	return json.Marshal(encoded)
}

type NetworkCatalog struct {
	Version uint32          `json:"version"`
	Presets []NetworkPreset `json:"presets"`
}

func DefaultNetworkCatalog() NetworkCatalog {
	return NetworkCatalog{
		Version: 0,
		Presets: []NetworkPreset{},
	}
}
func (value *NetworkCatalog) UnmarshalJSON(data []byte) error {
	type plain NetworkCatalog
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"version", "presets"}, true); err != nil {
		return err
	}
	*value = NetworkCatalog(decoded)
	return nil
}
func (value NetworkCatalog) MarshalJSON() ([]byte, error) {
	type plain NetworkCatalog
	encoded := plain(value)
	encoded.Presets = normalizeCollections(encoded.Presets)
	return json.Marshal(encoded)
}

type NetworkListing struct {
	Presets     []NetworkPreset `json:"presets"`
	Unavailable bool            `json:"unavailable"`
}

func DefaultNetworkListing() NetworkListing {
	return NetworkListing{
		Presets:     []NetworkPreset{},
		Unavailable: false,
	}
}
func (value *NetworkListing) UnmarshalJSON(data []byte) error {
	type plain NetworkListing
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"presets", "unavailable"}, true); err != nil {
		return err
	}
	*value = NetworkListing(decoded)
	return nil
}
func (value NetworkListing) MarshalJSON() ([]byte, error) {
	type plain NetworkListing
	encoded := plain(value)
	encoded.Presets = normalizeCollections(encoded.Presets)
	return json.Marshal(encoded)
}

type NetworkLatency struct {
	Selection NetworkSelection `json:"selection"`
	Millis    *uint64          `json:"millis"`
}

func DefaultNetworkLatency() NetworkLatency {
	return NetworkLatency{
		Selection: NetworkSelection{},
		Millis:    nil,
	}
}
func (value *NetworkLatency) UnmarshalJSON(data []byte) error {
	type plain NetworkLatency
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"selection"}, true); err != nil {
		return err
	}
	*value = NetworkLatency(decoded)
	return nil
}
func (value NetworkLatency) MarshalJSON() ([]byte, error) {
	type plain NetworkLatency
	encoded := plain(value)
	return json.Marshal(encoded)
}
