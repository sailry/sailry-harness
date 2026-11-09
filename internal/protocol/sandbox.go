// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type SandboxSettings struct {
	Revision uint64 `json:"revision"`
	Enabled  bool   `json:"enabled"`
}

func DefaultSandboxSettings() SandboxSettings {
	return SandboxSettings{
		Revision: 0,
		Enabled:  true,
	}
}
func (value *SandboxSettings) UnmarshalJSON(data []byte) error {
	type plain SandboxSettings
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "enabled"}, true); err != nil {
		return err
	}
	*value = SandboxSettings(decoded)
	return nil
}
func (value SandboxSettings) MarshalJSON() ([]byte, error) {
	type plain SandboxSettings
	encoded := plain(value)
	return json.Marshal(encoded)
}
