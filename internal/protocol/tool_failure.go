// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type ToolFailureOrigin string

const (
	ToolFailureOriginRuntime ToolFailureOrigin = "runtime"
	ToolFailureOriginTool    ToolFailureOrigin = "tool"
)

func (value ToolFailureOrigin) MarshalJSON() ([]byte, error) {
	switch value {
	case ToolFailureOriginRuntime, ToolFailureOriginTool:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ToolFailureOrigin %q", value)
	}
}
func (value *ToolFailureOrigin) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ToolFailureOrigin(decoded) {
	case ToolFailureOriginRuntime, ToolFailureOriginTool:
		*value = ToolFailureOrigin(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ToolFailureOrigin %q", decoded)
	}
}
func (value ToolFailureOrigin) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolFailureOrigin) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ToolFailure struct {
	Code   *ErrorCode        `json:"code,omitempty"`
	Origin ToolFailureOrigin `json:"origin"`
}

func DefaultToolFailure() ToolFailure {
	return ToolFailure{
		Code:   nil,
		Origin: ToolFailureOriginRuntime,
	}
}
func (value *ToolFailure) UnmarshalJSON(data []byte) error {
	type plain ToolFailure
	decoded := plain{}
	decoded.Code = nil
	decoded.Origin = ToolFailureOriginRuntime
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ToolFailure(decoded)
	return nil
}
func (value ToolFailure) MarshalJSON() ([]byte, error) {
	type plain ToolFailure
	encoded := plain(value)
	return json.Marshal(encoded)
}
