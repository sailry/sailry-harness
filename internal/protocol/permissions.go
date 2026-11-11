// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PermissionsKind string

const (
	PermissionsKindFullDisk        PermissionsKind = "full_disk"
	PermissionsKindScreenRecording PermissionsKind = "screen_recording"
	PermissionsKindAccessibility   PermissionsKind = "accessibility"
	PermissionsKindMicrophone      PermissionsKind = "microphone"
)

func (value PermissionsKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PermissionsKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PermissionsKind(decoded) {
	case PermissionsKindFullDisk, PermissionsKindScreenRecording, PermissionsKindAccessibility, PermissionsKindMicrophone:
		*value = PermissionsKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PermissionsKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PermissionsKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PermissionsIdentity struct {
	Device  NodeID          `json:"device"`
	Subject string          `json:"subject"`
	Kind    PermissionsKind `json:"kind"`
}

func DefaultPermissionsIdentity() PermissionsIdentity {
	return PermissionsIdentity{
		Device:  NodeID{},
		Subject: "",
		Kind:    PermissionsKind(""),
	}
}
func (value *PermissionsIdentity) UnmarshalJSON(data []byte) error {
	type plain PermissionsIdentity
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"device", "subject", "kind"}, false); err != nil {
		return err
	}
	*value = PermissionsIdentity(decoded)
	return nil
}
func (value PermissionsIdentity) MarshalJSON() ([]byte, error) {
	type plain PermissionsIdentity
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PermissionsContribution struct {
	Kind         PermissionsKind `json:"kind"`
	Subject      string          `json:"subject"`
	Check        *string         `json:"check"`
	Request      *string         `json:"request"`
	RequestGroup *string         `json:"request_group"`
	Actions      []PluginAction  `json:"actions"`
}

func DefaultPermissionsContribution() PermissionsContribution {
	return PermissionsContribution{
		Kind:         PermissionsKind(""),
		Subject:      "",
		Check:        nil,
		Request:      nil,
		RequestGroup: nil,
		Actions:      []PluginAction{},
	}
}
func (value *PermissionsContribution) UnmarshalJSON(data []byte) error {
	type plain PermissionsContribution
	decoded := plain{}
	decoded.Actions = []PluginAction{}
	if err := DecodeObject(data, &decoded, []string{"kind", "subject"}, true); err != nil {
		return err
	}
	*value = PermissionsContribution(decoded)
	return nil
}
func (value PermissionsContribution) MarshalJSON() ([]byte, error) {
	type plain PermissionsContribution
	encoded := plain(value)
	encoded.Actions = normalizeCollections(encoded.Actions)
	return json.Marshal(encoded)
}

type PermissionsGrant struct {
	Kind       PermissionsKind `json:"kind"`
	Authorized *bool           `json:"authorized"`
}

func DefaultPermissionsGrant() PermissionsGrant {
	return PermissionsGrant{
		Kind:       PermissionsKind(""),
		Authorized: nil,
	}
}
func (value *PermissionsGrant) UnmarshalJSON(data []byte) error {
	type plain PermissionsGrant
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind"}, false); err != nil {
		return err
	}
	*value = PermissionsGrant(decoded)
	return nil
}
func (value PermissionsGrant) MarshalJSON() ([]byte, error) {
	type plain PermissionsGrant
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PermissionsStatus struct {
	Grants     []PermissionsGrant `json:"grants"`
	CanRequest bool               `json:"can_request"`
}

func DefaultPermissionsStatus() PermissionsStatus {
	return PermissionsStatus{
		Grants:     []PermissionsGrant{},
		CanRequest: false,
	}
}
func (value *PermissionsStatus) UnmarshalJSON(data []byte) error {
	type plain PermissionsStatus
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"grants", "can_request"}, false); err != nil {
		return err
	}
	*value = PermissionsStatus(decoded)
	return nil
}
func (value PermissionsStatus) MarshalJSON() ([]byte, error) {
	type plain PermissionsStatus
	encoded := plain(value)
	encoded.Grants = normalizeCollections(encoded.Grants)
	return json.Marshal(encoded)
}
