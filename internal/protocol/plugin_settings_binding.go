// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type PluginSettingsBinding struct {
	Server      *string `json:"server,omitempty"`
	Env         *string `json:"env"`
	Header      *string `json:"header"`
	Origin      *string `json:"origin,omitempty"`
	Prefix      *string `json:"prefix,omitempty"`
	Unsupported bool    `json:"unsupported,omitempty"`
}

func DefaultPluginSettingsBinding() PluginSettingsBinding {
	return PluginSettingsBinding{
		Server:      nil,
		Env:         nil,
		Header:      nil,
		Origin:      nil,
		Prefix:      nil,
		Unsupported: false,
	}
}
func (value *PluginSettingsBinding) UnmarshalJSON(data []byte) error {
	type plain PluginSettingsBinding
	decoded := plain{}
	if err := DecodeObject(data, &decoded, nil, false); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	for name := range fields {
		switch name {
		case "server", "env", "header", "origin", "prefix", "unsupported":
		default:
			decoded.Unsupported = true
		}
	}
	*value = PluginSettingsBinding(decoded)
	return nil
}
func (value PluginSettingsBinding) MarshalJSON() ([]byte, error) {
	type plain PluginSettingsBinding
	encoded := plain(value)
	return json.Marshal(encoded)
}
