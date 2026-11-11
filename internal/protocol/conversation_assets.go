// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationAssetsKind string

const (
	ConversationAssetsKindResource ConversationAssetsKind = "resource"
	ConversationAssetsKindArtifact ConversationAssetsKind = "artifact"
)

func (value ConversationAssetsKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationAssetsKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationAssetsKind(decoded) {
	case ConversationAssetsKindResource, ConversationAssetsKindArtifact:
		*value = ConversationAssetsKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationAssetsKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationAssetsKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationAssetsQuery struct {
	Kind   *ConversationAssetsKind `json:"kind"`
	Before *uint64                 `json:"before"`
	Limit  uint16                  `json:"limit"`
}

func DefaultConversationAssetsQuery() ConversationAssetsQuery {
	return ConversationAssetsQuery{
		Kind:   nil,
		Before: nil,
		Limit:  0,
	}
}
func (value *ConversationAssetsQuery) UnmarshalJSON(data []byte) error {
	type plain ConversationAssetsQuery
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"limit"}, false); err != nil {
		return err
	}
	*value = ConversationAssetsQuery(decoded)
	return nil
}
func (value ConversationAssetsQuery) MarshalJSON() ([]byte, error) {
	type plain ConversationAssetsQuery
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationAssetsTarget struct {
	Kind string
	Data any
}
type ConversationAssetsTargetFilePayload struct {
	Path string `json:"path"`
}

func DefaultConversationAssetsTargetFilePayload() ConversationAssetsTargetFilePayload {
	return ConversationAssetsTargetFilePayload{
		Path: "",
	}
}
func (value *ConversationAssetsTargetFilePayload) UnmarshalJSON(data []byte) error {
	type plain ConversationAssetsTargetFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, false); err != nil {
		return err
	}
	*value = ConversationAssetsTargetFilePayload(decoded)
	return nil
}
func (value ConversationAssetsTargetFilePayload) MarshalJSON() ([]byte, error) {
	type plain ConversationAssetsTargetFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationAssetsTargetVariants = variantFactories{"attachment": func() any { return new(Attachment) },
	"image":    func() any { return new(ConversationImage) },
	"document": func() any { return new(ArtifactSummary) },
	"file":     func() any { return new(ConversationAssetsTargetFilePayload) }}

func (value ConversationAssetsTarget) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationAssetsTargetVariants)
}
func (value *ConversationAssetsTarget) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationAssetsTargetVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationAssetsGroup struct {
	Sequence    uint64                     `json:"sequence"`
	Turn        TurnID                     `json:"turn"`
	Worktree    WorktreeID                 `json:"worktree"`
	TimestampMs int64                      `json:"timestamp_ms"`
	Kind        ConversationAssetsKind     `json:"kind"`
	Items       []ConversationAssetsTarget `json:"items"`
}

func DefaultConversationAssetsGroup() ConversationAssetsGroup {
	return ConversationAssetsGroup{
		Sequence:    0,
		Turn:        TurnID{},
		Worktree:    WorktreeID{},
		TimestampMs: 0,
		Kind:        ConversationAssetsKind(""),
		Items:       []ConversationAssetsTarget{},
	}
}
func (value *ConversationAssetsGroup) UnmarshalJSON(data []byte) error {
	type plain ConversationAssetsGroup
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"sequence", "turn", "worktree", "timestamp_ms", "kind", "items"}, false); err != nil {
		return err
	}
	*value = ConversationAssetsGroup(decoded)
	return nil
}
func (value ConversationAssetsGroup) MarshalJSON() ([]byte, error) {
	type plain ConversationAssetsGroup
	encoded := plain(value)
	encoded.Items = normalizeCollections(encoded.Items)
	return json.Marshal(encoded)
}

type ConversationAssetsPage struct {
	Session    SessionID                 `json:"session"`
	Revision   uint64                    `json:"revision"`
	Groups     []ConversationAssetsGroup `json:"groups"`
	NextBefore *uint64                   `json:"next_before"`
}

func DefaultConversationAssetsPage() ConversationAssetsPage {
	return ConversationAssetsPage{
		Session:    SessionID{},
		Revision:   0,
		Groups:     []ConversationAssetsGroup{},
		NextBefore: nil,
	}
}
func (value *ConversationAssetsPage) UnmarshalJSON(data []byte) error {
	type plain ConversationAssetsPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "revision", "groups"}, false); err != nil {
		return err
	}
	*value = ConversationAssetsPage(decoded)
	return nil
}
func (value ConversationAssetsPage) MarshalJSON() ([]byte, error) {
	type plain ConversationAssetsPage
	encoded := plain(value)
	encoded.Groups = normalizeCollections(encoded.Groups)
	return json.Marshal(encoded)
}
