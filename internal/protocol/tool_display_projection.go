// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ToolDisplayProjectionOutput struct {
	Preview *ToolDisplayProjectionText    `json:"preview"`
	Notices []ToolDisplayProjectionNotice `json:"notices"`
	Body    *ToolDisplayProjectionBody    `json:"body"`
	Paths   *ToolDisplayProjectionPaths   `json:"paths"`
	Table   *ToolDisplayProjectionTable   `json:"table"`
}

func DefaultToolDisplayProjectionOutput() ToolDisplayProjectionOutput {
	return ToolDisplayProjectionOutput{
		Preview: nil,
		Notices: []ToolDisplayProjectionNotice{},
		Body:    nil,
		Paths:   nil,
		Table:   nil,
	}
}
func (value *ToolDisplayProjectionOutput) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionOutput
	decoded := plain{}
	decoded.Notices = []ToolDisplayProjectionNotice{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionOutput(decoded)
	return nil
}
func (value ToolDisplayProjectionOutput) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionOutput
	encoded := plain(value)
	encoded.Notices = normalizeCollections(encoded.Notices)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionReference struct {
	Source ToolDisplayProjectionSource `json:"source"`
	Path   string                      `json:"path"`
}

func DefaultToolDisplayProjectionReference() ToolDisplayProjectionReference {
	return ToolDisplayProjectionReference{
		Source: ToolDisplayProjectionSource(""),
		Path:   "",
	}
}
func (value *ToolDisplayProjectionReference) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionReference
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "path"}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionReference(decoded)
	return nil
}
func (value ToolDisplayProjectionReference) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionReference
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionSource string

const (
	ToolDisplayProjectionSourceArguments ToolDisplayProjectionSource = "arguments"
	ToolDisplayProjectionSourceResult    ToolDisplayProjectionSource = "result"
	ToolDisplayProjectionSourceItem      ToolDisplayProjectionSource = "item"
)

func (value ToolDisplayProjectionSource) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ToolDisplayProjectionSource) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ToolDisplayProjectionSource(decoded) {
	case ToolDisplayProjectionSourceArguments, ToolDisplayProjectionSourceResult, ToolDisplayProjectionSourceItem:
		*value = ToolDisplayProjectionSource(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ToolDisplayProjectionSource) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolDisplayProjectionSource) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ToolDisplayProjectionText struct {
	Parts     []ToolDisplayProjectionPart `json:"parts"`
	Separator string                      `json:"separator"`
}

func DefaultToolDisplayProjectionText() ToolDisplayProjectionText {
	return ToolDisplayProjectionText{
		Parts:     []ToolDisplayProjectionPart{},
		Separator: "",
	}
}
func (value *ToolDisplayProjectionText) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionText
	decoded := plain{}
	decoded.Separator = ""
	if err := DecodeObject(data, &decoded, []string{"parts"}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionText(decoded)
	return nil
}
func (value ToolDisplayProjectionText) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionText
	encoded := plain(value)
	encoded.Parts = normalizeCollections(encoded.Parts)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionPart struct {
	Take   *uint64                           `json:"take"`
	Text   *string                           `json:"text"`
	Value  *ToolDisplayProjectionReference   `json:"value"`
	Format *ToolDisplayProjectionValueFormat `json:"format,omitempty"`
	Rows   *ToolDisplayProjectionRows        `json:"rows"`
	When   *ToolDisplayProjectionCondition   `json:"when"`
}

func DefaultToolDisplayProjectionPart() ToolDisplayProjectionPart {
	return ToolDisplayProjectionPart{
		Take:   nil,
		Text:   nil,
		Value:  nil,
		Format: nil,
		Rows:   nil,
		When:   nil,
	}
}
func (value *ToolDisplayProjectionPart) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionPart
	decoded := plain{}
	decoded.Format = nil
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionPart(decoded)
	return nil
}
func (value ToolDisplayProjectionPart) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionPart
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionValueFormat string

const (
	ToolDisplayProjectionValueFormatJson ToolDisplayProjectionValueFormat = "json"
)

func (value ToolDisplayProjectionValueFormat) MarshalJSON() ([]byte, error) {
	return marshalEnum(value)
}
func (value *ToolDisplayProjectionValueFormat) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ToolDisplayProjectionValueFormat(decoded) {
	case ToolDisplayProjectionValueFormatJson:
		*value = ToolDisplayProjectionValueFormat(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ToolDisplayProjectionValueFormat) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolDisplayProjectionValueFormat) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ToolDisplayProjectionRows struct {
	Value     ToolDisplayProjectionReference `json:"value"`
	Text      ToolDisplayProjectionText      `json:"text"`
	Separator string                         `json:"separator"`
}

func DefaultToolDisplayProjectionRows() ToolDisplayProjectionRows {
	return ToolDisplayProjectionRows{
		Value:     DefaultToolDisplayProjectionReference(),
		Text:      DefaultToolDisplayProjectionText(),
		Separator: "",
	}
}
func (value *ToolDisplayProjectionRows) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionRows
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"value", "text", "separator"}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionRows(decoded)
	return nil
}
func (value ToolDisplayProjectionRows) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionRows
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionCondition struct {
	Any []ToolDisplayProjectionComparison `json:"any"`
	All []ToolDisplayProjectionComparison `json:"all"`
}

