// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type CodeIndexSearch struct {
	Query    string  `json:"query"`
	Name     *string `json:"name"`
	Path     *string `json:"path"`
	Language *string `json:"language"`
	Limit    uint64  `json:"limit"`
}

func DefaultCodeIndexSearch() CodeIndexSearch {
	return CodeIndexSearch{
		Query:    "",
		Name:     nil,
		Path:     nil,
		Language: nil,
		Limit:    20,
	}
}
func (value *CodeIndexSearch) UnmarshalJSON(data []byte) error {
	type plain CodeIndexSearch
	decoded := plain{}
	decoded.Query = ""
	decoded.Name = nil
	decoded.Path = nil
	decoded.Language = nil
	decoded.Limit = 20
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = CodeIndexSearch(decoded)
	return nil
}
func (value CodeIndexSearch) MarshalJSON() ([]byte, error) {
	type plain CodeIndexSearch
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeIndexRange struct {
	StartByte   uint32 `json:"start_byte"`
	EndByte     uint32 `json:"end_byte"`
	StartLine   uint32 `json:"start_line"`
	StartColumn uint32 `json:"start_column"`
	EndLine     uint32 `json:"end_line"`
	EndColumn   uint32 `json:"end_column"`
}

func DefaultCodeIndexRange() CodeIndexRange {
	return CodeIndexRange{
		StartByte:   0,
		EndByte:     0,
		StartLine:   0,
		StartColumn: 0,
		EndLine:     0,
		EndColumn:   0,
	}
}
func (value *CodeIndexRange) UnmarshalJSON(data []byte) error {
	type plain CodeIndexRange
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"start_byte", "end_byte", "start_line", "start_column", "end_line", "end_column"}, false); err != nil {
		return err
	}
	*value = CodeIndexRange(decoded)
	return nil
}
func (value CodeIndexRange) MarshalJSON() ([]byte, error) {
	type plain CodeIndexRange
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeIndexSymbol struct {
	Path         string         `json:"path"`
	Language     string         `json:"language"`
	Name         string         `json:"name"`
	Kind         string         `json:"kind"`
	Range        CodeIndexRange `json:"range"`
	Signature    *string        `json:"signature"`
	FileRevision string         `json:"file_revision"`
	Depth        uint16         `json:"depth"`
}

func DefaultCodeIndexSymbol() CodeIndexSymbol {
	return CodeIndexSymbol{
		Path:         "",
		Language:     "",
		Name:         "",
		Kind:         "",
		Range:        DefaultCodeIndexRange(),
		Signature:    nil,
		FileRevision: "",
		Depth:        0,
	}
}
func (value *CodeIndexSymbol) UnmarshalJSON(data []byte) error {
	type plain CodeIndexSymbol
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "language", "name", "kind", "range", "file_revision", "depth"}, false); err != nil {
		return err
	}
	*value = CodeIndexSymbol(decoded)
	return nil
}
func (value CodeIndexSymbol) MarshalJSON() ([]byte, error) {
	type plain CodeIndexSymbol
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeIndexStatus string

const (
	CodeIndexStatusParsed      CodeIndexStatus = "parsed"
	CodeIndexStatusUnsupported CodeIndexStatus = "unsupported"
	CodeIndexStatusTooLarge    CodeIndexStatus = "too_large"
	CodeIndexStatusInvalid     CodeIndexStatus = "invalid"
	CodeIndexStatusFailed      CodeIndexStatus = "failed"
)

func (value CodeIndexStatus) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *CodeIndexStatus) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch CodeIndexStatus(decoded) {
	case CodeIndexStatusParsed, CodeIndexStatusUnsupported, CodeIndexStatusTooLarge, CodeIndexStatusInvalid, CodeIndexStatusFailed:
		*value = CodeIndexStatus(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value CodeIndexStatus) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *CodeIndexStatus) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type CodeIndexCapabilities struct {
	Structure bool `json:"structure"`
	Symbols   bool `json:"symbols"`
	Data      bool `json:"data"`
	Embedded  bool `json:"embedded"`
}

