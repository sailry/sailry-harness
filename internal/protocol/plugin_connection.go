// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PluginConnectionStatus struct {
	Package       PluginReference               `json:"package"`
	Card          string                        `json:"card"`
	Server        string                        `json:"server"`
	Configuration PluginConnectionConfiguration `json:"configuration"`
	Authorization PluginConnectionAuthorization `json:"authorization"`
	Credential    PluginConnectionCredential    `json:"credential"`
}

func DefaultPluginConnectionStatus() PluginConnectionStatus {
	return PluginConnectionStatus{
		Package:       DefaultPluginReference(),
		Card:          "",
		Server:        "",
		Configuration: PluginConnectionConfiguration(""),
		Authorization: PluginConnectionAuthorization(""),
		Credential:    PluginConnectionCredential(""),
	}
}
func (value *PluginConnectionStatus) UnmarshalJSON(data []byte) error {
	type plain PluginConnectionStatus
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "card", "server", "configuration", "authorization", "credential"}, false); err != nil {
		return err
	}
	*value = PluginConnectionStatus(decoded)
	return nil
}
func (value PluginConnectionStatus) MarshalJSON() ([]byte, error) {
	type plain PluginConnectionStatus
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectionConfiguration string

const (
	PluginConnectionConfigurationReady       PluginConnectionConfiguration = "ready"
	PluginConnectionConfigurationMissing     PluginConnectionConfiguration = "missing"
	PluginConnectionConfigurationChanged     PluginConnectionConfiguration = "changed"
	PluginConnectionConfigurationUnsupported PluginConnectionConfiguration = "unsupported"
)

func (value PluginConnectionConfiguration) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginConnectionConfiguration) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginConnectionConfiguration(decoded) {
	case PluginConnectionConfigurationReady, PluginConnectionConfigurationMissing, PluginConnectionConfigurationChanged, PluginConnectionConfigurationUnsupported:
		*value = PluginConnectionConfiguration(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginConnectionConfiguration) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginConnectionConfiguration) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginConnectionAuthorization string

const (
	PluginConnectionAuthorizationNotDeclared PluginConnectionAuthorization = "not_declared"
	PluginConnectionAuthorizationMissing     PluginConnectionAuthorization = "missing"
	PluginConnectionAuthorizationConfigured  PluginConnectionAuthorization = "configured"
)

func (value PluginConnectionAuthorization) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginConnectionAuthorization) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginConnectionAuthorization(decoded) {
	case PluginConnectionAuthorizationNotDeclared, PluginConnectionAuthorizationMissing, PluginConnectionAuthorizationConfigured:
		*value = PluginConnectionAuthorization(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginConnectionAuthorization) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginConnectionAuthorization) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginConnectionCredential string

const (
	PluginConnectionCredentialNotDeclared PluginConnectionCredential = "not_declared"
	PluginConnectionCredentialAbsent      PluginConnectionCredential = "absent"
	PluginConnectionCredentialPresent     PluginConnectionCredential = "present"
)

func (value PluginConnectionCredential) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginConnectionCredential) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginConnectionCredential(decoded) {
	case PluginConnectionCredentialNotDeclared, PluginConnectionCredentialAbsent, PluginConnectionCredentialPresent:
		*value = PluginConnectionCredential(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginConnectionCredential) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginConnectionCredential) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
