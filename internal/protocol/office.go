// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type OfficeRead struct {
	Source OfficeSource `json:"source"`
	Offset uint64       `json:"offset"`
}

func DefaultOfficeRead() OfficeRead {
	return OfficeRead{
		Source: OfficeSource{},
		Offset: 0,
	}
}
func (value *OfficeRead) UnmarshalJSON(data []byte) error {
	type plain OfficeRead
	decoded := plain{}
	decoded.Offset = 0
	if err := DecodeObject(data, &decoded, []string{"source"}, true); err != nil {
		return err
	}
	*value = OfficeRead(decoded)
	return nil
}
func (value OfficeRead) MarshalJSON() ([]byte, error) {
	type plain OfficeRead
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OfficeSource struct {
	Kind string
	Data any
}

var officeSourceVariants = variantFactories{"path": func() any { return new(string) },
	"attachment": func() any { return new(AttachmentID) }}

func (value OfficeSource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "value", value.Kind, value.Data, officeSourceVariants)
}
func (value *OfficeSource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "value", officeSourceVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type OfficeInspection struct {
	Path     string          `json:"path"`
	Revision string          `json:"revision"`
	Sections []OfficeSection `json:"sections"`
	Next     *uint64         `json:"next"`
}

func DefaultOfficeInspection() OfficeInspection {
	return OfficeInspection{
		Path:     "",
		Revision: "",
		Sections: []OfficeSection{},
		Next:     nil,
	}
}
func (value *OfficeInspection) UnmarshalJSON(data []byte) error {
	type plain OfficeInspection
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "revision", "sections"}, false); err != nil {
		return err
	}
	*value = OfficeInspection(decoded)
	return nil
}
func (value OfficeInspection) MarshalJSON() ([]byte, error) {
	type plain OfficeInspection
	encoded := plain(value)
	encoded.Sections = normalizeCollections(encoded.Sections)
	return json.Marshal(encoded)
}

type OfficeSection struct {
	Name      string `json:"name"`
	Text      string `json:"text"`
	Truncated bool   `json:"truncated"`
}

func DefaultOfficeSection() OfficeSection {
	return OfficeSection{
		Name:      "",
		Text:      "",
		Truncated: false,
	}
}
func (value *OfficeSection) UnmarshalJSON(data []byte) error {
	type plain OfficeSection
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "text", "truncated"}, false); err != nil {
		return err
	}
	*value = OfficeSection(decoded)
	return nil
}
func (value OfficeSection) MarshalJSON() ([]byte, error) {
	type plain OfficeSection
	encoded := plain(value)
	return json.Marshal(encoded)
}
