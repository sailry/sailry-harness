// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationCatalogScoresScoreStatus struct {
	Revision    uint64  `json:"revision"`
	UpdatedAtMs *uint64 `json:"updated_at_ms"`
	CheckedAtMs *uint64 `json:"checked_at_ms"`
	Batch       *string `json:"batch"`
	PublishedAt *string `json:"published_at"`
	Models      uint32  `json:"models"`
	Fault       *Fault  `json:"fault"`
}

func DefaultConversationCatalogScoresScoreStatus() ConversationCatalogScoresScoreStatus {
	return ConversationCatalogScoresScoreStatus{
		Revision:    0,
		UpdatedAtMs: nil,
		CheckedAtMs: nil,
		Batch:       nil,
		PublishedAt: nil,
		Models:      0,
		Fault:       nil,
	}
}
func (value *ConversationCatalogScoresScoreStatus) UnmarshalJSON(data []byte) error {
	type plain ConversationCatalogScoresScoreStatus
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "models"}, false); err != nil {
		return err
	}
	*value = ConversationCatalogScoresScoreStatus(decoded)
	return nil
}
func (value ConversationCatalogScoresScoreStatus) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogScoresScoreStatus
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCatalogScoresModelRef struct {
	Provider ProviderID `json:"provider"`
	Model    string     `json:"model"`
}

func DefaultConversationCatalogScoresModelRef() ConversationCatalogScoresModelRef {
	return ConversationCatalogScoresModelRef{
		Provider: ProviderID{},
		Model:    "",
	}
}
func (value *ConversationCatalogScoresModelRef) UnmarshalJSON(data []byte) error {
	type plain ConversationCatalogScoresModelRef
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "model"}, false); err != nil {
		return err
	}
	*value = ConversationCatalogScoresModelRef(decoded)
	return nil
}
func (value ConversationCatalogScoresModelRef) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogScoresModelRef
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCatalogScores struct {
	Revision    uint64                                `json:"revision"`
	Batch       *string                               `json:"batch"`
	PublishedAt *string                               `json:"published_at"`
	Scores      []ConversationCatalogScoresModelScore `json:"scores"`
}

func DefaultConversationCatalogScores() ConversationCatalogScores {
	return ConversationCatalogScores{
		Revision:    0,
		Batch:       nil,
		PublishedAt: nil,
		Scores:      []ConversationCatalogScoresModelScore{},
	}
}
func (value *ConversationCatalogScores) UnmarshalJSON(data []byte) error {
	type plain ConversationCatalogScores
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "scores"}, false); err != nil {
		return err
	}
	*value = ConversationCatalogScores(decoded)
	return nil
}
func (value ConversationCatalogScores) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogScores
	encoded := plain(value)
	encoded.Scores = normalizeCollections(encoded.Scores)
	return json.Marshal(encoded)
}

type ConversationCatalogScoresModelScore struct {
	Provider ProviderID                             `json:"provider"`
	Model    string                                 `json:"model"`
	Efforts  []ConversationCatalogScoresEffortScore `json:"efforts"`
}

func DefaultConversationCatalogScoresModelScore() ConversationCatalogScoresModelScore {
	return ConversationCatalogScoresModelScore{
		Provider: ProviderID{},
		Model:    "",
		Efforts:  []ConversationCatalogScoresEffortScore{},
	}
}
func (value *ConversationCatalogScoresModelScore) UnmarshalJSON(data []byte) error {
	type plain ConversationCatalogScoresModelScore
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "model", "efforts"}, false); err != nil {
		return err
	}
	*value = ConversationCatalogScoresModelScore(decoded)
	return nil
}
func (value ConversationCatalogScoresModelScore) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogScoresModelScore
	encoded := plain(value)
	encoded.Efforts = normalizeCollections(encoded.Efforts)
	return json.Marshal(encoded)
}

type ConversationCatalogScoresEffortScore struct {
	Effort    Effort   `json:"effort"`
	Overall   float32  `json:"overall"`
	Axes      [5]uint8 `json:"axes"`
	ElapsedMs *uint64  `json:"elapsed_ms"`
	CostUsd   *float32 `json:"cost_usd"`
}

func DefaultConversationCatalogScoresEffortScore() ConversationCatalogScoresEffortScore {
	return ConversationCatalogScoresEffortScore{
		Effort:    Effort{Kind: "default"},
		Overall:   0,
		Axes:      [5]uint8{},
		ElapsedMs: nil,
		CostUsd:   nil,
	}
}
func (value *ConversationCatalogScoresEffortScore) UnmarshalJSON(data []byte) error {
	type plain ConversationCatalogScoresEffortScore
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"effort", "overall", "axes"}, false); err != nil {
		return err
	}
	*value = ConversationCatalogScoresEffortScore(decoded)
	return nil
}
func (value ConversationCatalogScoresEffortScore) MarshalJSON() ([]byte, error) {
	type plain ConversationCatalogScoresEffortScore
	encoded := plain(value)
	encoded.Axes = normalizeCollections(encoded.Axes)
	return json.Marshal(encoded)
}
