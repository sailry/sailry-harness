// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ModelsCatalog struct {
	Models       []ModelsLocalModel                       `json:"models"`
	Availability map[ModelsKind]ModelsCatalogAvailability `json:"availability"`
}

func DefaultModelsCatalog() ModelsCatalog {
	return ModelsCatalog{
		Models:       []ModelsLocalModel{},
		Availability: map[ModelsKind]ModelsCatalogAvailability{},
	}
}
func (value *ModelsCatalog) UnmarshalJSON(data []byte) error {
	type plain ModelsCatalog
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"models", "availability"}, true); err != nil {
		return err
	}
	*value = ModelsCatalog(decoded)
	return nil
}
func (value ModelsCatalog) MarshalJSON() ([]byte, error) {
	type plain ModelsCatalog
	encoded := plain(value)
	encoded.Models = normalizeCollections(encoded.Models)
	encoded.Availability = normalizeCollections(encoded.Availability)
	return json.Marshal(encoded)
}

type ModelsCatalogAvailability struct {
	Kind string
	Data any
}
type ModelsCatalogAvailabilityUnavailablePayload struct {
	Reason string `json:"reason"`
}

func DefaultModelsCatalogAvailabilityUnavailablePayload() ModelsCatalogAvailabilityUnavailablePayload {
	return ModelsCatalogAvailabilityUnavailablePayload{
		Reason: "",
	}
}
func (value *ModelsCatalogAvailabilityUnavailablePayload) UnmarshalJSON(data []byte) error {
	type plain ModelsCatalogAvailabilityUnavailablePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"reason"}, true); err != nil {
		return err
	}
	*value = ModelsCatalogAvailabilityUnavailablePayload(decoded)
	return nil
}
func (value ModelsCatalogAvailabilityUnavailablePayload) MarshalJSON() ([]byte, error) {
	type plain ModelsCatalogAvailabilityUnavailablePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var modelsCatalogAvailabilityVariants = variantFactories{"available": nil,
	"unavailable": func() any { return new(ModelsCatalogAvailabilityUnavailablePayload) }}

func (value ModelsCatalogAvailability) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, modelsCatalogAvailabilityVariants)
}
func (value *ModelsCatalogAvailability) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", modelsCatalogAvailabilityVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
