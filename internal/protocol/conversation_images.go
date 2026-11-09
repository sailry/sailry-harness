// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationImagesReference struct {
	Call  string `json:"call"`
	Index uint64 `json:"index"`
}

func DefaultConversationImagesReference() ConversationImagesReference {
	return ConversationImagesReference{
		Call:  "",
		Index: 0,
	}
}
func (value *ConversationImagesReference) UnmarshalJSON(data []byte) error {
	type plain ConversationImagesReference
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"call", "index"}, false); err != nil {
		return err
	}
	*value = ConversationImagesReference(decoded)
	return nil
}
func (value ConversationImagesReference) MarshalJSON() ([]byte, error) {
	type plain ConversationImagesReference
	encoded := plain(value)
	return json.Marshal(encoded)
}
