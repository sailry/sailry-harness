// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationCheckpointHunk struct {
	ID        string `json:"id"`
	OldStart  uint32 `json:"old_start"`
	NewStart  uint32 `json:"new_start"`
	Text      string `json:"text"`
	Additions uint64 `json:"additions"`
	Deletions uint64 `json:"deletions"`
}

func DefaultConversationCheckpointHunk() ConversationCheckpointHunk {
	return ConversationCheckpointHunk{
		ID:        "",
		OldStart:  0,
		NewStart:  0,
		Text:      "",
		Additions: 0,
		Deletions: 0,
	}
}
func (value *ConversationCheckpointHunk) UnmarshalJSON(data []byte) error {
	type plain ConversationCheckpointHunk
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "old_start", "new_start", "text", "additions", "deletions"}, false); err != nil {
		return err
	}
	*value = ConversationCheckpointHunk(decoded)
	return nil
}
func (value ConversationCheckpointHunk) MarshalJSON() ([]byte, error) {
	type plain ConversationCheckpointHunk
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCheckpointHunkReview struct {
	Session  SessionID                    `json:"session"`
	Turn     TurnID                       `json:"turn"`
	Worktree WorktreeID                   `json:"worktree"`
	Path     string                       `json:"path"`
	Hunks    []ConversationCheckpointHunk `json:"hunks"`
	Partial  bool                         `json:"partial"`
}

func DefaultConversationCheckpointHunkReview() ConversationCheckpointHunkReview {
	return ConversationCheckpointHunkReview{
		Session:  SessionID{},
		Turn:     TurnID{},
		Worktree: WorktreeID{},
		Path:     "",
		Hunks:    []ConversationCheckpointHunk{},
		Partial:  false,
	}
}
func (value *ConversationCheckpointHunkReview) UnmarshalJSON(data []byte) error {
	type plain ConversationCheckpointHunkReview
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "worktree", "path", "hunks", "partial"}, false); err != nil {
		return err
	}
	*value = ConversationCheckpointHunkReview(decoded)
	return nil
}
func (value ConversationCheckpointHunkReview) MarshalJSON() ([]byte, error) {
	type plain ConversationCheckpointHunkReview
	encoded := plain(value)
	encoded.Hunks = normalizeCollections(encoded.Hunks)
	return json.Marshal(encoded)
}

type ConversationCheckpointHunkOperation string

const (
	ConversationCheckpointHunkOperationReject ConversationCheckpointHunkOperation = "reject"
	ConversationCheckpointHunkOperationStage  ConversationCheckpointHunkOperation = "stage"
)

func (value ConversationCheckpointHunkOperation) MarshalJSON() ([]byte, error) {
	return marshalEnum(value)
}
func (value *ConversationCheckpointHunkOperation) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationCheckpointHunkOperation(decoded) {
	case ConversationCheckpointHunkOperationReject, ConversationCheckpointHunkOperationStage:
		*value = ConversationCheckpointHunkOperation(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationCheckpointHunkOperation) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationCheckpointHunkOperation) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationCheckpointHunkSelection struct {
	Session          SessionID                           `json:"session"`
	Turn             TurnID                              `json:"turn"`
	Worktree         WorktreeID                          `json:"worktree"`
	Path             string                              `json:"path"`
	Hunk             string                              `json:"hunk"`
	Operation        ConversationCheckpointHunkOperation `json:"operation"`
	ExpectedRevision string                              `json:"expected_revision"`
	ExpectedIndex    *string                             `json:"expected_index"`
	ExpectedHead     *string                             `json:"expected_head"`
}

func DefaultConversationCheckpointHunkSelection() ConversationCheckpointHunkSelection {
	return ConversationCheckpointHunkSelection{
		Session:          SessionID{},
		Turn:             TurnID{},
		Worktree:         WorktreeID{},
		Path:             "",
		Hunk:             "",
		Operation:        ConversationCheckpointHunkOperation(""),
		ExpectedRevision: "",
		ExpectedIndex:    nil,
		ExpectedHead:     nil,
	}
}
func (value *ConversationCheckpointHunkSelection) UnmarshalJSON(data []byte) error {
	type plain ConversationCheckpointHunkSelection
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "worktree", "path", "hunk", "operation", "expected_revision"}, true); err != nil {
		return err
	}
	*value = ConversationCheckpointHunkSelection(decoded)
	return nil
}
func (value ConversationCheckpointHunkSelection) MarshalJSON() ([]byte, error) {
	type plain ConversationCheckpointHunkSelection
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCheckpointHunkApplied struct {
	Session  SessionID `json:"session"`
	Turn     TurnID    `json:"turn"`
	Path     string    `json:"path"`
	Hunk     string    `json:"hunk"`
	Revision string    `json:"revision"`
	Index    *GitIndex `json:"index"`
}

func DefaultConversationCheckpointHunkApplied() ConversationCheckpointHunkApplied {
	return ConversationCheckpointHunkApplied{
		Session:  SessionID{},
		Turn:     TurnID{},
		Path:     "",
		Hunk:     "",
		Revision: "",
		Index:    nil,
	}
}
func (value *ConversationCheckpointHunkApplied) UnmarshalJSON(data []byte) error {
	type plain ConversationCheckpointHunkApplied
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "path", "hunk", "revision"}, false); err != nil {
		return err
	}
	*value = ConversationCheckpointHunkApplied(decoded)
	return nil
}
func (value ConversationCheckpointHunkApplied) MarshalJSON() ([]byte, error) {
	type plain ConversationCheckpointHunkApplied
	encoded := plain(value)
	return json.Marshal(encoded)
}
