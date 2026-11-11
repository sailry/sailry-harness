// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ToolDisplay struct {
	Label     string                        `json:"label"`
	Icon      *PluginDesktopIcon            `json:"icon,omitempty"`
	Locales   map[string]string             `json:"locales"`
	Input     *ToolDisplayInput             `json:"input,omitempty"`
	Output    *ToolDisplayProjectionOutput  `json:"output,omitempty"`
	Approval  []ToolDisplayProjectionPrompt `json:"approval,omitempty"`
	Aliases   []ToolDisplayAlias            `json:"aliases,omitempty"`
	RawResult bool                          `json:"raw_result,omitempty"`
}

func DefaultToolDisplay() ToolDisplay {
	return ToolDisplay{
		Label:     "",
		Icon:      nil,
		Locales:   map[string]string{},
		Input:     nil,
		Output:    nil,
		Approval:  []ToolDisplayProjectionPrompt{},
		Aliases:   []ToolDisplayAlias{},
		RawResult: false,
	}
}
func (value *ToolDisplay) UnmarshalJSON(data []byte) error {
	type plain ToolDisplay
	decoded := plain{}
	decoded.Icon = nil
	decoded.Locales = map[string]string{}
	decoded.Input = nil
	decoded.Output = nil
	decoded.Approval = []ToolDisplayProjectionPrompt{}
	decoded.Aliases = []ToolDisplayAlias{}
	decoded.RawResult = false
	if err := DecodeObject(data, &decoded, []string{"label"}, true); err != nil {
		return err
	}
	*value = ToolDisplay(decoded)
	return nil
}
func (value ToolDisplay) MarshalJSON() ([]byte, error) {
	type plain ToolDisplay
	encoded := plain(value)
	encoded.Locales = normalizeCollections(encoded.Locales)
	encoded.Approval = normalizeCollections(encoded.Approval)
	encoded.Aliases = normalizeCollections(encoded.Aliases)
	return json.Marshal(encoded)
}

type ToolDisplayInput struct {
	Summary *ToolDisplayField `json:"summary"`
	Context *ToolDisplayField `json:"context"`
	Target  *ToolDisplayField `json:"target"`
	Content *ToolDisplayField `json:"content"`
}

func DefaultToolDisplayInput() ToolDisplayInput {
	return ToolDisplayInput{
		Summary: nil,
		Context: nil,
		Target:  nil,
		Content: nil,
	}
}
func (value *ToolDisplayInput) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayInput
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ToolDisplayInput(decoded)
	return nil
}
func (value ToolDisplayInput) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayInput
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayField struct {
	Path       string                  `json:"path"`
	Code       bool                    `json:"code"`
	Omit       []string                `json:"omit"`
	Format     *ToolDisplayInputFormat `json:"format,omitempty"`
	ResultPath *string                 `json:"result_path,omitempty"`
	Names      map[string]string       `json:"names,omitempty"`
}

func DefaultToolDisplayField() ToolDisplayField {
	return ToolDisplayField{
		Path:       "",
		Code:       false,
		Omit:       []string{},
		Format:     nil,
		ResultPath: nil,
		Names:      map[string]string{},
	}
}
func (value *ToolDisplayField) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayField
	decoded := plain{}
	decoded.Code = false
	decoded.Omit = []string{}
	decoded.Format = nil
	decoded.ResultPath = nil
	decoded.Names = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"path"}, true); err != nil {
		return err
	}
	*value = ToolDisplayField(decoded)
	return nil
}
func (value ToolDisplayField) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayField
	encoded := plain(value)
	encoded.Omit = normalizeCollections(encoded.Omit)
	encoded.Names = normalizeCollections(encoded.Names)
	return json.Marshal(encoded)
}

type ToolDisplayInputFormat string

const (
	ToolDisplayInputFormatCommand    ToolDisplayInputFormat = "command"
	ToolDisplayInputFormatSession    ToolDisplayInputFormat = "session"
	ToolDisplayInputFormatConnection ToolDisplayInputFormat = "connection"
)

func (value ToolDisplayInputFormat) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ToolDisplayInputFormat) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ToolDisplayInputFormat(decoded) {
	case ToolDisplayInputFormatCommand, ToolDisplayInputFormatSession, ToolDisplayInputFormatConnection:
		*value = ToolDisplayInputFormat(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ToolDisplayInputFormat) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolDisplayInputFormat) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ToolDisplayAlias struct {
	Path string               `json:"path"`
	Kind ToolDisplayAliasKind `json:"kind"`
}

func DefaultToolDisplayAlias() ToolDisplayAlias {
	return ToolDisplayAlias{
		Path: "",
		Kind: ToolDisplayAliasKind(""),
	}
}
func (value *ToolDisplayAlias) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayAlias
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "kind"}, true); err != nil {
		return err
	}
	*value = ToolDisplayAlias(decoded)
	return nil
}
func (value ToolDisplayAlias) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayAlias
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayAliasKind string

const (
	ToolDisplayAliasKindHome   ToolDisplayAliasKind = "home"
	ToolDisplayAliasKindPython ToolDisplayAliasKind = "python"
)

func (value ToolDisplayAliasKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ToolDisplayAliasKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ToolDisplayAliasKind(decoded) {
	case ToolDisplayAliasKindHome, ToolDisplayAliasKindPython:
		*value = ToolDisplayAliasKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ToolDisplayAliasKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolDisplayAliasKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
