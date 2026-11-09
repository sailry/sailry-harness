// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type WslDistribution struct {
	Name    string `json:"name"`
	Version uint32 `json:"version"`
}

func DefaultWslDistribution() WslDistribution {
	return WslDistribution{
		Name:    "",
		Version: 0,
	}
}
func (value *WslDistribution) UnmarshalJSON(data []byte) error {
	type plain WslDistribution
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "version"}, false); err != nil {
		return err
	}
	*value = WslDistribution(decoded)
	return nil
}
func (value WslDistribution) MarshalJSON() ([]byte, error) {
	type plain WslDistribution
	encoded := plain(value)
	return json.Marshal(encoded)
}
