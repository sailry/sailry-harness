// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PluginModesDeclaration struct {
	ID           string                  `json:"id"`
	Option       string                  `json:"option"`
	Display      PluginDesktopNavigation `json:"display"`
	Permissions  PluginModesPermissions  `json:"permissions"`
	Instructions string                  `json:"instructions"`
}

func DefaultPluginModesDeclaration() PluginModesDeclaration {
	return PluginModesDeclaration{
		ID:           "",
		Option:       "",
		Display:      DefaultPluginDesktopNavigation(),
		Permissions:  DefaultPluginModesPermissions(),
		Instructions: "",
	}
}
func (value *PluginModesDeclaration) UnmarshalJSON(data []byte) error {
	type plain PluginModesDeclaration
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "option", "display", "permissions", "instructions"}, true); err != nil {
		return err
	}
	*value = PluginModesDeclaration(decoded)
	return nil
}
func (value PluginModesDeclaration) MarshalJSON() ([]byte, error) {
	type plain PluginModesDeclaration
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModesPermissions struct {
	Write bool `json:"write"`
}

func DefaultPluginModesPermissions() PluginModesPermissions {
	return PluginModesPermissions{
		Write: false,
	}
}
func (value *PluginModesPermissions) UnmarshalJSON(data []byte) error {
	type plain PluginModesPermissions
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"write"}, true); err != nil {
		return err
	}
	*value = PluginModesPermissions(decoded)
	return nil
}
func (value PluginModesPermissions) MarshalJSON() ([]byte, error) {
	type plain PluginModesPermissions
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModesSelection struct {
	Package PluginReference `json:"package"`
	ID      string          `json:"id"`
}

func DefaultPluginModesSelection() PluginModesSelection {
	return PluginModesSelection{
		Package: DefaultPluginReference(),
		ID:      "",
	}
}
func (value *PluginModesSelection) UnmarshalJSON(data []byte) error {
	type plain PluginModesSelection
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "id"}, true); err != nil {
		return err
	}
	*value = PluginModesSelection(decoded)
	return nil
}
func (value PluginModesSelection) MarshalJSON() ([]byte, error) {
	type plain PluginModesSelection
	encoded := plain(value)
	return json.Marshal(encoded)
}
