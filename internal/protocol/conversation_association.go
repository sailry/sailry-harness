// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationAssociation struct {
	Namespace string `json:"namespace"`
	Kind      string `json:"kind"`
	ID        string `json:"id"`
}

func DefaultConversationAssociation() ConversationAssociation {
	return ConversationAssociation{
		Namespace: "",
		Kind:      "",
		ID:        "",
	}
}
func (value *ConversationAssociation) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationAssociation) DecodeJSON(decoder *Decoder) error {
	type plain ConversationAssociation
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"namespace", "kind", "id"}, true); err != nil {
		return err
	}
	*value = ConversationAssociation(decoded)
	return nil
}
func (value ConversationAssociation) MarshalJSON() ([]byte, error) {
	type plain ConversationAssociation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationAssociationAssociatedSessions struct {
	Sessions   []Session  `json:"sessions"`
	NextBefore *SessionID `json:"next_before"`
}

func DefaultConversationAssociationAssociatedSessions() ConversationAssociationAssociatedSessions {
	return ConversationAssociationAssociatedSessions{
		Sessions:   []Session{},
		NextBefore: nil,
	}
}
func (value *ConversationAssociationAssociatedSessions) UnmarshalJSON(data []byte) error {
	type plain ConversationAssociationAssociatedSessions
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"sessions"}, false); err != nil {
		return err
	}
	*value = ConversationAssociationAssociatedSessions(decoded)
	return nil
}
func (value ConversationAssociationAssociatedSessions) MarshalJSON() ([]byte, error) {
	type plain ConversationAssociationAssociatedSessions
	encoded := plain(value)
	encoded.Sessions = normalizeCollections(encoded.Sessions)
	return json.Marshal(encoded)
}
