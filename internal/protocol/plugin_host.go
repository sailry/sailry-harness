// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type PluginHostManifest struct {
	Entry     string              `json:"entry"`
	Resources []string            `json:"resources"`
	Handlers  []string            `json:"handlers"`
	ReadOnly  []string            `json:"read_only,omitempty"`
	Turn      *PluginHostTurn     `json:"turn,omitempty"`
	Commands  []PluginHostCommand `json:"commands,omitempty"`
	Status    *PluginHostStatus   `json:"status,omitempty"`
}

func DefaultPluginHostManifest() PluginHostManifest {
	return PluginHostManifest{
		Entry:     "",
		Resources: []string{},
		Handlers:  []string{},
		ReadOnly:  []string{},
		Turn:      nil,
		Commands:  []PluginHostCommand{},
		Status:    nil,
	}
}
func (value *PluginHostManifest) UnmarshalJSON(data []byte) error {
	type plain PluginHostManifest
	decoded := plain{}
	decoded.ReadOnly = []string{}
	decoded.Turn = nil
	decoded.Commands = []PluginHostCommand{}
	decoded.Status = nil
	if err := DecodeObject(data, &decoded, []string{"entry", "resources", "handlers"}, true); err != nil {
		return err
	}
	*value = PluginHostManifest(decoded)
	return nil
}
func (value PluginHostManifest) MarshalJSON() ([]byte, error) {
	type plain PluginHostManifest
	encoded := plain(value)
	encoded.Resources = normalizeCollections(encoded.Resources)
	encoded.Handlers = normalizeCollections(encoded.Handlers)
	encoded.ReadOnly = normalizeCollections(encoded.ReadOnly)
	encoded.Commands = normalizeCollections(encoded.Commands)
	return json.Marshal(encoded)
}

type PluginHostStatus struct {
	Read    string             `json:"read"`
	Invoke  *string            `json:"invoke,omitempty"`
	Changes []PluginHostChange `json:"changes,omitempty"`
}

func DefaultPluginHostStatus() PluginHostStatus {
	return PluginHostStatus{
		Read:    "",
		Invoke:  nil,
		Changes: []PluginHostChange{},
	}
}
func (value *PluginHostStatus) UnmarshalJSON(data []byte) error {
	type plain PluginHostStatus
	decoded := plain{}
	decoded.Invoke = nil
	decoded.Changes = []PluginHostChange{}
	if err := DecodeObject(data, &decoded, []string{"read"}, true); err != nil {
		return err
	}
	*value = PluginHostStatus(decoded)
	return nil
}
func (value PluginHostStatus) MarshalJSON() ([]byte, error) {
	type plain PluginHostStatus
	encoded := plain(value)
	encoded.Changes = normalizeCollections(encoded.Changes)
	return json.Marshal(encoded)
}

type PluginHostChange string

const (
	PluginHostChangePackage      PluginHostChange = "package"
	PluginHostChangeConversation PluginHostChange = "conversation"
	PluginHostChangeRepository   PluginHostChange = "repository"
)

func (value PluginHostChange) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginHostChange) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginHostChange(decoded) {
	case PluginHostChangePackage, PluginHostChangeConversation, PluginHostChangeRepository:
		*value = PluginHostChange(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginHostChange) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginHostChange) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginHostCommand struct {
	Name    string `json:"name"`
	Handler string `json:"handler"`
}

func DefaultPluginHostCommand() PluginHostCommand {
	return PluginHostCommand{
		Name:    "",
		Handler: "",
	}
}
func (value *PluginHostCommand) UnmarshalJSON(data []byte) error {
	type plain PluginHostCommand
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "handler"}, true); err != nil {
		return err
	}
	*value = PluginHostCommand(decoded)
	return nil
}
func (value PluginHostCommand) MarshalJSON() ([]byte, error) {
	type plain PluginHostCommand
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginHostTurn struct {
	Initialize  string  `json:"initialize"`
	BeforeModel *string `json:"before_model"`
	AfterTurn   *string `json:"after_turn,omitempty"`
}

func DefaultPluginHostTurn() PluginHostTurn {
	return PluginHostTurn{
		Initialize:  "",
		BeforeModel: nil,
		AfterTurn:   nil,
	}
}
func (value *PluginHostTurn) UnmarshalJSON(data []byte) error {
	type plain PluginHostTurn
	decoded := plain{}
	decoded.AfterTurn = nil
	if err := DecodeObject(data, &decoded, []string{"initialize"}, true); err != nil {
		return err
	}
	*value = PluginHostTurn(decoded)
	return nil
}
func (value PluginHostTurn) MarshalJSON() ([]byte, error) {
	type plain PluginHostTurn
	encoded := plain(value)
	return json.Marshal(encoded)
}
