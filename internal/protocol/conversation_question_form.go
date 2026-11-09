// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationQuestionFormField struct {
	Name        string                        `json:"name"`
	Title       string                        `json:"title"`
	Description *string                       `json:"description"`
	Required    bool                          `json:"required"`
	Input       ConversationQuestionFormInput `json:"input"`
}

func DefaultConversationQuestionFormField() ConversationQuestionFormField {
	return ConversationQuestionFormField{
		Name:        "",
		Title:       "",
		Description: nil,
		Required:    false,
		Input:       ConversationQuestionFormInput{},
	}
}
func (value *ConversationQuestionFormField) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionFormField
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "title", "required", "input"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionFormField(decoded)
	return nil
}
func (value ConversationQuestionFormField) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionFormField
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationQuestionFormInput struct {
	Kind string
	Data any
}
type ConversationQuestionFormInputTextPayload struct {
	MinLength *uint64 `json:"min_length"`
	MaxLength *uint64 `json:"max_length"`
	Format    *string `json:"format"`
}

func DefaultConversationQuestionFormInputTextPayload() ConversationQuestionFormInputTextPayload {
	return ConversationQuestionFormInputTextPayload{
		MinLength: nil,
		MaxLength: nil,
		Format:    nil,
	}
}
func (value *ConversationQuestionFormInputTextPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionFormInputTextPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ConversationQuestionFormInputTextPayload(decoded)
	return nil
}
func (value ConversationQuestionFormInputTextPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionFormInputTextPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationQuestionFormInputNumberPayload struct {
	Integer bool         `json:"integer"`
	Minimum *json.Number `json:"minimum"`
	Maximum *json.Number `json:"maximum"`
}

func DefaultConversationQuestionFormInputNumberPayload() ConversationQuestionFormInputNumberPayload {
	return ConversationQuestionFormInputNumberPayload{
		Integer: false,
		Minimum: nil,
		Maximum: nil,
	}
}
func (value *ConversationQuestionFormInputNumberPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionFormInputNumberPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"integer"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionFormInputNumberPayload(decoded)
	return nil
}
func (value ConversationQuestionFormInputNumberPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionFormInputNumberPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationQuestionFormInputChoicePayload struct {
	Options  []ConversationQuestionFormChoice `json:"options"`
	Multiple bool                             `json:"multiple"`
	MinItems *uint64                          `json:"min_items"`
	MaxItems *uint64                          `json:"max_items"`
}

func DefaultConversationQuestionFormInputChoicePayload() ConversationQuestionFormInputChoicePayload {
	return ConversationQuestionFormInputChoicePayload{
		Options:  []ConversationQuestionFormChoice{},
		Multiple: false,
		MinItems: nil,
		MaxItems: nil,
	}
}
func (value *ConversationQuestionFormInputChoicePayload) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionFormInputChoicePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"options", "multiple"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionFormInputChoicePayload(decoded)
	return nil
}
func (value ConversationQuestionFormInputChoicePayload) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionFormInputChoicePayload
	encoded := plain(value)
	encoded.Options = normalizeCollections(encoded.Options)
	return json.Marshal(encoded)
}

var conversationQuestionFormInputVariants = variantFactories{"text": func() any { return new(ConversationQuestionFormInputTextPayload) },
	"number":  func() any { return new(ConversationQuestionFormInputNumberPayload) },
	"boolean": nil,
	"choice":  func() any { return new(ConversationQuestionFormInputChoicePayload) }}

func (value ConversationQuestionFormInput) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, conversationQuestionFormInputVariants)
}
func (value *ConversationQuestionFormInput) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", conversationQuestionFormInputVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationQuestionFormChoice struct {
	Value string `json:"value"`
	Title string `json:"title"`
}

func DefaultConversationQuestionFormChoice() ConversationQuestionFormChoice {
	return ConversationQuestionFormChoice{
		Value: "",
		Title: "",
	}
}
func (value *ConversationQuestionFormChoice) UnmarshalJSON(data []byte) error {
	type plain ConversationQuestionFormChoice
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"value", "title"}, true); err != nil {
		return err
	}
	*value = ConversationQuestionFormChoice(decoded)
	return nil
}
func (value ConversationQuestionFormChoice) MarshalJSON() ([]byte, error) {
	type plain ConversationQuestionFormChoice
	encoded := plain(value)
	return json.Marshal(encoded)
}
