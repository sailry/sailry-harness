// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type ToolDisplayProjectionTable struct {
	Columns   ToolDisplayProjectionReference  `json:"columns"`
	Rows      ToolDisplayProjectionReference  `json:"rows"`
	Truncated *ToolDisplayProjectionReference `json:"truncated"`
	Cell      ToolDisplayProjectionTableCell  `json:"cell"`
}

func DefaultToolDisplayProjectionTable() ToolDisplayProjectionTable {
	return ToolDisplayProjectionTable{
		Columns:   DefaultToolDisplayProjectionReference(),
		Rows:      DefaultToolDisplayProjectionReference(),
		Truncated: nil,
		Cell:      DefaultToolDisplayProjectionTableCell(),
	}
}
func (value *ToolDisplayProjectionTable) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionTable
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"columns", "rows", "cell"}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionTable(decoded)
	return nil
}
func (value ToolDisplayProjectionTable) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionTable
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionTableCell struct {
	Value   string                           `json:"value"`
	Missing string                           `json:"missing"`
	Bytes   *ToolDisplayProjectionTableBytes `json:"bytes"`
}

func DefaultToolDisplayProjectionTableCell() ToolDisplayProjectionTableCell {
	return ToolDisplayProjectionTableCell{
		Value:   "",
		Missing: "",
		Bytes:   nil,
	}
}
func (value *ToolDisplayProjectionTableCell) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionTableCell
	decoded := plain{}
	decoded.Value = ""
	decoded.Missing = ""
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionTableCell(decoded)
	return nil
}
func (value ToolDisplayProjectionTableCell) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionTableCell
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionTableBytes string

const (
	ToolDisplayProjectionTableBytesHex ToolDisplayProjectionTableBytes = "hex"
)

func (value ToolDisplayProjectionTableBytes) MarshalJSON() ([]byte, error) {
	switch value {
	case ToolDisplayProjectionTableBytesHex:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ToolDisplayProjectionTableBytes %q", value)
	}
}
func (value *ToolDisplayProjectionTableBytes) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ToolDisplayProjectionTableBytes(decoded) {
	case ToolDisplayProjectionTableBytesHex:
		*value = ToolDisplayProjectionTableBytes(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ToolDisplayProjectionTableBytes %q", decoded)
	}
}
func (value ToolDisplayProjectionTableBytes) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolDisplayProjectionTableBytes) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
