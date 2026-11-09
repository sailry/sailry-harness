// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PluginTransactionOperation struct {
	Kind string
	Data any
}
type PluginTransactionOperationWritePayload struct {
	Key              string          `json:"key"`
	Value            json.RawMessage `json:"value"`
	ExpectedRevision uint64          `json:"expected_revision"`
}

func DefaultPluginTransactionOperationWritePayload() PluginTransactionOperationWritePayload {
	return PluginTransactionOperationWritePayload{
		Key:              "",
		Value:            json.RawMessage("null"),
		ExpectedRevision: 0,
	}
}
func (value *PluginTransactionOperationWritePayload) UnmarshalJSON(data []byte) error {
	type plain PluginTransactionOperationWritePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "value", "expected_revision"}, true); err != nil {
		return err
	}
	*value = PluginTransactionOperationWritePayload(decoded)
	return nil
}
func (value PluginTransactionOperationWritePayload) MarshalJSON() ([]byte, error) {
	type plain PluginTransactionOperationWritePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginTransactionOperationIndexPayload struct {
	Key              string             `json:"key"`
	Value            json.RawMessage    `json:"value"`
	Index            PluginStorageIndex `json:"index"`
	ExpectedRevision uint64             `json:"expected_revision"`
}

func DefaultPluginTransactionOperationIndexPayload() PluginTransactionOperationIndexPayload {
	return PluginTransactionOperationIndexPayload{
		Key:              "",
		Value:            json.RawMessage("null"),
		Index:            DefaultPluginStorageIndex(),
		ExpectedRevision: 0,
	}
}
func (value *PluginTransactionOperationIndexPayload) UnmarshalJSON(data []byte) error {
	type plain PluginTransactionOperationIndexPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "value", "index", "expected_revision"}, true); err != nil {
		return err
	}
	*value = PluginTransactionOperationIndexPayload(decoded)
	return nil
}
func (value PluginTransactionOperationIndexPayload) MarshalJSON() ([]byte, error) {
	type plain PluginTransactionOperationIndexPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginTransactionOperationRemovePayload struct {
	Key              string `json:"key"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultPluginTransactionOperationRemovePayload() PluginTransactionOperationRemovePayload {
	return PluginTransactionOperationRemovePayload{
		Key:              "",
		ExpectedRevision: 0,
	}
}
func (value *PluginTransactionOperationRemovePayload) UnmarshalJSON(data []byte) error {
	type plain PluginTransactionOperationRemovePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "expected_revision"}, true); err != nil {
		return err
	}
	*value = PluginTransactionOperationRemovePayload(decoded)
	return nil
}
func (value PluginTransactionOperationRemovePayload) MarshalJSON() ([]byte, error) {
	type plain PluginTransactionOperationRemovePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginTransactionOperationConversationWritePayload struct {
	Key              string          `json:"key"`
	Value            json.RawMessage `json:"value"`
	ExpectedRevision uint64          `json:"expected_revision"`
}

func DefaultPluginTransactionOperationConversationWritePayload() PluginTransactionOperationConversationWritePayload {
	return PluginTransactionOperationConversationWritePayload{
		Key:              "",
		Value:            json.RawMessage("null"),
		ExpectedRevision: 0,
	}
}
func (value *PluginTransactionOperationConversationWritePayload) UnmarshalJSON(data []byte) error {
	type plain PluginTransactionOperationConversationWritePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "value", "expected_revision"}, true); err != nil {
		return err
	}
	*value = PluginTransactionOperationConversationWritePayload(decoded)
	return nil
}
func (value PluginTransactionOperationConversationWritePayload) MarshalJSON() ([]byte, error) {
	type plain PluginTransactionOperationConversationWritePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginTransactionOperationConversationRemovePayload struct {
	Key              string `json:"key"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultPluginTransactionOperationConversationRemovePayload() PluginTransactionOperationConversationRemovePayload {
	return PluginTransactionOperationConversationRemovePayload{
		Key:              "",
		ExpectedRevision: 0,
	}
}
func (value *PluginTransactionOperationConversationRemovePayload) UnmarshalJSON(data []byte) error {
	type plain PluginTransactionOperationConversationRemovePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "expected_revision"}, true); err != nil {
		return err
	}
	*value = PluginTransactionOperationConversationRemovePayload(decoded)
	return nil
}
func (value PluginTransactionOperationConversationRemovePayload) MarshalJSON() ([]byte, error) {
	type plain PluginTransactionOperationConversationRemovePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginTransactionOperationQueuePausePayload struct {
	Session          SessionID `json:"session"`
	ExpectedRevision uint64    `json:"expected_revision"`
	Paused           bool      `json:"paused"`
}

func DefaultPluginTransactionOperationQueuePausePayload() PluginTransactionOperationQueuePausePayload {
	return PluginTransactionOperationQueuePausePayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Paused:           false,
	}
}
func (value *PluginTransactionOperationQueuePausePayload) UnmarshalJSON(data []byte) error {
	type plain PluginTransactionOperationQueuePausePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "paused"}, true); err != nil {
		return err
	}
	*value = PluginTransactionOperationQueuePausePayload(decoded)
	return nil
}
func (value PluginTransactionOperationQueuePausePayload) MarshalJSON() ([]byte, error) {
	type plain PluginTransactionOperationQueuePausePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginTransactionOperationSubmitPayload struct {
	Session          SessionID         `json:"session"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Message          ConversationInput `json:"message"`
}

func DefaultPluginTransactionOperationSubmitPayload() PluginTransactionOperationSubmitPayload {
	return PluginTransactionOperationSubmitPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Message:          DefaultConversationInput(),
	}
}
func (value *PluginTransactionOperationSubmitPayload) UnmarshalJSON(data []byte) error {
	type plain PluginTransactionOperationSubmitPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "message"}, true); err != nil {
		return err
	}
	*value = PluginTransactionOperationSubmitPayload(decoded)
	return nil
}
func (value PluginTransactionOperationSubmitPayload) MarshalJSON() ([]byte, error) {
	type plain PluginTransactionOperationSubmitPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginTransactionOperationContinuePayload struct {
	Session          SessionID          `json:"session"`
	After            *TurnID            `json:"after"`
	Message          ConversationInput  `json:"message"`
	Key              string             `json:"key"`
	ExpectedRevision uint64             `json:"expected_revision"`
	Scope            PluginStorageScope `json:"scope"`
}

func DefaultPluginTransactionOperationContinuePayload() PluginTransactionOperationContinuePayload {
	return PluginTransactionOperationContinuePayload{
		Session:          SessionID{},
		After:            nil,
		Message:          DefaultConversationInput(),
		Key:              "",
		ExpectedRevision: 0,
		Scope:            PluginStorageScope(""),
	}
}
func (value *PluginTransactionOperationContinuePayload) UnmarshalJSON(data []byte) error {
	type plain PluginTransactionOperationContinuePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "message", "key", "expected_revision", "scope"}, true); err != nil {
		return err
	}
	*value = PluginTransactionOperationContinuePayload(decoded)
	return nil
}
func (value PluginTransactionOperationContinuePayload) MarshalJSON() ([]byte, error) {
	type plain PluginTransactionOperationContinuePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginTransactionOperationStopPayload struct {
	Turn TurnID `json:"turn"`
}

func DefaultPluginTransactionOperationStopPayload() PluginTransactionOperationStopPayload {
	return PluginTransactionOperationStopPayload{
		Turn: TurnID{},
	}
}
func (value *PluginTransactionOperationStopPayload) UnmarshalJSON(data []byte) error {
	type plain PluginTransactionOperationStopPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn"}, true); err != nil {
		return err
	}
	*value = PluginTransactionOperationStopPayload(decoded)
	return nil
}
func (value PluginTransactionOperationStopPayload) MarshalJSON() ([]byte, error) {
	type plain PluginTransactionOperationStopPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginTransactionOperationVariants = variantFactories{"write": func() any { return new(PluginTransactionOperationWritePayload) },
	"index":               func() any { return new(PluginTransactionOperationIndexPayload) },
	"remove":              func() any { return new(PluginTransactionOperationRemovePayload) },
	"conversation_write":  func() any { return new(PluginTransactionOperationConversationWritePayload) },
	"conversation_remove": func() any { return new(PluginTransactionOperationConversationRemovePayload) },
	"dispatch":            func() any { return new(DispatchCommand) },
	"notify":              func() any { return new(NotificationDraft) },
	"queue_pause":         func() any { return new(PluginTransactionOperationQueuePausePayload) },
	"submit":              func() any { return new(PluginTransactionOperationSubmitPayload) },
	"continue":            func() any { return new(PluginTransactionOperationContinuePayload) },
	"stop":                func() any { return new(PluginTransactionOperationStopPayload) }}

func (value PluginTransactionOperation) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, pluginTransactionOperationVariants)
}
func (value *PluginTransactionOperation) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", pluginTransactionOperationVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
