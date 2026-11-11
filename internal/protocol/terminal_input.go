// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type TerminalInputInputError struct {
	Kind string
	Data any
}
type TerminalInputInputErrorEmptyPayload struct {
	Field string `json:"field"`
}

func DefaultTerminalInputInputErrorEmptyPayload() TerminalInputInputErrorEmptyPayload {
	return TerminalInputInputErrorEmptyPayload{
		Field: "",
	}
}
func (value *TerminalInputInputErrorEmptyPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputInputErrorEmptyPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"field"}, false); err != nil {
		return err
	}
	*value = TerminalInputInputErrorEmptyPayload(decoded)
	return nil
}
func (value TerminalInputInputErrorEmptyPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputInputErrorEmptyPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputInputErrorTooLargePayload struct {
	Field  string `json:"field"`
	Max    uint64 `json:"max"`
	Actual uint64 `json:"actual"`
}

func DefaultTerminalInputInputErrorTooLargePayload() TerminalInputInputErrorTooLargePayload {
	return TerminalInputInputErrorTooLargePayload{
		Field:  "",
		Max:    0,
		Actual: 0,
	}
}
func (value *TerminalInputInputErrorTooLargePayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputInputErrorTooLargePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"field", "max", "actual"}, false); err != nil {
		return err
	}
	*value = TerminalInputInputErrorTooLargePayload(decoded)
	return nil
}
func (value TerminalInputInputErrorTooLargePayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputInputErrorTooLargePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputInputErrorTooManyPayload struct {
	Field  string `json:"field"`
	Max    uint64 `json:"max"`
	Actual uint64 `json:"actual"`
}

func DefaultTerminalInputInputErrorTooManyPayload() TerminalInputInputErrorTooManyPayload {
	return TerminalInputInputErrorTooManyPayload{
		Field:  "",
		Max:    0,
		Actual: 0,
	}
}
func (value *TerminalInputInputErrorTooManyPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputInputErrorTooManyPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"field", "max", "actual"}, false); err != nil {
		return err
	}
	*value = TerminalInputInputErrorTooManyPayload(decoded)
	return nil
}
func (value TerminalInputInputErrorTooManyPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputInputErrorTooManyPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputInputErrorInvalidPayload struct {
	Field string `json:"field"`
}

func DefaultTerminalInputInputErrorInvalidPayload() TerminalInputInputErrorInvalidPayload {
	return TerminalInputInputErrorInvalidPayload{
		Field: "",
	}
}
func (value *TerminalInputInputErrorInvalidPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputInputErrorInvalidPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"field"}, false); err != nil {
		return err
	}
	*value = TerminalInputInputErrorInvalidPayload(decoded)
	return nil
}
func (value TerminalInputInputErrorInvalidPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputInputErrorInvalidPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var terminalInputInputErrorVariants = variantFactories{"Empty": func() any { return new(TerminalInputInputErrorEmptyPayload) },
	"TooLarge": func() any { return new(TerminalInputInputErrorTooLargePayload) },
	"TooMany":  func() any { return new(TerminalInputInputErrorTooManyPayload) },
	"Invalid":  func() any { return new(TerminalInputInputErrorInvalidPayload) }}

func (value TerminalInputInputError) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, terminalInputInputErrorVariants)
}
func (value *TerminalInputInputError) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", terminalInputInputErrorVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type TerminalInput struct {
	Kind string
	Data any
}
type TerminalInputTextPayload struct {
	Text string `json:"text"`
}

