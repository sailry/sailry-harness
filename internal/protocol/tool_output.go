// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ToolOutputOutputReference struct {
	Call     string `json:"call"`
	Revision string `json:"revision"`
}

func DefaultToolOutputOutputReference() ToolOutputOutputReference {
	return ToolOutputOutputReference{
		Call:     "",
		Revision: "",
	}
}
func (value *ToolOutputOutputReference) UnmarshalJSON(data []byte) error {
	type plain ToolOutputOutputReference
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"call", "revision"}, true); err != nil {
		return err
	}
	*value = ToolOutputOutputReference(decoded)
	return nil
}
func (value ToolOutputOutputReference) MarshalJSON() ([]byte, error) {
	type plain ToolOutputOutputReference
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolOutputOutputPage struct {
	Turn       TurnID                    `json:"turn"`
	Output     ToolOutputOutputReference `json:"output"`
	Offset     uint64                    `json:"offset"`
	TotalBytes uint64                    `json:"total_bytes"`
	NextOffset *uint64                   `json:"next_offset"`
	Text       string                    `json:"text"`
}

func DefaultToolOutputOutputPage() ToolOutputOutputPage {
	return ToolOutputOutputPage{
		Turn:       TurnID{},
		Output:     DefaultToolOutputOutputReference(),
		Offset:     0,
		TotalBytes: 0,
		NextOffset: nil,
		Text:       "",
	}
}
func (value *ToolOutputOutputPage) UnmarshalJSON(data []byte) error {
	type plain ToolOutputOutputPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "output", "offset", "total_bytes", "text"}, false); err != nil {
		return err
	}
	*value = ToolOutputOutputPage(decoded)
	return nil
}
func (value ToolOutputOutputPage) MarshalJSON() ([]byte, error) {
	type plain ToolOutputOutputPage
	encoded := plain(value)
	return json.Marshal(encoded)
}
