// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ModelsLocalChoice struct {
	Enabled bool    `json:"enabled"`
	Model   *string `json:"model"`
}

func DefaultModelsLocalChoice() ModelsLocalChoice {
	return ModelsLocalChoice{
		Enabled: false,
		Model:   nil,
	}
}
func (value *ModelsLocalChoice) UnmarshalJSON(data []byte) error {
	type plain ModelsLocalChoice
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"enabled"}, true); err != nil {
		return err
	}
	*value = ModelsLocalChoice(decoded)
	return nil
}
func (value ModelsLocalChoice) MarshalJSON() ([]byte, error) {
	type plain ModelsLocalChoice
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ModelsLocalSettings struct {
	Revision uint64                           `json:"revision"`
	Mirror   *string                          `json:"mirror"`
	Choices  map[ModelsKind]ModelsLocalChoice `json:"choices"`
}

func DefaultModelsLocalSettings() ModelsLocalSettings {
	return ModelsLocalSettings{
		Revision: 0,
		Mirror:   nil,
		Choices:  defaultModelsLocalChoices(),
	}
}
func (value *ModelsLocalSettings) UnmarshalJSON(data []byte) error {
	type plain ModelsLocalSettings
	decoded := plain{}
	decoded.Choices = map[ModelsKind]ModelsLocalChoice{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, true); err != nil {
		return err
	}
	*value = ModelsLocalSettings(decoded)
	return nil
}
func (value ModelsLocalSettings) MarshalJSON() ([]byte, error) {
	type plain ModelsLocalSettings
	encoded := plain(value)
	encoded.Choices = normalizeCollections(encoded.Choices)
	return json.Marshal(encoded)
}

type ModelsLocalModel struct {
	ID         string           `json:"id"`
	Definition ModelsDefinition `json:"definition"`
	State      ModelsLocalState `json:"state"`
}

func DefaultModelsLocalModel() ModelsLocalModel {
	return ModelsLocalModel{
		ID:         "",
		Definition: DefaultModelsDefinition(),
		State:      ModelsLocalState{},
	}
}
func (value *ModelsLocalModel) UnmarshalJSON(data []byte) error {
	type plain ModelsLocalModel
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "definition", "state"}, false); err != nil {
		return err
	}
	*value = ModelsLocalModel(decoded)
	return nil
}
func (value ModelsLocalModel) MarshalJSON() ([]byte, error) {
	type plain ModelsLocalModel
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ModelsLocalState struct {
	Kind string
	Data any
}

var modelsLocalStateVariants = variantFactories{"missing": nil,
	"downloading": nil,
	"installed":   nil}

func (value ModelsLocalState) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, modelsLocalStateVariants)
}
func (value *ModelsLocalState) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", modelsLocalStateVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