func DefaultCodeIndexCapabilities() CodeIndexCapabilities {
	return CodeIndexCapabilities{
		Structure: false,
		Symbols:   false,
		Data:      false,
		Embedded:  false,
	}
}
func (value *CodeIndexCapabilities) UnmarshalJSON(data []byte) error {
	type plain CodeIndexCapabilities
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"structure", "symbols", "data", "embedded"}, false); err != nil {
		return err
	}
	*value = CodeIndexCapabilities(decoded)
	return nil
}
func (value CodeIndexCapabilities) MarshalJSON() ([]byte, error) {
	type plain CodeIndexCapabilities
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeIndexIssue struct {
	Path   string          `json:"path"`
	Status CodeIndexStatus `json:"status"`
}

func DefaultCodeIndexIssue() CodeIndexIssue {
	return CodeIndexIssue{
		Path:   "",
		Status: CodeIndexStatus(""),
	}
}
func (value *CodeIndexIssue) UnmarshalJSON(data []byte) error {
	type plain CodeIndexIssue
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "status"}, false); err != nil {
		return err
	}
	*value = CodeIndexIssue(decoded)
	return nil
}
func (value CodeIndexIssue) MarshalJSON() ([]byte, error) {
	type plain CodeIndexIssue
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeIndexSearchPage struct {
	Symbols       []CodeIndexSymbol `json:"symbols"`
	IndexRevision uint64            `json:"index_revision"`
	Partial       bool              `json:"partial"`
	Truncated     bool              `json:"truncated"`
	Visited       uint64            `json:"visited"`
	Skipped       uint64            `json:"skipped"`
	Issues        []CodeIndexIssue  `json:"issues"`
}

func DefaultCodeIndexSearchPage() CodeIndexSearchPage {
	return CodeIndexSearchPage{
		Symbols:       []CodeIndexSymbol{},
		IndexRevision: 0,
		Partial:       false,
		Truncated:     false,
		Visited:       0,
		Skipped:       0,
		Issues:        []CodeIndexIssue{},
	}
}
func (value *CodeIndexSearchPage) UnmarshalJSON(data []byte) error {
	type plain CodeIndexSearchPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"symbols", "index_revision", "partial", "truncated", "visited", "skipped", "issues"}, false); err != nil {
		return err
	}
	*value = CodeIndexSearchPage(decoded)
	return nil
}
func (value CodeIndexSearchPage) MarshalJSON() ([]byte, error) {
	type plain CodeIndexSearchPage
	encoded := plain(value)
	encoded.Symbols = normalizeCollections(encoded.Symbols)
	encoded.Issues = normalizeCollections(encoded.Issues)
	return json.Marshal(encoded)
}

type CodeIndexOutline struct {
	Path          string                `json:"path"`
	Language      *string               `json:"language"`
	FileRevision  *string               `json:"file_revision"`
	IndexRevision uint64                `json:"index_revision"`
	Status        CodeIndexStatus       `json:"status"`
	Symbols       []CodeIndexSymbol     `json:"symbols"`
	Capabilities  CodeIndexCapabilities `json:"capabilities"`
	Partial       bool                  `json:"partial"`
	Truncated     bool                  `json:"truncated"`
}

func DefaultCodeIndexOutline() CodeIndexOutline {
	return CodeIndexOutline{
		Path:          "",
		Language:      nil,
		FileRevision:  nil,
		IndexRevision: 0,
		Status:        CodeIndexStatus(""),
		Symbols:       []CodeIndexSymbol{},
		Capabilities:  DefaultCodeIndexCapabilities(),
		Partial:       false,
		Truncated:     false,
	}
}
func (value *CodeIndexOutline) UnmarshalJSON(data []byte) error {
	type plain CodeIndexOutline
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "index_revision", "status", "symbols", "capabilities", "partial", "truncated"}, false); err != nil {
		return err
	}
	*value = CodeIndexOutline(decoded)
	return nil
}
func (value CodeIndexOutline) MarshalJSON() ([]byte, error) {
	type plain CodeIndexOutline
	encoded := plain(value)
	encoded.Symbols = normalizeCollections(encoded.Symbols)
	return json.Marshal(encoded)
}

type CodeIndexSettings struct {
	Revision uint64 `json:"revision"`
	Enabled  bool   `json:"enabled"`
}

func DefaultCodeIndexSettings() CodeIndexSettings {
	return CodeIndexSettings{
		Revision: 0,
		Enabled:  true,
	}
}
func (value *CodeIndexSettings) UnmarshalJSON(data []byte) error {
	type plain CodeIndexSettings
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "enabled"}, false); err != nil {
		return err
	}
	*value = CodeIndexSettings(decoded)
	return nil
}
func (value CodeIndexSettings) MarshalJSON() ([]byte, error) {
	type plain CodeIndexSettings
	encoded := plain(value)
	return json.Marshal(encoded)
}
