// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ToolContent struct {
	Version uint32             `json:"version"`
	Blocks  []ToolContentBlock `json:"blocks"`
}

func DefaultToolContent() ToolContent {
	return ToolContent{
		Version: 0,
		Blocks:  []ToolContentBlock{},
	}
}
func (value *ToolContent) UnmarshalJSON(data []byte) error {
	type plain ToolContent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"version", "blocks"}, true); err != nil {
		return err
	}
	*value = ToolContent(decoded)
	return nil
}
func (value ToolContent) MarshalJSON() ([]byte, error) {
	type plain ToolContent
	encoded := plain(value)
	encoded.Blocks = normalizeCollections(encoded.Blocks)
	return json.Marshal(encoded)
}

type ToolContentBlock struct {
	Kind string
	Data any
}
type ToolContentBlockNoticePayload struct {
	Message PluginDesktopNavigation `json:"message"`
}

func DefaultToolContentBlockNoticePayload() ToolContentBlockNoticePayload {
	return ToolContentBlockNoticePayload{
		Message: DefaultPluginDesktopNavigation(),
	}
}
func (value *ToolContentBlockNoticePayload) UnmarshalJSON(data []byte) error {
	type plain ToolContentBlockNoticePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"message"}, true); err != nil {
		return err
	}
	*value = ToolContentBlockNoticePayload(decoded)
	return nil
}
func (value ToolContentBlockNoticePayload) MarshalJSON() ([]byte, error) {
	type plain ToolContentBlockNoticePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolContentBlockImagePayload struct {
	Index uint64 `json:"index"`
}

func DefaultToolContentBlockImagePayload() ToolContentBlockImagePayload {
	return ToolContentBlockImagePayload{
		Index: 0,
	}
}
func (value *ToolContentBlockImagePayload) UnmarshalJSON(data []byte) error {
	type plain ToolContentBlockImagePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"index"}, true); err != nil {
		return err
	}
	*value = ToolContentBlockImagePayload(decoded)
	return nil
}
func (value ToolContentBlockImagePayload) MarshalJSON() ([]byte, error) {
	type plain ToolContentBlockImagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolContentBlockArtifactPayload struct {
	Artifact ArtifactSummary `json:"artifact"`
}

func DefaultToolContentBlockArtifactPayload() ToolContentBlockArtifactPayload {
	return ToolContentBlockArtifactPayload{
		Artifact: DefaultArtifactSummary(),
	}
}
func (value *ToolContentBlockArtifactPayload) UnmarshalJSON(data []byte) error {
	type plain ToolContentBlockArtifactPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"artifact"}, true); err != nil {
		return err
	}
	*value = ToolContentBlockArtifactPayload(decoded)
	return nil
}
func (value ToolContentBlockArtifactPayload) MarshalJSON() ([]byte, error) {
	type plain ToolContentBlockArtifactPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var toolContentBlockVariants = variantFactories{"text": func() any { return new(ToolContentText) },
	"notice":   func() any { return new(ToolContentBlockNoticePayload) },
	"table":    func() any { return new(ToolContentTable) },
	"image":    func() any { return new(ToolContentBlockImagePayload) },
	"diff":     func() any { return new(ToolContentDiff) },
	"file":     func() any { return new(ToolContentFile) },
	"artifact": func() any { return new(ToolContentBlockArtifactPayload) },
	"card":     func() any { return new(PluginCardSummary) }}

func (value ToolContentBlock) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, toolContentBlockVariants)
}
func (value *ToolContentBlock) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", toolContentBlockVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ToolContentText struct {
	Path    string                    `json:"path"`
	Notices []PluginDesktopNavigation `json:"notices"`
}

func DefaultToolContentText() ToolContentText {
	return ToolContentText{
		Path:    "",
		Notices: []PluginDesktopNavigation{},
	}
}
func (value *ToolContentText) UnmarshalJSON(data []byte) error {
	type plain ToolContentText
	decoded := plain{}
	decoded.Notices = []PluginDesktopNavigation{}
	if err := DecodeObject(data, &decoded, []string{"path"}, true); err != nil {
		return err
	}
	*value = ToolContentText(decoded)
	return nil
}
func (value ToolContentText) MarshalJSON() ([]byte, error) {
	type plain ToolContentText
	encoded := plain(value)
	encoded.Notices = normalizeCollections(encoded.Notices)
	return json.Marshal(encoded)
}

type ToolContentFile struct {
	Path     string  `json:"path"`
	Name     *string `json:"name"`
	Mime     string  `json:"mime"`
	Size     *uint64 `json:"size"`
	Revision *string `json:"revision"`
}

func DefaultToolContentFile() ToolContentFile {
	return ToolContentFile{
		Path:     "",
		Name:     nil,
		Mime:     "",
		Size:     nil,
		Revision: nil,
	}
}
func (value *ToolContentFile) UnmarshalJSON(data []byte) error {
	type plain ToolContentFile
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "mime"}, true); err != nil {
		return err
	}
	*value = ToolContentFile(decoded)
	return nil
}
func (value ToolContentFile) MarshalJSON() ([]byte, error) {
	type plain ToolContentFile
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolContentTable struct {
	Columns   []string   `json:"columns"`
	Rows      [][]string `json:"rows"`
	Truncated bool       `json:"truncated"`
}

func DefaultToolContentTable() ToolContentTable {
	return ToolContentTable{
		Columns:   []string{},
		Rows:      [][]string{},
		Truncated: false,
	}
}
func (value *ToolContentTable) UnmarshalJSON(data []byte) error {
	type plain ToolContentTable
	decoded := plain{}
	decoded.Truncated = false
	if err := DecodeObject(data, &decoded, []string{"columns", "rows"}, true); err != nil {
		return err
	}
	*value = ToolContentTable(decoded)
	return nil
}
func (value ToolContentTable) MarshalJSON() ([]byte, error) {
	type plain ToolContentTable
	encoded := plain(value)
	encoded.Columns = normalizeCollections(encoded.Columns)
	encoded.Rows = normalizeCollections(encoded.Rows)
	return json.Marshal(encoded)
}

type ToolContentDiff struct {
	Path      string `json:"path"`
	Text      string `json:"text"`
	Truncated bool   `json:"truncated"`
}

func DefaultToolContentDiff() ToolContentDiff {
	return ToolContentDiff{
		Path:      "",
		Text:      "",
		Truncated: false,
	}
}
func (value *ToolContentDiff) UnmarshalJSON(data []byte) error {
	type plain ToolContentDiff
	decoded := plain{}
	decoded.Truncated = false
	if err := DecodeObject(data, &decoded, []string{"path", "text"}, true); err != nil {
		return err
	}
	*value = ToolContentDiff(decoded)
	return nil
}
func (value ToolContentDiff) MarshalJSON() ([]byte, error) {
	type plain ToolContentDiff
	encoded := plain(value)
	return json.Marshal(encoded)
}