func DefaultTerminalInputTextPayload() TerminalInputTextPayload {
	return TerminalInputTextPayload{
		Text: "",
	}
}
func (value *TerminalInputTextPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputTextPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text"}, true); err != nil {
		return err
	}
	*value = TerminalInputTextPayload(decoded)
	return nil
}
func (value TerminalInputTextPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputTextPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputFocusPayload struct {
	Focused bool `json:"focused"`
}

func DefaultTerminalInputFocusPayload() TerminalInputFocusPayload {
	return TerminalInputFocusPayload{
		Focused: false,
	}
}
func (value *TerminalInputFocusPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputFocusPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"focused"}, true); err != nil {
		return err
	}
	*value = TerminalInputFocusPayload(decoded)
	return nil
}
func (value TerminalInputFocusPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputFocusPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputKeyPayload struct {
	Event TerminalInputKeyEvent `json:"event"`
}

func DefaultTerminalInputKeyPayload() TerminalInputKeyPayload {
	return TerminalInputKeyPayload{
		Event: DefaultTerminalInputKeyEvent(),
	}
}
func (value *TerminalInputKeyPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputKeyPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"event"}, true); err != nil {
		return err
	}
	*value = TerminalInputKeyPayload(decoded)
	return nil
}
func (value TerminalInputKeyPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputKeyPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputPastePayload struct {
	Text string `json:"text"`
}

func DefaultTerminalInputPastePayload() TerminalInputPastePayload {
	return TerminalInputPastePayload{
		Text: "",
	}
}
func (value *TerminalInputPastePayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputPastePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text"}, true); err != nil {
		return err
	}
	*value = TerminalInputPastePayload(decoded)
	return nil
}
func (value TerminalInputPastePayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputPastePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputMousePayload struct {
	Event TerminalInputMouseEvent `json:"event"`
}

func DefaultTerminalInputMousePayload() TerminalInputMousePayload {
	return TerminalInputMousePayload{
		Event: DefaultTerminalInputMouseEvent(),
	}
}
func (value *TerminalInputMousePayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputMousePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"event"}, true); err != nil {
		return err
	}
	*value = TerminalInputMousePayload(decoded)
	return nil
}
func (value TerminalInputMousePayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputMousePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var terminalInputVariants = variantFactories{"text": func() any { return new(TerminalInputTextPayload) },
	"focus": func() any { return new(TerminalInputFocusPayload) },
	"key":   func() any { return new(TerminalInputKeyPayload) },
	"paste": func() any { return new(TerminalInputPastePayload) },
	"mouse": func() any { return new(TerminalInputMousePayload) }}

func (value TerminalInput) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, terminalInputVariants)
}
func (value *TerminalInput) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", terminalInputVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type TerminalInputMouseEvent struct {
	Action    TerminalInputMouseAction  `json:"action"`
	Button    *TerminalInputMouseButton `json:"button"`
	Modifiers TerminalInputModifiers    `json:"modifiers"`
	Column    uint16                    `json:"column"`
	Row       uint16                    `json:"row"`
	X         uint32                    `json:"x"`
	Y         uint32                    `json:"y"`
}

func DefaultTerminalInputMouseEvent() TerminalInputMouseEvent {
	return TerminalInputMouseEvent{
		Action:    TerminalInputMouseAction(""),
		Button:    nil,
		Modifiers: DefaultTerminalInputModifiers(),
		Column:    0,
		Row:       0,
		X:         0,
		Y:         0,
	}
}
func (value *TerminalInputMouseEvent) UnmarshalJSON(data []byte) error {
	type plain TerminalInputMouseEvent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"action", "modifiers", "column", "row", "x", "y"}, true); err != nil {
		return err
	}
	*value = TerminalInputMouseEvent(decoded)
	return nil
}
func (value TerminalInputMouseEvent) MarshalJSON() ([]byte, error) {
	type plain TerminalInputMouseEvent
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputMouseAction string

const (
	TerminalInputMouseActionPress   TerminalInputMouseAction = "press"
	TerminalInputMouseActionRelease TerminalInputMouseAction = "release"
	TerminalInputMouseActionMotion  TerminalInputMouseAction = "motion"
)

func (value TerminalInputMouseAction) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *TerminalInputMouseAction) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch TerminalInputMouseAction(decoded) {
	case TerminalInputMouseActionPress, TerminalInputMouseActionRelease, TerminalInputMouseActionMotion:
		*value = TerminalInputMouseAction(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value TerminalInputMouseAction) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *TerminalInputMouseAction) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type TerminalInputMouseButton string

