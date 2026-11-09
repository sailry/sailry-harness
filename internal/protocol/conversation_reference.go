// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationReferenceTarget struct {
	Kind string
	Data any
}
type ConversationReferenceTargetDatabasePayload struct {
	Connection DatabaseID     `json:"connection"`
	Database   *string        `json:"database"`
	Table      *DatabaseTable `json:"table"`
}

func DefaultConversationReferenceTargetDatabasePayload() ConversationReferenceTargetDatabasePayload {
	return ConversationReferenceTargetDatabasePayload{
		Connection: DatabaseID{},
		Database:   nil,
		Table:      nil,
	}
}
func (value *ConversationReferenceTargetDatabasePayload) UnmarshalJSON(data []byte) error {
	type plain ConversationReferenceTargetDatabasePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"connection"}, false); err != nil {
		return err
	}
	*value = ConversationReferenceTargetDatabasePayload(decoded)
	return nil
}
func (value ConversationReferenceTargetDatabasePayload) MarshalJSON() ([]byte, error) {
	type plain ConversationReferenceTargetDatabasePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationReferenceTargetSkillPayload struct {
	Package string `json:"package"`
	Name    string `json:"name"`
}

func DefaultConversationReferenceTargetSkillPayload() ConversationReferenceTargetSkillPayload {
	return ConversationReferenceTargetSkillPayload{
		Package: "",
		Name:    "",
	}
}
func (value *ConversationReferenceTargetSkillPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationReferenceTargetSkillPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "name"}, false); err != nil {
		return err
	}
	*value = ConversationReferenceTargetSkillPayload(decoded)
	return nil
}
func (value ConversationReferenceTargetSkillPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationReferenceTargetSkillPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationReferenceTargetVariants = variantFactories{"ssh": func() any { return new(SshID) },
	"database":   func() any { return new(ConversationReferenceTargetDatabasePayload) },
	"file":       func() any { return new(string) },
	"directory":  func() any { return new(string) },
	"agent":      func() any { return new(RoleReference) },
	"session":    func() any { return new(SessionID) },
	"plugin":     func() any { return new(string) },
	"skill":      func() any { return new(ConversationReferenceTargetSkillPayload) },
	"host":       nil,
	"project":    nil,
	"worktree":   nil,
	"attachment": func() any { return new(AttachmentID) }}

func (value ConversationReferenceTarget) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationReferenceTargetVariants)
}
func (value *ConversationReferenceTarget) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationReferenceTargetVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationReference struct {
	Target ConversationReferenceTarget `json:"target"`
	Label  string                      `json:"label"`
}

func DefaultConversationReference() ConversationReference {
	return ConversationReference{
		Target: ConversationReferenceTarget{},
		Label:  "",
	}
}
func (value *ConversationReference) UnmarshalJSON(data []byte) error {
	type plain ConversationReference
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"target", "label"}, false); err != nil {
		return err
	}
	*value = ConversationReference(decoded)
	return nil
}
func (value ConversationReference) MarshalJSON() ([]byte, error) {
	type plain ConversationReference
	encoded := plain(value)
	return json.Marshal(encoded)
}
