// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type RoleReference struct {
	ID       RoleID `json:"id"`
	Revision uint64 `json:"revision"`
}

func DefaultRoleReference() RoleReference {
	return RoleReference{
		ID:       RoleID{},
		Revision: 0,
	}
}
func (value *RoleReference) UnmarshalJSON(data []byte) error {
	type plain RoleReference
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "revision"}, false); err != nil {
		return err
	}
	*value = RoleReference(decoded)
	return nil
}
func (value RoleReference) MarshalJSON() ([]byte, error) {
	type plain RoleReference
	encoded := plain(value)
	return json.Marshal(encoded)
}

type RoleSnapshot struct {
	Execution    ConversationExecution  `json:"execution"`
	Profiles     []RoleProfile          `json:"profiles"`
	Providers    []ConversationProvider `json:"providers"`
	Team         *TeamReference         `json:"team,omitempty"`
	Instructions string                 `json:"instructions,omitempty"`
	Models       map[RoleID]RoleModel   `json:"models,omitempty"`
}

func DefaultRoleSnapshot() RoleSnapshot {
	return RoleSnapshot{
		Execution:    ConversationExecutionTeam,
		Profiles:     []RoleProfile{},
		Providers:    []ConversationProvider{},
		Team:         nil,
		Instructions: "",
		Models:       map[RoleID]RoleModel{},
	}
}
func (value *RoleSnapshot) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *RoleSnapshot) DecodeJSON(decoder *Decoder) error {
	type plain RoleSnapshot
	decoded := plain{}
	decoded.Execution = ConversationExecutionTeam
	decoded.Team = nil
	decoded.Instructions = ""
	decoded.Models = map[RoleID]RoleModel{}
	if err := decoder.Object(&decoded, []string{"profiles", "providers"}, false); err != nil {
		return err
	}
	*value = RoleSnapshot(decoded)
	return nil
}
func (value RoleSnapshot) MarshalJSON() ([]byte, error) {
	type plain RoleSnapshot
	encoded := plain(value)
	encoded.Profiles = normalizeCollections(encoded.Profiles)
	encoded.Providers = normalizeCollections(encoded.Providers)
	encoded.Models = normalizeCollections(encoded.Models)
	return json.Marshal(encoded)
}

type RoleProviderSecret struct {
	Provider    ProviderID `json:"provider"`
	Secret      Secret     `json:"secret"`
	ExpiresAtMs *uint64    `json:"expires_at_ms"`
}

func DefaultRoleProviderSecret() RoleProviderSecret {
	return RoleProviderSecret{
		Provider:    ProviderID{},
		Secret:      Secret{},
		ExpiresAtMs: nil,
	}
}
func (value *RoleProviderSecret) UnmarshalJSON(data []byte) error {
	type plain RoleProviderSecret
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "secret"}, false); err != nil {
		return err
	}
	*value = RoleProviderSecret(decoded)
	return nil
}
func (value RoleProviderSecret) MarshalJSON() ([]byte, error) {
	type plain RoleProviderSecret
	encoded := plain(value)
	return json.Marshal(encoded)
}

type RoleModel struct {
	Provider ProviderID         `json:"provider"`
	Model    string             `json:"model"`
	Effort   *Effort            `json:"effort"`
	Features *map[string]string `json:"features,omitempty"`
}

func DefaultRoleModel() RoleModel {
	return RoleModel{
		Provider: ProviderID{},
		Model:    "",
		Effort:   nil,
		Features: nil,
	}
}
func (value *RoleModel) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *RoleModel) DecodeJSON(decoder *Decoder) error {
	type plain RoleModel
	decoded := plain{}
	decoded.Features = nil
	if err := decoder.Object(&decoded, []string{"provider", "model"}, false); err != nil {
		return err
	}
	*value = RoleModel(decoded)
	return nil
}
func (value RoleModel) MarshalJSON() ([]byte, error) {
	type plain RoleModel
	encoded := plain(value)
	encoded.Features = normalizeCollections(encoded.Features)
	return json.Marshal(encoded)
}

type RoleProfile struct {
	ID           RoleID                    `json:"id"`
	Revision     uint64                    `json:"revision"`
	Key          string                    `json:"key"`
	Name         string                    `json:"name"`
	Appearance   *ProjectsAppearance       `json:"appearance"`
	Description  string                    `json:"description"`
	ReadOnly     bool                      `json:"read_only"`
	Model        *RoleModel                `json:"model"`
	MaxTurns     *uint32                   `json:"max_turns"`
	Skills       []string                  `json:"skills"`
	Tools        *[]PluginConversationTool `json:"tools,omitempty"`
	Instructions string                    `json:"instructions"`
}

func DefaultRoleProfile() RoleProfile {
	return RoleProfile{
		ID:           RoleID{},
		Revision:     0,
		Key:          "",
		Name:         "",
		Appearance:   nil,
		Description:  "",
		ReadOnly:     false,
		Model:        nil,
		MaxTurns:     nil,
		Skills:       []string{},
		Tools:        nil,
		Instructions: "",
	}
}
func (value *RoleProfile) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *RoleProfile) DecodeJSON(decoder *Decoder) error {
	type plain RoleProfile
	decoded := plain{}
	decoded.Tools = nil
	if err := decoder.Object(&decoded, []string{"id", "revision", "key", "name", "description", "read_only", "skills", "instructions"}, false); err != nil {
		return err
	}
	*value = RoleProfile(decoded)
	return nil
}
func (value RoleProfile) MarshalJSON() ([]byte, error) {
	type plain RoleProfile
	encoded := plain(value)
	encoded.Skills = normalizeCollections(encoded.Skills)
	encoded.Tools = normalizeCollections(encoded.Tools)
	return json.Marshal(encoded)
}