const (
	TerminalInputMouseButtonLeft       TerminalInputMouseButton = "left"
	TerminalInputMouseButtonMiddle     TerminalInputMouseButton = "middle"
	TerminalInputMouseButtonRight      TerminalInputMouseButton = "right"
	TerminalInputMouseButtonWheelUp    TerminalInputMouseButton = "wheel_up"
	TerminalInputMouseButtonWheelDown  TerminalInputMouseButton = "wheel_down"
	TerminalInputMouseButtonWheelLeft  TerminalInputMouseButton = "wheel_left"
	TerminalInputMouseButtonWheelRight TerminalInputMouseButton = "wheel_right"
)

func (value TerminalInputMouseButton) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *TerminalInputMouseButton) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch TerminalInputMouseButton(decoded) {
	case TerminalInputMouseButtonLeft, TerminalInputMouseButtonMiddle, TerminalInputMouseButtonRight, TerminalInputMouseButtonWheelUp, TerminalInputMouseButtonWheelDown, TerminalInputMouseButtonWheelLeft, TerminalInputMouseButtonWheelRight:
		*value = TerminalInputMouseButton(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value TerminalInputMouseButton) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *TerminalInputMouseButton) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type TerminalInputKeyEvent struct {
	Key                TerminalInputKey       `json:"key"`
	Action             TerminalInputAction    `json:"action"`
	Modifiers          TerminalInputModifiers `json:"modifiers"`
	Utf8               *string                `json:"utf8"`
	UnshiftedCodepoint *uint32                `json:"unshifted_codepoint"`
}

func DefaultTerminalInputKeyEvent() TerminalInputKeyEvent {
	return TerminalInputKeyEvent{
		Key:                TerminalInputKey{},
		Action:             TerminalInputAction(""),
		Modifiers:          DefaultTerminalInputModifiers(),
		Utf8:               nil,
		UnshiftedCodepoint: nil,
	}
}
func (value *TerminalInputKeyEvent) UnmarshalJSON(data []byte) error {
	type plain TerminalInputKeyEvent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "action", "modifiers"}, true); err != nil {
		return err
	}
	*value = TerminalInputKeyEvent(decoded)
	return nil
}
func (value TerminalInputKeyEvent) MarshalJSON() ([]byte, error) {
	type plain TerminalInputKeyEvent
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputKey struct {
	Kind string
	Data any
}
type TerminalInputKeyCharacterPayload struct {
	Codepoint uint32 `json:"codepoint"`
}

func DefaultTerminalInputKeyCharacterPayload() TerminalInputKeyCharacterPayload {
	return TerminalInputKeyCharacterPayload{
		Codepoint: 0,
	}
}
func (value *TerminalInputKeyCharacterPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputKeyCharacterPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"codepoint"}, true); err != nil {
		return err
	}
	*value = TerminalInputKeyCharacterPayload(decoded)
	return nil
}
func (value TerminalInputKeyCharacterPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputKeyCharacterPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputKeyNamedPayload struct {
	Key TerminalInputNamedKey `json:"key"`
}

func DefaultTerminalInputKeyNamedPayload() TerminalInputKeyNamedPayload {
	return TerminalInputKeyNamedPayload{
		Key: TerminalInputNamedKey(""),
	}
}
func (value *TerminalInputKeyNamedPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputKeyNamedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key"}, true); err != nil {
		return err
	}
	*value = TerminalInputKeyNamedPayload(decoded)
	return nil
}
func (value TerminalInputKeyNamedPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputKeyNamedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputKeyFunctionPayload struct {
	Number uint8 `json:"number"`
}

func DefaultTerminalInputKeyFunctionPayload() TerminalInputKeyFunctionPayload {
	return TerminalInputKeyFunctionPayload{
		Number: 0,
	}
}
func (value *TerminalInputKeyFunctionPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalInputKeyFunctionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"number"}, true); err != nil {
		return err
	}
	*value = TerminalInputKeyFunctionPayload(decoded)
	return nil
}
func (value TerminalInputKeyFunctionPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalInputKeyFunctionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var terminalInputKeyVariants = variantFactories{"character": func() any { return new(TerminalInputKeyCharacterPayload) },
	"named":       func() any { return new(TerminalInputKeyNamedPayload) },
	"escape":      nil,
	"enter":       nil,
	"tab":         nil,
	"backspace":   nil,
	"insert":      nil,
	"delete":      nil,
	"home":        nil,
	"end":         nil,
	"page_up":     nil,
	"page_down":   nil,
	"arrow_up":    nil,
	"arrow_down":  nil,
	"arrow_left":  nil,
	"arrow_right": nil,
	"function":    func() any { return new(TerminalInputKeyFunctionPayload) }}

func (value TerminalInputKey) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, terminalInputKeyVariants)
}
func (value *TerminalInputKey) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", terminalInputKeyVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type TerminalInputAction string

