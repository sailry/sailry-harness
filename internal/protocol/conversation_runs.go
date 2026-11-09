// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationRunsSummary struct {
	Turn      TurnID             `json:"turn"`
	Sequence  uint64             `json:"sequence"`
	Status    ConversationStatus `json:"status"`
	Origin    *SessionID         `json:"origin"`
	ErrorCode *ErrorCode         `json:"error_code"`
}

func DefaultConversationRunsSummary() ConversationRunsSummary {
	return ConversationRunsSummary{
		Turn:      TurnID{},
		Sequence:  0,
		Status:    ConversationStatus(""),
		Origin:    nil,
		ErrorCode: nil,
	}
}
func (value *ConversationRunsSummary) UnmarshalJSON(data []byte) error {
	type plain ConversationRunsSummary
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "sequence", "status"}, false); err != nil {
		return err
	}
	*value = ConversationRunsSummary(decoded)
	return nil
}
func (value ConversationRunsSummary) MarshalJSON() ([]byte, error) {
	type plain ConversationRunsSummary
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationRunsPage struct {
	Session       SessionID                 `json:"session"`
	Revision      uint64                    `json:"revision"`
	Runs          []ConversationRunsSummary `json:"runs"`
	NextBefore    *TurnID                   `json:"next_before"`
	QueueRevision uint64                    `json:"queue_revision"`
}

func DefaultConversationRunsPage() ConversationRunsPage {
	return ConversationRunsPage{
		Session:       SessionID{},
		Revision:      0,
		Runs:          []ConversationRunsSummary{},
		NextBefore:    nil,
		QueueRevision: 0,
	}
}
func (value *ConversationRunsPage) UnmarshalJSON(data []byte) error {
	type plain ConversationRunsPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "revision", "runs", "queue_revision"}, false); err != nil {
		return err
	}
	*value = ConversationRunsPage(decoded)
	return nil
}
func (value ConversationRunsPage) MarshalJSON() ([]byte, error) {
	type plain ConversationRunsPage
	encoded := plain(value)
	encoded.Runs = normalizeCollections(encoded.Runs)
	return json.Marshal(encoded)
}
