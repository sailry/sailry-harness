// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type ConversationProgress struct {
	Title *string                    `json:"title"`
	Steps []ConversationProgressStep `json:"steps"`
}

func DefaultConversationProgress() ConversationProgress {
	return ConversationProgress{
		Title: nil,
		Steps: []ConversationProgressStep{},
	}
}
func (value *ConversationProgress) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationProgress) DecodeJSON(decoder *Decoder) error {
	type plain ConversationProgress
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"steps"}, true); err != nil {
		return err
	}
	*value = ConversationProgress(decoded)
	return nil
}
func (value ConversationProgress) MarshalJSON() ([]byte, error) {
	type plain ConversationProgress
	encoded := plain(value)
	encoded.Steps = normalizeCollections(encoded.Steps)
	return json.Marshal(encoded)
}

type ConversationProgressStep struct {
	Description string                    `json:"description"`
	State       ConversationProgressState `json:"state"`
}

func DefaultConversationProgressStep() ConversationProgressStep {
	return ConversationProgressStep{
		Description: "",
		State:       ConversationProgressState(""),
	}
}
func (value *ConversationProgressStep) UnmarshalJSON(data []byte) error {
	type plain ConversationProgressStep
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"description", "state"}, true); err != nil {
		return err
	}
	*value = ConversationProgressStep(decoded)
	return nil
}
func (value ConversationProgressStep) MarshalJSON() ([]byte, error) {
	type plain ConversationProgressStep
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationProgressState string

const (
	ConversationProgressStatePending    ConversationProgressState = "pending"
	ConversationProgressStateInProgress ConversationProgressState = "in_progress"
	ConversationProgressStateCompleted  ConversationProgressState = "completed"
	ConversationProgressStateSkipped    ConversationProgressState = "skipped"
)

func (value ConversationProgressState) MarshalJSON() ([]byte, error) {
	switch value {
	case ConversationProgressStatePending, ConversationProgressStateInProgress, ConversationProgressStateCompleted, ConversationProgressStateSkipped:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ConversationProgressState %q", value)
	}
}
func (value *ConversationProgressState) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationProgressState(decoded) {
	case ConversationProgressStatePending, ConversationProgressStateInProgress, ConversationProgressStateCompleted, ConversationProgressStateSkipped:
		*value = ConversationProgressState(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ConversationProgressState %q", decoded)
	}
}
func (value ConversationProgressState) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationProgressState) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
