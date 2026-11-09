// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationCompactionSettings struct {
	Revision uint64 `json:"revision"`
	Percent  uint8  `json:"percent"`
}

func DefaultConversationCompactionSettings() ConversationCompactionSettings {
	return ConversationCompactionSettings{
		Revision: 0,
		Percent:  80,
	}
}
func (value *ConversationCompactionSettings) UnmarshalJSON(data []byte) error {
	type plain ConversationCompactionSettings
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "percent"}, false); err != nil {
		return err
	}
	*value = ConversationCompactionSettings(decoded)
	return nil
}
func (value ConversationCompactionSettings) MarshalJSON() ([]byte, error) {
	type plain ConversationCompactionSettings
	encoded := plain(value)
	return json.Marshal(encoded)
}
