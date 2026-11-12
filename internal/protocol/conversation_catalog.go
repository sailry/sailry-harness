// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ConversationCatalogStatus struct {
	Revision    uint64                               `json:"revision"`
	UpdatedAtMs *uint64                              `json:"updated_at_ms"`
	Providers   uint32                               `json:"providers"`
	Models      uint32                               `json:"models"`
	Bytes       uint64                               `json:"bytes"`
	Fault       *Fault                               `json:"fault"`
	Scores      ConversationCatalogScoresScoreStatus `json:"scores"`
}

func DefaultConversationCatalogStatus() ConversationCatalogStatus {
	return ConversationCatalogStatus{
		Revision:    0,
		UpdatedAtMs: nil,
		Providers:   0,
		Models:      0,
		Bytes:       0,
		Fault:       nil,
		Scores:      DefaultConversationCatalogScoresScoreStatus(),
	}
}
func (value *ConversationCatalogStatus) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationCatalogStatus) DecodeJSON(decoder *Decoder) error {
	type plain ConversationCatalogStatus
	decoded := plain{}
	decoded.Fault = nil
	decoded.Scores = DefaultConversationCatalogScoresScoreStatus()
	if err := decoder.Object(&decoded, []string{"revision", "providers", "models", "bytes"}, false); err != nil {
		return err
	}
	*value = ConversationCatalogStatus(decoded)
	return nil
}
func (value ConversationCatalogStatus) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogStatus
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCatalogQuery struct {
	Provider *string  `json:"provider"`
	IDs      []string `json:"ids"`
	Revision *uint64  `json:"revision"`
	After    *string  `json:"after"`
	Limit    uint16   `json:"limit"`
}

func DefaultConversationCatalogQuery() ConversationCatalogQuery {
	return ConversationCatalogQuery{
		Provider: nil,
		IDs:      []string{},
		Revision: nil,
		After:    nil,
		Limit:    0,
	}
}
func (value *ConversationCatalogQuery) UnmarshalJSON(data []byte) error {
	type plain ConversationCatalogQuery
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"ids", "limit"}, false); err != nil {
		return err
	}
	*value = ConversationCatalogQuery(decoded)
	return nil
}
func (value ConversationCatalogQuery) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogQuery
	encoded := plain(value)
	encoded.IDs = normalizeCollections(encoded.IDs)
	return json.Marshal(encoded)
}

type ConversationCatalogPage struct {
	Revision uint64                     `json:"revision"`
	Models   []ConversationCatalogModel `json:"models"`
	Next     *string                    `json:"next"`
}

func DefaultConversationCatalogPage() ConversationCatalogPage {
	return ConversationCatalogPage{
		Revision: 0,
		Models:   []ConversationCatalogModel{},
		Next:     nil,
	}
}
func (value *ConversationCatalogPage) UnmarshalJSON(data []byte) error {
	type plain ConversationCatalogPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "models"}, false); err != nil {
		return err
	}
	*value = ConversationCatalogPage(decoded)
	return nil
}
func (value ConversationCatalogPage) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogPage
	encoded := plain(value)
	encoded.Models = normalizeCollections(encoded.Models)
	return json.Marshal(encoded)
}

type ConversationCatalogModel struct {
	ID        string                         `json:"id"`
	Name      string                         `json:"name"`
	Context   *uint32                        `json:"context"`
	Output    *uint32                        `json:"output"`
	Inputs    []string                       `json:"inputs"`
	Outputs   []string                       `json:"outputs"`
	Tools     *bool                          `json:"tools"`
	Reasoning *bool                          `json:"reasoning"`
	Options   []ConversationCatalogReasoning `json:"options"`
}

func DefaultConversationCatalogModel() ConversationCatalogModel {
	return ConversationCatalogModel{
		ID:        "",
		Name:      "",
		Context:   nil,
		Output:    nil,
		Inputs:    []string{},
		Outputs:   []string{},
		Tools:     nil,
		Reasoning: nil,
		Options:   []ConversationCatalogReasoning{},
	}
}
func (value *ConversationCatalogModel) UnmarshalJSON(data []byte) error {
	type plain ConversationCatalogModel
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "name", "inputs", "outputs", "options"}, false); err != nil {
		return err
	}
	*value = ConversationCatalogModel(decoded)
	return nil
}
func (value ConversationCatalogModel) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogModel
	encoded := plain(value)
	encoded.Inputs = normalizeCollections(encoded.Inputs)
	encoded.Outputs = normalizeCollections(encoded.Outputs)
	encoded.Options = normalizeCollections(encoded.Options)
	return json.Marshal(encoded)
}

type ConversationCatalogReasoning struct {
	Kind string
	Data any
}
type ConversationCatalogReasoningEffortPayload struct {
	Values []*string `json:"values"`
}

func DefaultConversationCatalogReasoningEffortPayload() ConversationCatalogReasoningEffortPayload {
	return ConversationCatalogReasoningEffortPayload{
		Values: []*string{},
	}
}
func (value *ConversationCatalogReasoningEffortPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationCatalogReasoningEffortPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"values"}, false); err != nil {
		return err
	}
	*value = ConversationCatalogReasoningEffortPayload(decoded)
	return nil
}
func (value ConversationCatalogReasoningEffortPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogReasoningEffortPayload
	encoded := plain(value)
	encoded.Values = normalizeCollections(encoded.Values)
	return json.Marshal(encoded)
}

type ConversationCatalogReasoningBudgetTokensPayload struct {
	Min *int32  `json:"min"`
	Max *uint32 `json:"max"`
}

func DefaultConversationCatalogReasoningBudgetTokensPayload() ConversationCatalogReasoningBudgetTokensPayload {
	return ConversationCatalogReasoningBudgetTokensPayload{
		Min: nil,
		Max: nil,
	}
}
func (value *ConversationCatalogReasoningBudgetTokensPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationCatalogReasoningBudgetTokensPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = ConversationCatalogReasoningBudgetTokensPayload(decoded)
	return nil
}
func (value ConversationCatalogReasoningBudgetTokensPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogReasoningBudgetTokensPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationCatalogReasoningVariants = variantFactories{"toggle": nil,
	"effort":        func() any { return new(ConversationCatalogReasoningEffortPayload) },
	"budget_tokens": func() any { return new(ConversationCatalogReasoningBudgetTokensPayload) }}

func (value ConversationCatalogReasoning) MarshalJSON() ([]byte, error) {
	return MarshalUnion("type", "", value.Kind, value.Data, conversationCatalogReasoningVariants)
}
func (value *ConversationCatalogReasoning) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "type", "", conversationCatalogReasoningVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
