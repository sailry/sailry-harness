// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PowerSettings struct {
	Revision     uint64 `json:"revision"`
	PreventSleep bool   `json:"prevent_sleep"`
}

func DefaultPowerSettings() PowerSettings {
	return PowerSettings{
		Revision:     0,
		PreventSleep: true,
	}
}
func (value *PowerSettings) UnmarshalJSON(data []byte) error {
	type plain PowerSettings
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "prevent_sleep"}, true); err != nil {
		return err
	}
	*value = PowerSettings(decoded)
	return nil
}
func (value PowerSettings) MarshalJSON() ([]byte, error) {
	type plain PowerSettings
	encoded := plain(value)
	return json.Marshal(encoded)
}