const (
	TerminalInputActionPress   TerminalInputAction = "press"
	TerminalInputActionRepeat  TerminalInputAction = "repeat"
	TerminalInputActionRelease TerminalInputAction = "release"
)

func (value TerminalInputAction) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *TerminalInputAction) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch TerminalInputAction(decoded) {
	case TerminalInputActionPress, TerminalInputActionRepeat, TerminalInputActionRelease:
		*value = TerminalInputAction(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value TerminalInputAction) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *TerminalInputAction) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type TerminalInputModifiers struct {
	Shift    bool `json:"shift"`
	Control  bool `json:"control"`
	Alt      bool `json:"alt"`
	SuperKey bool `json:"super_key"`
	CapsLock bool `json:"caps_lock"`
	NumLock  bool `json:"num_lock"`
}

func DefaultTerminalInputModifiers() TerminalInputModifiers {
	return TerminalInputModifiers{
		Shift:    false,
		Control:  false,
		Alt:      false,
		SuperKey: false,
		CapsLock: false,
		NumLock:  false,
	}
}
func (value *TerminalInputModifiers) UnmarshalJSON(data []byte) error {
	type plain TerminalInputModifiers
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"shift", "control", "alt", "super_key", "caps_lock", "num_lock"}, true); err != nil {
		return err
	}
	*value = TerminalInputModifiers(decoded)
	return nil
}
func (value TerminalInputModifiers) MarshalJSON() ([]byte, error) {
	type plain TerminalInputModifiers
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInputNamedKey string

const (
	TerminalInputNamedKeyAltLeft        TerminalInputNamedKey = "alt_left"
	TerminalInputNamedKeyAltRight       TerminalInputNamedKey = "alt_right"
	TerminalInputNamedKeyControlLeft    TerminalInputNamedKey = "control_left"
	TerminalInputNamedKeyControlRight   TerminalInputNamedKey = "control_right"
	TerminalInputNamedKeyMetaLeft       TerminalInputNamedKey = "meta_left"
	TerminalInputNamedKeyMetaRight      TerminalInputNamedKey = "meta_right"
	TerminalInputNamedKeyShiftLeft      TerminalInputNamedKey = "shift_left"
	TerminalInputNamedKeyShiftRight     TerminalInputNamedKey = "shift_right"
	TerminalInputNamedKeyCapsLock       TerminalInputNamedKey = "caps_lock"
	TerminalInputNamedKeyNumLock        TerminalInputNamedKey = "num_lock"
	TerminalInputNamedKeyFn             TerminalInputNamedKey = "fn"
	TerminalInputNamedKeyPrintScreen    TerminalInputNamedKey = "print_screen"
	TerminalInputNamedKeyScrollLock     TerminalInputNamedKey = "scroll_lock"
	TerminalInputNamedKeyPause          TerminalInputNamedKey = "pause"
	TerminalInputNamedKeyContextMenu    TerminalInputNamedKey = "context_menu"
	TerminalInputNamedKeyNumpad0        TerminalInputNamedKey = "numpad0"
	TerminalInputNamedKeyNumpad1        TerminalInputNamedKey = "numpad1"
	TerminalInputNamedKeyNumpad2        TerminalInputNamedKey = "numpad2"
	TerminalInputNamedKeyNumpad3        TerminalInputNamedKey = "numpad3"
	TerminalInputNamedKeyNumpad4        TerminalInputNamedKey = "numpad4"
	TerminalInputNamedKeyNumpad5        TerminalInputNamedKey = "numpad5"
	TerminalInputNamedKeyNumpad6        TerminalInputNamedKey = "numpad6"
	TerminalInputNamedKeyNumpad7        TerminalInputNamedKey = "numpad7"
	TerminalInputNamedKeyNumpad8        TerminalInputNamedKey = "numpad8"
	TerminalInputNamedKeyNumpad9        TerminalInputNamedKey = "numpad9"
	TerminalInputNamedKeyNumpadAdd      TerminalInputNamedKey = "numpad_add"
	TerminalInputNamedKeyNumpadSubtract TerminalInputNamedKey = "numpad_subtract"
	TerminalInputNamedKeyNumpadMultiply TerminalInputNamedKey = "numpad_multiply"
	TerminalInputNamedKeyNumpadDivide   TerminalInputNamedKey = "numpad_divide"
	TerminalInputNamedKeyNumpadEnter    TerminalInputNamedKey = "numpad_enter"
	TerminalInputNamedKeyNumpadEqual    TerminalInputNamedKey = "numpad_equal"
	TerminalInputNamedKeyNumpadDecimal  TerminalInputNamedKey = "numpad_decimal"
	TerminalInputNamedKeyNumpadComma    TerminalInputNamedKey = "numpad_comma"
	TerminalInputNamedKeyNumpadClear    TerminalInputNamedKey = "numpad_clear"
)

func (value TerminalInputNamedKey) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *TerminalInputNamedKey) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch TerminalInputNamedKey(decoded) {
	case TerminalInputNamedKeyAltLeft, TerminalInputNamedKeyAltRight, TerminalInputNamedKeyControlLeft, TerminalInputNamedKeyControlRight, TerminalInputNamedKeyMetaLeft, TerminalInputNamedKeyMetaRight, TerminalInputNamedKeyShiftLeft, TerminalInputNamedKeyShiftRight, TerminalInputNamedKeyCapsLock, TerminalInputNamedKeyNumLock, TerminalInputNamedKeyFn, TerminalInputNamedKeyPrintScreen, TerminalInputNamedKeyScrollLock, TerminalInputNamedKeyPause, TerminalInputNamedKeyContextMenu, TerminalInputNamedKeyNumpad0, TerminalInputNamedKeyNumpad1, TerminalInputNamedKeyNumpad2, TerminalInputNamedKeyNumpad3, TerminalInputNamedKeyNumpad4, TerminalInputNamedKeyNumpad5, TerminalInputNamedKeyNumpad6, TerminalInputNamedKeyNumpad7, TerminalInputNamedKeyNumpad8, TerminalInputNamedKeyNumpad9, TerminalInputNamedKeyNumpadAdd, TerminalInputNamedKeyNumpadSubtract, TerminalInputNamedKeyNumpadMultiply, TerminalInputNamedKeyNumpadDivide, TerminalInputNamedKeyNumpadEnter, TerminalInputNamedKeyNumpadEqual, TerminalInputNamedKeyNumpadDecimal, TerminalInputNamedKeyNumpadComma, TerminalInputNamedKeyNumpadClear:
		*value = TerminalInputNamedKey(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value TerminalInputNamedKey) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *TerminalInputNamedKey) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
