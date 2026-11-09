package model

import "encoding/json"

func (value callParameters) MarshalJSON() ([]byte, error) {
	type plain callParameters
	return json.Marshal(plain(value))
}
func (value *callParameters) UnmarshalJSON(data []byte) error {
	type plain callParameters
	return json.Unmarshal(data, (*plain)(value))
}
func (value responseMetadata) MarshalJSON() ([]byte, error) {
	type plain responseMetadata
	return json.Marshal(plain(value))
}
func (value *responseMetadata) UnmarshalJSON(data []byte) error {
	type plain responseMetadata
	return json.Unmarshal(data, (*plain)(value))
}
func (value indexMetadata) MarshalJSON() ([]byte, error) {
	type plain indexMetadata
	return json.Marshal(plain(value))
}
func (value *indexMetadata) UnmarshalJSON(data []byte) error {
	type plain indexMetadata
	return json.Unmarshal(data, (*plain)(value))
}
