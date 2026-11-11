// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ComputerPermission string

const (
	ComputerPermissionScreenCapture ComputerPermission = "screen_capture"
	ComputerPermissionAccessibility ComputerPermission = "accessibility"
)

func (value ComputerPermission) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ComputerPermission) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ComputerPermission(decoded) {
	case ComputerPermissionScreenCapture, ComputerPermissionAccessibility:
		*value = ComputerPermission(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ComputerPermission) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ComputerPermission) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ComputerPermissions struct {
	Node          NodeID `json:"node"`
	Platform      string `json:"platform"`
	Local         bool   `json:"local"`
	ScreenCapture *bool  `json:"screen_capture"`
	Accessibility *bool  `json:"accessibility"`
}

func DefaultComputerPermissions() ComputerPermissions {
	return ComputerPermissions{
		Node:          NodeID{},
		Platform:      "",
		Local:         false,
		ScreenCapture: nil,
		Accessibility: nil,
	}
}
func (value *ComputerPermissions) UnmarshalJSON(data []byte) error {
	type plain ComputerPermissions
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "platform", "local"}, false); err != nil {
		return err
	}
	*value = ComputerPermissions(decoded)
	return nil
}
func (value ComputerPermissions) MarshalJSON() ([]byte, error) {
	type plain ComputerPermissions
	encoded := plain(value)
	return json.Marshal(encoded)
}
