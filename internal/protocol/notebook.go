// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type NotebookRead struct {
	Path   string `json:"path"`
	Offset uint64 `json:"offset"`
	Limit  uint64 `json:"limit"`
}

func DefaultNotebookRead() NotebookRead {
	return NotebookRead{
		Path:   "",
		Offset: 0,
		Limit:  0,
	}
}
func (value *NotebookRead) UnmarshalJSON(data []byte) error {
	type plain NotebookRead
	decoded := plain{}
	decoded.Offset = 0
	decoded.Limit = 20
	if err := DecodeObject(data, &decoded, []string{"path"}, true); err != nil {
		return err
	}
	*value = NotebookRead(decoded)
	return nil
}
func (value NotebookRead) MarshalJSON() ([]byte, error) {
	type plain NotebookRead
	encoded := plain(value)
	return json.Marshal(encoded)
}

type NotebookEdit struct {
	Path             string           `json:"path"`
	ExpectedRevision string           `json:"expected_revision"`
	Edits            []NotebookChange `json:"edits"`
}

func DefaultNotebookEdit() NotebookEdit {
	return NotebookEdit{
		Path:             "",
		ExpectedRevision: "",
		Edits:            []NotebookChange{},
	}
}
func (value *NotebookEdit) UnmarshalJSON(data []byte) error {
	type plain NotebookEdit
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "expected_revision", "edits"}, true); err != nil {
		return err
	}
	*value = NotebookEdit(decoded)
	return nil
}
func (value NotebookEdit) MarshalJSON() ([]byte, error) {
	type plain NotebookEdit
	encoded := plain(value)
	encoded.Edits = normalizeCollections(encoded.Edits)
	return json.Marshal(encoded)
}

type NotebookChange struct {
	Kind string
	Data any
}
type NotebookChangeReplacePayload struct {
	Index  uint64 `json:"index"`
	Source string `json:"source"`
}

func DefaultNotebookChangeReplacePayload() NotebookChangeReplacePayload {
	return NotebookChangeReplacePayload{
		Index:  0,
		Source: "",
	}
}
func (value *NotebookChangeReplacePayload) UnmarshalJSON(data []byte) error {
	type plain NotebookChangeReplacePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"index", "source"}, true); err != nil {
		return err
	}
	*value = NotebookChangeReplacePayload(decoded)
	return nil
}
func (value NotebookChangeReplacePayload) MarshalJSON() ([]byte, error) {
	type plain NotebookChangeReplacePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type NotebookChangeInsertPayload struct {
	Index    uint64       `json:"index"`
	CellType NotebookKind `json:"cell_type"`
	Source   string       `json:"source"`
}

func DefaultNotebookChangeInsertPayload() NotebookChangeInsertPayload {
	return NotebookChangeInsertPayload{
		Index:    0,
		CellType: NotebookKind(""),
		Source:   "",
	}
}
func (value *NotebookChangeInsertPayload) UnmarshalJSON(data []byte) error {
	type plain NotebookChangeInsertPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"index", "cell_type", "source"}, true); err != nil {
		return err
	}
	*value = NotebookChangeInsertPayload(decoded)
	return nil
}
func (value NotebookChangeInsertPayload) MarshalJSON() ([]byte, error) {
	type plain NotebookChangeInsertPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type NotebookChangeDeletePayload struct {
	Index uint64 `json:"index"`
}

func DefaultNotebookChangeDeletePayload() NotebookChangeDeletePayload {
	return NotebookChangeDeletePayload{
		Index: 0,
	}
}
func (value *NotebookChangeDeletePayload) UnmarshalJSON(data []byte) error {
	type plain NotebookChangeDeletePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"index"}, true); err != nil {
		return err
	}
	*value = NotebookChangeDeletePayload(decoded)
	return nil
}
func (value NotebookChangeDeletePayload) MarshalJSON() ([]byte, error) {
	type plain NotebookChangeDeletePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var notebookChangeVariants = variantFactories{"replace": func() any { return new(NotebookChangeReplacePayload) },
	"insert": func() any { return new(NotebookChangeInsertPayload) },
	"delete": func() any { return new(NotebookChangeDeletePayload) }}

func (value NotebookChange) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, notebookChangeVariants)
}
func (value *NotebookChange) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", notebookChangeVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type NotebookKind string

const (
	NotebookKindCode     NotebookKind = "code"
	NotebookKindMarkdown NotebookKind = "markdown"
	NotebookKindRaw      NotebookKind = "raw"
)

func (value NotebookKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *NotebookKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch NotebookKind(decoded) {
	case NotebookKindCode, NotebookKindMarkdown, NotebookKindRaw:
		*value = NotebookKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value NotebookKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *NotebookKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type NotebookPage struct {
	Path              string          `json:"path"`
	Revision          string          `json:"revision"`
	Nbformat          uint32          `json:"nbformat"`
	NbformatMinor     uint32          `json:"nbformat_minor"`
	Metadata          json.RawMessage `json:"metadata"`
	MetadataTruncated bool            `json:"metadata_truncated"`
	Total             uint64          `json:"total"`
	Cells             []NotebookCell  `json:"cells"`
	NextOffset        *uint64         `json:"next_offset"`
}

func DefaultNotebookPage() NotebookPage {
	return NotebookPage{
		Path:              "",
		Revision:          "",
		Nbformat:          0,
		NbformatMinor:     0,
		Metadata:          json.RawMessage("null"),
		MetadataTruncated: false,
		Total:             0,
		Cells:             []NotebookCell{},
		NextOffset:        nil,
	}
}
func (value *NotebookPage) UnmarshalJSON(data []byte) error {
	type plain NotebookPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "revision", "nbformat", "nbformat_minor", "metadata", "metadata_truncated", "total", "cells"}, false); err != nil {
		return err
	}
	*value = NotebookPage(decoded)
	return nil
}
func (value NotebookPage) MarshalJSON() ([]byte, error) {
	type plain NotebookPage
	encoded := plain(value)
	encoded.Cells = normalizeCollections(encoded.Cells)
	return json.Marshal(encoded)
}

type NotebookCell struct {
	Index     uint64          `json:"index"`
	Value     json.RawMessage `json:"value"`
	Truncated bool            `json:"truncated"`
}

func DefaultNotebookCell() NotebookCell {
	return NotebookCell{
		Index:     0,
		Value:     json.RawMessage("null"),
		Truncated: false,
	}
}
func (value *NotebookCell) UnmarshalJSON(data []byte) error {
	type plain NotebookCell
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"index", "value", "truncated"}, false); err != nil {
		return err
	}
	*value = NotebookCell(decoded)
	return nil
}
func (value NotebookCell) MarshalJSON() ([]byte, error) {
	type plain NotebookCell
	encoded := plain(value)
	return json.Marshal(encoded)
}
