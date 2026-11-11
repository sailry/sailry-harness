// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationCheckpointReceipt string

const (
	ConversationCheckpointReceiptWritten   ConversationCheckpointReceipt = "Written"
	ConversationCheckpointReceiptUncertain ConversationCheckpointReceipt = "Uncertain"
	ConversationCheckpointReceiptPending   ConversationCheckpointReceipt = "Pending"
	ConversationCheckpointReceiptAbsent    ConversationCheckpointReceipt = "Absent"
)

func (value ConversationCheckpointReceipt) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationCheckpointReceipt) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationCheckpointReceipt(decoded) {
	case ConversationCheckpointReceiptWritten, ConversationCheckpointReceiptUncertain, ConversationCheckpointReceiptPending, ConversationCheckpointReceiptAbsent:
		*value = ConversationCheckpointReceipt(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationCheckpointReceipt) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationCheckpointReceipt) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationCheckpointVersion struct {
	Revision string `json:"revision"`
	Size     uint64 `json:"size"`
}

func DefaultConversationCheckpointVersion() ConversationCheckpointVersion {
	return ConversationCheckpointVersion{
		Revision: "",
		Size:     0,
	}
}
func (value *ConversationCheckpointVersion) UnmarshalJSON(data []byte) error {
	type plain ConversationCheckpointVersion
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "size"}, false); err != nil {
		return err
	}
	*value = ConversationCheckpointVersion(decoded)
	return nil
}
func (value ConversationCheckpointVersion) MarshalJSON() ([]byte, error) {
	type plain ConversationCheckpointVersion
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCheckpointFile struct {
	ID       CheckpointID                   `json:"id"`
	Turn     TurnID                         `json:"turn"`
	Worktree WorktreeID                     `json:"worktree"`
	Path     string                         `json:"path"`
	Before   *ConversationCheckpointVersion `json:"before"`
	After    ConversationCheckpointVersion  `json:"after"`
	Outcome  RequestOutcome                 `json:"outcome"`
}

func DefaultConversationCheckpointFile() ConversationCheckpointFile {
	return ConversationCheckpointFile{
		ID:       CheckpointID{},
		Turn:     TurnID{},
		Worktree: WorktreeID{},
		Path:     "",
		Before:   nil,
		After:    DefaultConversationCheckpointVersion(),
		Outcome:  RequestOutcome{},
	}
}
func (value *ConversationCheckpointFile) UnmarshalJSON(data []byte) error {
	type plain ConversationCheckpointFile
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "turn", "worktree", "path", "after", "outcome"}, false); err != nil {
		return err
	}
	*value = ConversationCheckpointFile(decoded)
	return nil
}
func (value ConversationCheckpointFile) MarshalJSON() ([]byte, error) {
	type plain ConversationCheckpointFile
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCheckpointPage struct {
	Session SessionID                    `json:"session"`
	Turn    TurnID                       `json:"turn"`
	Files   []ConversationCheckpointFile `json:"files"`
	Next    *CheckpointID                `json:"next"`
}

func DefaultConversationCheckpointPage() ConversationCheckpointPage {
	return ConversationCheckpointPage{
		Session: SessionID{},
		Turn:    TurnID{},
		Files:   []ConversationCheckpointFile{},
		Next:    nil,
	}
}
func (value *ConversationCheckpointPage) UnmarshalJSON(data []byte) error {
	type plain ConversationCheckpointPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "files"}, false); err != nil {
		return err
	}
	*value = ConversationCheckpointPage(decoded)
	return nil
}
func (value ConversationCheckpointPage) MarshalJSON() ([]byte, error) {
	type plain ConversationCheckpointPage
	encoded := plain(value)
	encoded.Files = normalizeCollections(encoded.Files)
	return json.Marshal(encoded)
}

type ConversationCheckpointTurnDiff struct {
	Session SessionID `json:"session"`
	Turn    TurnID    `json:"turn"`
	Files   []GitDiff `json:"files"`
	Partial bool      `json:"partial"`
}

func DefaultConversationCheckpointTurnDiff() ConversationCheckpointTurnDiff {
	return ConversationCheckpointTurnDiff{
		Session: SessionID{},
		Turn:    TurnID{},
		Files:   []GitDiff{},
		Partial: false,
	}
}
func (value *ConversationCheckpointTurnDiff) UnmarshalJSON(data []byte) error {
	type plain ConversationCheckpointTurnDiff
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "files", "partial"}, false); err != nil {
		return err
	}
	*value = ConversationCheckpointTurnDiff(decoded)
	return nil
}
func (value ConversationCheckpointTurnDiff) MarshalJSON() ([]byte, error) {
	type plain ConversationCheckpointTurnDiff
	encoded := plain(value)
	encoded.Files = normalizeCollections(encoded.Files)
	return json.Marshal(encoded)
}

type ConversationCheckpointContent struct {
	Session SessionID                  `json:"session"`
	File    ConversationCheckpointFile `json:"file"`
	Before  *string                    `json:"before"`
	After   string                     `json:"after"`
}

func DefaultConversationCheckpointContent() ConversationCheckpointContent {
	return ConversationCheckpointContent{
		Session: SessionID{},
		File:    DefaultConversationCheckpointFile(),
		Before:  nil,
		After:   "",
	}
}
func (value *ConversationCheckpointContent) UnmarshalJSON(data []byte) error {
	type plain ConversationCheckpointContent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "file", "after"}, false); err != nil {
		return err
	}
	*value = ConversationCheckpointContent(decoded)
	return nil
}
func (value ConversationCheckpointContent) MarshalJSON() ([]byte, error) {
	type plain ConversationCheckpointContent
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCheckpointRestored struct {
	Session    SessionID    `json:"session"`
	Checkpoint CheckpointID `json:"checkpoint"`
	Path       string       `json:"path"`
	Revision   *string      `json:"revision"`
}

func DefaultConversationCheckpointRestored() ConversationCheckpointRestored {
	return ConversationCheckpointRestored{
		Session:    SessionID{},
		Checkpoint: CheckpointID{},
		Path:       "",
		Revision:   nil,
	}
}
func (value *ConversationCheckpointRestored) UnmarshalJSON(data []byte) error {
	type plain ConversationCheckpointRestored
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "checkpoint", "path"}, false); err != nil {
		return err
	}
	*value = ConversationCheckpointRestored(decoded)
	return nil
}
func (value ConversationCheckpointRestored) MarshalJSON() ([]byte, error) {
	type plain ConversationCheckpointRestored
	encoded := plain(value)
	return json.Marshal(encoded)
}