func DefaultToolDisplayProjectionCondition() ToolDisplayProjectionCondition {
	return ToolDisplayProjectionCondition{
		Any: []ToolDisplayProjectionComparison{},
		All: []ToolDisplayProjectionComparison{},
	}
}
func (value *ToolDisplayProjectionCondition) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionCondition
	decoded := plain{}
	decoded.Any = []ToolDisplayProjectionComparison{}
	decoded.All = []ToolDisplayProjectionComparison{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionCondition(decoded)
	return nil
}
func (value ToolDisplayProjectionCondition) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionCondition
	encoded := plain(value)
	encoded.Any = normalizeCollections(encoded.Any)
	encoded.All = normalizeCollections(encoded.All)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionComparison struct {
	Value  ToolDisplayProjectionReference `json:"value"`
	Item   *string                        `json:"item"`
	Equals json.RawMessage                `json:"equals"`
	Negate bool                           `json:"negate"`
}

func DefaultToolDisplayProjectionComparison() ToolDisplayProjectionComparison {
	return ToolDisplayProjectionComparison{
		Value:  DefaultToolDisplayProjectionReference(),
		Item:   nil,
		Equals: json.RawMessage("null"),
		Negate: false,
	}
}
func (value *ToolDisplayProjectionComparison) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionComparison
	decoded := plain{}
	decoded.Negate = false
	if err := DecodeObject(data, &decoded, []string{"value", "equals"}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionComparison(decoded)
	return nil
}
func (value ToolDisplayProjectionComparison) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionComparison
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionNotice struct {
	Message PluginDesktopNavigation         `json:"message"`
	When    *ToolDisplayProjectionCondition `json:"when"`
}

func DefaultToolDisplayProjectionNotice() ToolDisplayProjectionNotice {
	return ToolDisplayProjectionNotice{
		Message: DefaultPluginDesktopNavigation(),
		When:    nil,
	}
}
func (value *ToolDisplayProjectionNotice) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionNotice
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"message"}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionNotice(decoded)
	return nil
}
func (value ToolDisplayProjectionNotice) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionNotice
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionBody struct {
	Text  ToolDisplayProjectionReference  `json:"text"`
	Path  *ToolDisplayProjectionReference `json:"path"`
	Diff  *ToolDisplayProjectionDiff      `json:"diff"`
	Empty *PluginDesktopNavigation        `json:"empty"`
}

func DefaultToolDisplayProjectionBody() ToolDisplayProjectionBody {
	return ToolDisplayProjectionBody{
		Text:  DefaultToolDisplayProjectionReference(),
		Path:  nil,
		Diff:  nil,
		Empty: nil,
	}
}
func (value *ToolDisplayProjectionBody) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionBody
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text"}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionBody(decoded)
	return nil
}
func (value ToolDisplayProjectionBody) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionBody
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionDiff struct {
	Format ToolDisplayProjectionFormat     `json:"format"`
	When   *ToolDisplayProjectionCondition `json:"when"`
}

func DefaultToolDisplayProjectionDiff() ToolDisplayProjectionDiff {
	return ToolDisplayProjectionDiff{
		Format: ToolDisplayProjectionFormat(""),
		When:   nil,
	}
}
func (value *ToolDisplayProjectionDiff) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionDiff
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"format"}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionDiff(decoded)
	return nil
}
func (value ToolDisplayProjectionDiff) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionDiff
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionFormat string

const (
	ToolDisplayProjectionFormatAdded      ToolDisplayProjectionFormat = "added"
	ToolDisplayProjectionFormatUnified    ToolDisplayProjectionFormat = "unified"
	ToolDisplayProjectionFormatCheckpoint ToolDisplayProjectionFormat = "checkpoint"
)

func (value ToolDisplayProjectionFormat) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ToolDisplayProjectionFormat) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ToolDisplayProjectionFormat(decoded) {
	case ToolDisplayProjectionFormatAdded, ToolDisplayProjectionFormatUnified, ToolDisplayProjectionFormatCheckpoint:
		*value = ToolDisplayProjectionFormat(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ToolDisplayProjectionFormat) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolDisplayProjectionFormat) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ToolDisplayProjectionPaths struct {
	Value ToolDisplayProjectionReference `json:"value"`
	Item  *string                        `json:"item"`
}

func DefaultToolDisplayProjectionPaths() ToolDisplayProjectionPaths {
	return ToolDisplayProjectionPaths{
		Value: DefaultToolDisplayProjectionReference(),
		Item:  nil,
	}
}
func (value *ToolDisplayProjectionPaths) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionPaths
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"value"}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionPaths(decoded)
	return nil
}
func (value ToolDisplayProjectionPaths) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionPaths
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDisplayProjectionPrompt struct {
	Message PluginDesktopNavigation                   `json:"message"`
	Values  map[string]ToolDisplayProjectionReference `json:"values"`
	When    *ToolDisplayProjectionCondition           `json:"when"`
}

func DefaultToolDisplayProjectionPrompt() ToolDisplayProjectionPrompt {
	return ToolDisplayProjectionPrompt{
		Message: DefaultPluginDesktopNavigation(),
		Values:  map[string]ToolDisplayProjectionReference{},
		When:    nil,
	}
}
func (value *ToolDisplayProjectionPrompt) UnmarshalJSON(data []byte) error {
	type plain ToolDisplayProjectionPrompt
	decoded := plain{}
	decoded.Values = map[string]ToolDisplayProjectionReference{}
	if err := DecodeObject(data, &decoded, []string{"message"}, true); err != nil {
		return err
	}
	*value = ToolDisplayProjectionPrompt(decoded)
	return nil
}
func (value ToolDisplayProjectionPrompt) MarshalJSON() ([]byte, error) {
	type plain ToolDisplayProjectionPrompt
	encoded := plain(value)
	encoded.Values = normalizeCollections(encoded.Values)
	return json.Marshal(encoded)
}
