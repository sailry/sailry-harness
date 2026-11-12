// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type TerminalScreenTextStyle struct {
	Foreground     *TerminalRgb                 `json:"foreground"`
	Background     *TerminalRgb                 `json:"background"`
	UnderlineColor *TerminalRgb                 `json:"underline_color"`
	Bold           bool                         `json:"bold"`
	Faint          bool                         `json:"faint"`
	Italic         bool                         `json:"italic"`
	Blink          bool                         `json:"blink"`
	Underline      TerminalScreenUnderlineStyle `json:"underline"`
	Inverse        bool                         `json:"inverse"`
	Invisible      bool                         `json:"invisible"`
	Strikethrough  bool                         `json:"strikethrough"`
	Overline       bool                         `json:"overline"`
}

func DefaultTerminalScreenTextStyle() TerminalScreenTextStyle {
	return TerminalScreenTextStyle{
		Foreground:     nil,
		Background:     nil,
		UnderlineColor: nil,
		Bold:           false,
		Faint:          false,
		Italic:         false,
		Blink:          false,
		Underline:      TerminalScreenUnderlineStyleNone,
		Inverse:        false,
		Invisible:      false,
		Strikethrough:  false,
		Overline:       false,
	}
}
func (value *TerminalScreenTextStyle) UnmarshalJSON(data []byte) error {
	type plain TerminalScreenTextStyle
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"bold", "faint", "italic", "blink", "underline", "inverse", "invisible", "strikethrough", "overline"}, false); err != nil {
		return err
	}
	*value = TerminalScreenTextStyle(decoded)
	return nil
}
func (value TerminalScreenTextStyle) MarshalJSON() ([]byte, error) {
	type plain TerminalScreenTextStyle
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalScreenUnderlineStyle string

const (
	TerminalScreenUnderlineStyleNone   TerminalScreenUnderlineStyle = "none"
	TerminalScreenUnderlineStyleSingle TerminalScreenUnderlineStyle = "single"
	TerminalScreenUnderlineStyleDouble TerminalScreenUnderlineStyle = "double"
	TerminalScreenUnderlineStyleCurly  TerminalScreenUnderlineStyle = "curly"
	TerminalScreenUnderlineStyleDotted TerminalScreenUnderlineStyle = "dotted"
	TerminalScreenUnderlineStyleDashed TerminalScreenUnderlineStyle = "dashed"
)

func (value TerminalScreenUnderlineStyle) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *TerminalScreenUnderlineStyle) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch TerminalScreenUnderlineStyle(decoded) {
	case TerminalScreenUnderlineStyleNone, TerminalScreenUnderlineStyleSingle, TerminalScreenUnderlineStyleDouble, TerminalScreenUnderlineStyleCurly, TerminalScreenUnderlineStyleDotted, TerminalScreenUnderlineStyleDashed:
		*value = TerminalScreenUnderlineStyle(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value TerminalScreenUnderlineStyle) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *TerminalScreenUnderlineStyle) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type TerminalScreenSpan struct {
	Column    uint16                  `json:"column"`
	Columns   uint16                  `json:"columns"`
	Text      string                  `json:"text"`
	Style     TerminalScreenTextStyle `json:"style"`
	Hyperlink *string                 `json:"hyperlink"`
}

func DefaultTerminalScreenSpan() TerminalScreenSpan {
	return TerminalScreenSpan{
		Column:    0,
		Columns:   0,
		Text:      "",
		Style:     DefaultTerminalScreenTextStyle(),
		Hyperlink: nil,
	}
}
func (value *TerminalScreenSpan) UnmarshalJSON(data []byte) error {
	type plain TerminalScreenSpan
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"column", "columns", "text", "style"}, false); err != nil {
		return err
	}
	*value = TerminalScreenSpan(decoded)
	return nil
}
func (value TerminalScreenSpan) MarshalJSON() ([]byte, error) {
	type plain TerminalScreenSpan
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalScreenLine struct {
	Spans   []TerminalScreenSpan `json:"spans"`
	Wrapped bool                 `json:"wrapped"`
}

func DefaultTerminalScreenLine() TerminalScreenLine {
	return TerminalScreenLine{
		Spans:   []TerminalScreenSpan{},
		Wrapped: false,
	}
}
func (value *TerminalScreenLine) UnmarshalJSON(data []byte) error {
	type plain TerminalScreenLine
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"spans", "wrapped"}, false); err != nil {
		return err
	}
	*value = TerminalScreenLine(decoded)
	return nil
}
func (value TerminalScreenLine) MarshalJSON() ([]byte, error) {
	type plain TerminalScreenLine
	encoded := plain(value)
	encoded.Spans = normalizeCollections(encoded.Spans)
	return json.Marshal(encoded)
}

