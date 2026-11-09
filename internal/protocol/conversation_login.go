// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationLoginAttempt struct {
	ID       RequestID  `json:"id"`
	Provider ProviderID `json:"provider"`
}

func DefaultConversationLoginAttempt() ConversationLoginAttempt {
	return ConversationLoginAttempt{
		ID:       RequestID{},
		Provider: ProviderID{},
	}
}
func (value *ConversationLoginAttempt) UnmarshalJSON(data []byte) error {
	type plain ConversationLoginAttempt
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "provider"}, false); err != nil {
		return err
	}
	*value = ConversationLoginAttempt(decoded)
	return nil
}
func (value ConversationLoginAttempt) MarshalJSON() ([]byte, error) {
	type plain ConversationLoginAttempt
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationLoginUpdate struct {
	Attempt    ConversationLoginAttempt `json:"attempt"`
	Revision   uint64                   `json:"revision"`
	State      ConversationLoginState   `json:"state"`
	ModelError *Fault                   `json:"model_error,omitempty"`
}

func DefaultConversationLoginUpdate() ConversationLoginUpdate {
	return ConversationLoginUpdate{
		Attempt:    DefaultConversationLoginAttempt(),
		Revision:   0,
		State:      ConversationLoginState{},
		ModelError: nil,
	}
}
func (value *ConversationLoginUpdate) UnmarshalJSON(data []byte) error {
	type plain ConversationLoginUpdate
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"attempt", "revision", "state"}, false); err != nil {
		return err
	}
	*value = ConversationLoginUpdate(decoded)
	return nil
}
func (value ConversationLoginUpdate) MarshalJSON() ([]byte, error) {
	type plain ConversationLoginUpdate
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationLoginState struct {
	Kind string
	Data any
}
type ConversationLoginStatePendingPayload struct {
	VerificationUrl string `json:"verification_url"`
	UserCode        string `json:"user_code"`
	ExpiresAtMs     uint64 `json:"expires_at_ms"`
}

func DefaultConversationLoginStatePendingPayload() ConversationLoginStatePendingPayload {
	return ConversationLoginStatePendingPayload{
		VerificationUrl: "",
		UserCode:        "",
		ExpiresAtMs:     0,
	}
}
func (value *ConversationLoginStatePendingPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationLoginStatePendingPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"verification_url", "user_code", "expires_at_ms"}, false); err != nil {
		return err
	}
	*value = ConversationLoginStatePendingPayload(decoded)
	return nil
}
func (value ConversationLoginStatePendingPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationLoginStatePendingPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationLoginStateBrowserPayload struct {
	AuthorizationUrl string `json:"authorization_url"`
	ExpiresAtMs      uint64 `json:"expires_at_ms"`
}

func DefaultConversationLoginStateBrowserPayload() ConversationLoginStateBrowserPayload {
	return ConversationLoginStateBrowserPayload{
		AuthorizationUrl: "",
		ExpiresAtMs:      0,
	}
}
func (value *ConversationLoginStateBrowserPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationLoginStateBrowserPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"authorization_url", "expires_at_ms"}, false); err != nil {
		return err
	}
	*value = ConversationLoginStateBrowserPayload(decoded)
	return nil
}
func (value ConversationLoginStateBrowserPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationLoginStateBrowserPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationLoginStateVariants = variantFactories{"starting": nil,
	"pending":    func() any { return new(ConversationLoginStatePendingPayload) },
	"browser":    func() any { return new(ConversationLoginStateBrowserPayload) },
	"exchanging": nil,
	"connected":  nil,
	"cancelled":  nil,
	"failed":     func() any { return new(Fault) }}

func (value ConversationLoginState) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationLoginStateVariants)
}
func (value *ConversationLoginState) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationLoginStateVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
