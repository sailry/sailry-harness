// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
	"fmt"
	"reflect"
)

type PluginDesktopIcon struct {
	Kind string
	Data any
}
type PluginDesktopIconSvgPayload struct {
	Svg string `json:"svg"`
}

func DefaultPluginDesktopIconSvgPayload() PluginDesktopIconSvgPayload {
	return PluginDesktopIconSvgPayload{
		Svg: "",
	}
}
func (value *PluginDesktopIconSvgPayload) UnmarshalJSON(data []byte) error {
	type plain PluginDesktopIconSvgPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"svg"}, true); err != nil {
		return err
	}
	*value = PluginDesktopIconSvgPayload(decoded)
	return nil
}
func (value PluginDesktopIconSvgPayload) MarshalJSON() ([]byte, error) {
	type plain PluginDesktopIconSvgPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

func (value PluginDesktopIcon) MarshalJSON() ([]byte, error) {
	switch value.Kind {
	case "Name", "Svg":
		return json.Marshal(value.Data)
	default:
		return nil, fmt.Errorf("unknown PluginDesktopIcon variant %q", value.Kind)
	}
}
func (value *PluginDesktopIcon) UnmarshalJSON(data []byte) error {
	{
		var decoded string
		if err := checkJSONNulls(data, reflect.TypeFor[string]()); err == nil {
			if err := json.Unmarshal(data, &decoded); err == nil {
				value.Kind = "Name"
				value.Data = &decoded
				return nil
			}
		}
	}
	{
		var decoded PluginDesktopIconSvgPayload
		if err := checkJSONNulls(data, reflect.TypeFor[PluginDesktopIconSvgPayload]()); err == nil {
			if err := json.Unmarshal(data, &decoded); err == nil {
				value.Kind = "Svg"
				value.Data = &decoded
				return nil
			}
		}
	}
	return fmt.Errorf("unrecognized PluginDesktopIcon shape")
}