type TerminalScreenCursorStyle string

const (
	TerminalScreenCursorStyleBar         TerminalScreenCursorStyle = "bar"
	TerminalScreenCursorStyleBlock       TerminalScreenCursorStyle = "block"
	TerminalScreenCursorStyleUnderline   TerminalScreenCursorStyle = "underline"
	TerminalScreenCursorStyleBlockHollow TerminalScreenCursorStyle = "block_hollow"
)

func (value TerminalScreenCursorStyle) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *TerminalScreenCursorStyle) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch TerminalScreenCursorStyle(decoded) {
	case TerminalScreenCursorStyleBar, TerminalScreenCursorStyleBlock, TerminalScreenCursorStyleUnderline, TerminalScreenCursorStyleBlockHollow:
		*value = TerminalScreenCursorStyle(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value TerminalScreenCursorStyle) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *TerminalScreenCursorStyle) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type TerminalScreenCursor struct {
	Column     uint16                    `json:"column"`
	Row        uint16                    `json:"row"`
	AtWideTail bool                      `json:"at_wide_tail"`
	Blinking   bool                      `json:"blinking"`
	Style      TerminalScreenCursorStyle `json:"style"`
}

func DefaultTerminalScreenCursor() TerminalScreenCursor {
	return TerminalScreenCursor{
		Column:     0,
		Row:        0,
		AtWideTail: false,
		Blinking:   false,
		Style:      TerminalScreenCursorStyle(""),
	}
}
func (value *TerminalScreenCursor) UnmarshalJSON(data []byte) error {
	type plain TerminalScreenCursor
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"column", "row", "at_wide_tail", "blinking", "style"}, false); err != nil {
		return err
	}
	*value = TerminalScreenCursor(decoded)
	return nil
}
func (value TerminalScreenCursor) MarshalJSON() ([]byte, error) {
	type plain TerminalScreenCursor
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalScreen struct {
	VT             *TerminalVT           `json:"vt,omitempty"`
	Columns        uint16                `json:"columns"`
	Scrollback     []TerminalScreenLine  `json:"scrollback"`
	Rows           []TerminalScreenLine  `json:"rows"`
	Cursor         *TerminalScreenCursor `json:"cursor"`
	Foreground     TerminalRgb           `json:"foreground"`
	Background     TerminalRgb           `json:"background"`
	CursorColor    TerminalRgb           `json:"cursor_color"`
	BracketedPaste bool                  `json:"bracketed_paste"`
	MouseTracking  bool                  `json:"mouse_tracking"`
	Alternate      bool                  `json:"alternate"`
	Features       TerminalFeatures      `json:"features"`
	Graphics       TerminalGraphics      `json:"graphics"`
}

func DefaultTerminalScreen() TerminalScreen {
	return TerminalScreen{
		Columns:        0,
		Scrollback:     []TerminalScreenLine{},
		Rows:           []TerminalScreenLine{},
		Cursor:         nil,
		Foreground:     DefaultTerminalRgb(),
		Background:     DefaultTerminalRgb(),
		CursorColor:    DefaultTerminalRgb(),
		BracketedPaste: false,
		MouseTracking:  false,
		Alternate:      false,
		Features:       DefaultTerminalFeatures(),
		Graphics:       DefaultTerminalGraphics(),
	}
}
func (value *TerminalScreen) UnmarshalJSON(data []byte) error {
	type plain TerminalScreen
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"columns", "scrollback", "rows", "foreground", "background", "cursor_color", "bracketed_paste", "mouse_tracking", "alternate", "features", "graphics"}, false); err != nil {
		return err
	}
	*value = TerminalScreen(decoded)
	return nil
}
func (value TerminalScreen) MarshalJSON() ([]byte, error) {
	type plain TerminalScreen
	encoded := plain(value)
	encoded.Scrollback = normalizeCollections(encoded.Scrollback)
	encoded.Rows = normalizeCollections(encoded.Rows)
	return json.Marshal(encoded)
}

