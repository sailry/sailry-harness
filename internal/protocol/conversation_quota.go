// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationQuotaWindow struct {
	UsedBasisPoints uint32  `json:"used_basis_points"`
	ResetAtMs       *uint64 `json:"reset_at_ms"`
}

func DefaultConversationQuotaWindow() ConversationQuotaWindow {
	return ConversationQuotaWindow{
		UsedBasisPoints: 0,
		ResetAtMs:       nil,
	}
}
func (value *ConversationQuotaWindow) UnmarshalJSON(data []byte) error {
	type plain ConversationQuotaWindow
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"used_basis_points"}, false); err != nil {
		return err
	}
	*value = ConversationQuotaWindow(decoded)
	return nil
}
func (value ConversationQuotaWindow) MarshalJSON() ([]byte, error) {
	type plain ConversationQuotaWindow
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationQuotaSnapshot struct {
	Provider   ProviderID               `json:"provider"`
	Credential CredentialRef            `json:"credential"`
	Revision   uint64                   `json:"revision"`
	ObservedMs uint64                   `json:"observed_ms"`
	FiveHour   *ConversationQuotaWindow `json:"five_hour"`
	SevenDay   *ConversationQuotaWindow `json:"seven_day"`
}

func DefaultConversationQuotaSnapshot() ConversationQuotaSnapshot {
	return ConversationQuotaSnapshot{
		Provider:   ProviderID{},
		Credential: DefaultCredentialRef(),
		Revision:   0,
		ObservedMs: 0,
		FiveHour:   nil,
		SevenDay:   nil,
	}
}
func (value *ConversationQuotaSnapshot) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationQuotaSnapshot) DecodeJSON(decoder *Decoder) error {
	type plain ConversationQuotaSnapshot
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"provider", "credential", "revision", "observed_ms"}, false); err != nil {
		return err
	}
	*value = ConversationQuotaSnapshot(decoded)
	return nil
}
func (value ConversationQuotaSnapshot) MarshalJSON() ([]byte, error) {
	type plain ConversationQuotaSnapshot
	encoded := plain(value)
	return json.Marshal(encoded)
}
