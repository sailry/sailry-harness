// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type TeamSettings struct {
	Revision         uint64 `json:"revision"`
	ChildConcurrency uint64 `json:"child_concurrency"`
}

func DefaultTeamSettings() TeamSettings {
	return TeamSettings{
		Revision:         0,
		ChildConcurrency: 8,
	}
}
func (value *TeamSettings) UnmarshalJSON(data []byte) error {
	type plain TeamSettings
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "child_concurrency"}, true); err != nil {
		return err
	}
	*value = TeamSettings(decoded)
	return nil
}
func (value TeamSettings) MarshalJSON() ([]byte, error) {
	type plain TeamSettings
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TeamReference struct {
	ID       TeamID `json:"id"`
	Revision uint64 `json:"revision"`
}

func DefaultTeamReference() TeamReference {
	return TeamReference{
		ID:       TeamID{},
		Revision: 0,
	}
}
func (value *TeamReference) UnmarshalJSON(data []byte) error {
	type plain TeamReference
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "revision"}, false); err != nil {
		return err
	}
	*value = TeamReference(decoded)
	return nil
}
func (value TeamReference) MarshalJSON() ([]byte, error) {
	type plain TeamReference
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TeamSelection struct {
	ID       TeamID  `json:"id"`
	Revision *uint64 `json:"revision"`
}

func DefaultTeamSelection() TeamSelection {
	return TeamSelection{
		ID:       TeamID{},
		Revision: nil,
	}
}
func (value *TeamSelection) UnmarshalJSON(data []byte) error {
	type plain TeamSelection
	decoded := plain{}
	decoded.Revision = nil
	if err := DecodeObject(data, &decoded, []string{"id"}, true); err != nil {
		return err
	}
	*value = TeamSelection(decoded)
	return nil
}
func (value TeamSelection) MarshalJSON() ([]byte, error) {
	type plain TeamSelection
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TeamProfile struct {
	ID           TeamID              `json:"id"`
	Revision     uint64              `json:"revision"`
	Key          string              `json:"key"`
	Name         string              `json:"name"`
	Appearance   *ProjectsAppearance `json:"appearance"`
	Description  string              `json:"description"`
	Model        *RoleModel          `json:"model"`
	Instructions string              `json:"instructions"`
	Members      []RoleID            `json:"members"`
}

func DefaultTeamProfile() TeamProfile {
	return TeamProfile{
		ID:           TeamID{},
		Revision:     0,
		Key:          "",
		Name:         "",
		Appearance:   nil,
		Description:  "",
		Model:        nil,
		Instructions: "",
		Members:      []RoleID{},
	}
}
func (value *TeamProfile) UnmarshalJSON(data []byte) error {
	type plain TeamProfile
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "revision", "key", "name", "description", "instructions", "members"}, false); err != nil {
		return err
	}
	*value = TeamProfile(decoded)
	return nil
}
func (value TeamProfile) MarshalJSON() ([]byte, error) {
	type plain TeamProfile
	encoded := plain(value)
	encoded.Members = normalizeCollections(encoded.Members)
	return json.Marshal(encoded)
}