type TerminalScreenPatch struct {
	DroppedScrollbackRows uint32                `json:"dropped_scrollback_rows"`
	AppendedScrollback    []TerminalScreenLine  `json:"appended_scrollback"`
	Columns               uint16                `json:"columns"`
	Rows                  []TerminalScreenLine  `json:"rows"`
	Cursor                *TerminalScreenCursor `json:"cursor"`
	Foreground            TerminalRgb           `json:"foreground"`
	Background            TerminalRgb           `json:"background"`
	CursorColor           TerminalRgb           `json:"cursor_color"`
	BracketedPaste        bool                  `json:"bracketed_paste"`
	MouseTracking         bool                  `json:"mouse_tracking"`
	Alternate             bool                  `json:"alternate"`
	Features              TerminalFeatures      `json:"features"`
	Graphics              *TerminalGraphics     `json:"graphics"`
}

func DefaultTerminalScreenPatch() TerminalScreenPatch {
	return TerminalScreenPatch{
		DroppedScrollbackRows: 0,
		AppendedScrollback:    []TerminalScreenLine{},
		Columns:               0,
		Rows:                  []TerminalScreenLine{},
		Cursor:                nil,
		Foreground:            DefaultTerminalRgb(),
		Background:            DefaultTerminalRgb(),
		CursorColor:           DefaultTerminalRgb(),
		BracketedPaste:        false,
		MouseTracking:         false,
		Alternate:             false,
		Features:              DefaultTerminalFeatures(),
		Graphics:              nil,
	}
}
func (value *TerminalScreenPatch) UnmarshalJSON(data []byte) error {
	type plain TerminalScreenPatch
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"dropped_scrollback_rows", "appended_scrollback", "columns", "rows", "foreground", "background", "cursor_color", "bracketed_paste", "mouse_tracking", "alternate", "features"}, false); err != nil {
		return err
	}
	*value = TerminalScreenPatch(decoded)
	return nil
}
func (value TerminalScreenPatch) MarshalJSON() ([]byte, error) {
	type plain TerminalScreenPatch
	encoded := plain(value)
	encoded.AppendedScrollback = normalizeCollections(encoded.AppendedScrollback)
	encoded.Rows = normalizeCollections(encoded.Rows)
	return json.Marshal(encoded)
}

type TerminalScreenScreenUpdate struct {
	Kind string
	Data any
}
type TerminalScreenScreenUpdateReplacePayload struct {
	Screen TerminalScreen `json:"screen"`
}

func DefaultTerminalScreenScreenUpdateReplacePayload() TerminalScreenScreenUpdateReplacePayload {
	return TerminalScreenScreenUpdateReplacePayload{
		Screen: DefaultTerminalScreen(),
	}
}
func (value *TerminalScreenScreenUpdateReplacePayload) UnmarshalJSON(data []byte) error {
	type plain TerminalScreenScreenUpdateReplacePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"screen"}, false); err != nil {
		return err
	}
	*value = TerminalScreenScreenUpdateReplacePayload(decoded)
	return nil
}
func (value TerminalScreenScreenUpdateReplacePayload) MarshalJSON() ([]byte, error) {
	type plain TerminalScreenScreenUpdateReplacePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalScreenScreenUpdatePatchPayload struct {
	Patch TerminalScreenPatch `json:"patch"`
}

func DefaultTerminalScreenScreenUpdatePatchPayload() TerminalScreenScreenUpdatePatchPayload {
	return TerminalScreenScreenUpdatePatchPayload{
		Patch: DefaultTerminalScreenPatch(),
	}
}
func (value *TerminalScreenScreenUpdatePatchPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalScreenScreenUpdatePatchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"patch"}, false); err != nil {
		return err
	}
	*value = TerminalScreenScreenUpdatePatchPayload(decoded)
	return nil
}
func (value TerminalScreenScreenUpdatePatchPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalScreenScreenUpdatePatchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var terminalScreenScreenUpdateVariants = variantFactories{"replace": func() any { return new(TerminalScreenScreenUpdateReplacePayload) },
	"patch": func() any { return new(TerminalScreenScreenUpdatePatchPayload) }}

func (value TerminalScreenScreenUpdate) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, terminalScreenScreenUpdateVariants)
}
func (value *TerminalScreenScreenUpdate) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", terminalScreenScreenUpdateVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
