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
	// Recovered is set for a run that ended before the answering Node
	// started: whatever was to follow its end in that process, a callback or
	// an automatic turn, did not survive. Absent otherwise.
	Recovered bool `json:"recovered,omitempty"`
}

// ConversationRunsBackground counts the session's background work as an
// automatic loop waits on it.
type ConversationRunsBackground struct {
	// Running is work that starts a notice turn when it ends.
	Running uint32 `json:"running"`
	// Pending is ended results a notice turn has reserved or will deliver.
	// Results that only the next user or continuation turn takes (stopped,
	// held or recovered ones) are not counted.
	Pending uint32 `json:"pending"`
}

func DefaultConversationRunsSummary() ConversationRunsSummary {
	return ConversationRunsSummary{
		Turn:      TurnID{},
		Sequence:  0,
		Status:    ConversationStatus(""),
		Origin:    nil,
		ErrorCode: nil,
		Recovered: false,
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
	// Background is absent from a Node that does not state it.
	Background *ConversationRunsBackground `json:"background,omitempty"`
}

func DefaultConversationRunsPage() ConversationRunsPage {
	return ConversationRunsPage{
		Session:       SessionID{},
		Revision:      0,
		Runs:          []ConversationRunsSummary{},
		NextBefore:    nil,
		QueueRevision: 0,
		Background:    nil,
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
