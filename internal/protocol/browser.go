// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type BrowserAction struct {
	Kind string
	Data any
}
type BrowserActionReadPayload struct {
	Tab *uint64 `json:"tab"`
}

func DefaultBrowserActionReadPayload() BrowserActionReadPayload {
	return BrowserActionReadPayload{
		Tab: nil,
	}
}
func (value *BrowserActionReadPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionReadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = BrowserActionReadPayload(decoded)
	return nil
}
func (value BrowserActionReadPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionReadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionNavigatePayload struct {
	Url        string  `json:"url"`
	Tab        *uint64 `json:"tab"`
	Background bool    `json:"background"`
}

func DefaultBrowserActionNavigatePayload() BrowserActionNavigatePayload {
	return BrowserActionNavigatePayload{
		Url:        "",
		Tab:        nil,
		Background: false,
	}
}
func (value *BrowserActionNavigatePayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionNavigatePayload
	decoded := plain{}
	decoded.Background = false
	if err := DecodeObject(data, &decoded, []string{"url"}, true); err != nil {
		return err
	}
	*value = BrowserActionNavigatePayload(decoded)
	return nil
}
func (value BrowserActionNavigatePayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionNavigatePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionClickPayload struct {
	Tab      uint64 `json:"tab"`
	Snapshot string `json:"snapshot"`
	Element  uint64 `json:"element"`
}

func DefaultBrowserActionClickPayload() BrowserActionClickPayload {
	return BrowserActionClickPayload{
		Tab:      0,
		Snapshot: "",
		Element:  0,
	}
}
func (value *BrowserActionClickPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionClickPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab", "snapshot", "element"}, true); err != nil {
		return err
	}
	*value = BrowserActionClickPayload(decoded)
	return nil
}
func (value BrowserActionClickPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionClickPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionInputPayload struct {
	Tab      uint64 `json:"tab"`
	Snapshot string `json:"snapshot"`
	Element  uint64 `json:"element"`
	Text     string `json:"text"`
}

func DefaultBrowserActionInputPayload() BrowserActionInputPayload {
	return BrowserActionInputPayload{
		Tab:      0,
		Snapshot: "",
		Element:  0,
		Text:     "",
	}
}
func (value *BrowserActionInputPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionInputPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab", "snapshot", "element", "text"}, true); err != nil {
		return err
	}
	*value = BrowserActionInputPayload(decoded)
	return nil
}
func (value BrowserActionInputPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionInputPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionScrollPayload struct {
	Tab       uint64           `json:"tab"`
	Direction BrowserDirection `json:"direction"`
}

func DefaultBrowserActionScrollPayload() BrowserActionScrollPayload {
	return BrowserActionScrollPayload{
		Tab:       0,
		Direction: BrowserDirection(""),
	}
}
func (value *BrowserActionScrollPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionScrollPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab", "direction"}, true); err != nil {
		return err
	}
	*value = BrowserActionScrollPayload(decoded)
	return nil
}
func (value BrowserActionScrollPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionScrollPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionBackPayload struct {
	Tab uint64 `json:"tab"`
}

func DefaultBrowserActionBackPayload() BrowserActionBackPayload {
	return BrowserActionBackPayload{
		Tab: 0,
	}
}
func (value *BrowserActionBackPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionBackPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab"}, true); err != nil {
		return err
	}
	*value = BrowserActionBackPayload(decoded)
	return nil
}
func (value BrowserActionBackPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionBackPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionForwardPayload struct {
	Tab uint64 `json:"tab"`
}

func DefaultBrowserActionForwardPayload() BrowserActionForwardPayload {
	return BrowserActionForwardPayload{
		Tab: 0,
	}
}
func (value *BrowserActionForwardPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionForwardPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab"}, true); err != nil {
		return err
	}
	*value = BrowserActionForwardPayload(decoded)
	return nil
}
func (value BrowserActionForwardPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionForwardPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionRefreshPayload struct {
	Tab uint64 `json:"tab"`
}

func DefaultBrowserActionRefreshPayload() BrowserActionRefreshPayload {
	return BrowserActionRefreshPayload{
		Tab: 0,
	}
}
func (value *BrowserActionRefreshPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionRefreshPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab"}, true); err != nil {
		return err
	}
	*value = BrowserActionRefreshPayload(decoded)
	return nil
}
func (value BrowserActionRefreshPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionRefreshPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionOpenPayload struct {
	Url        string `json:"url"`
	Background bool   `json:"background"`
}

func DefaultBrowserActionOpenPayload() BrowserActionOpenPayload {
	return BrowserActionOpenPayload{
		Url:        "",
		Background: false,
	}
}
func (value *BrowserActionOpenPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionOpenPayload
	decoded := plain{}
	decoded.Background = false
	if err := DecodeObject(data, &decoded, []string{"url"}, true); err != nil {
		return err
	}
	*value = BrowserActionOpenPayload(decoded)
	return nil
}
func (value BrowserActionOpenPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionOpenPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionClosePayload struct {
	Tab uint64 `json:"tab"`
}

func DefaultBrowserActionClosePayload() BrowserActionClosePayload {
	return BrowserActionClosePayload{
		Tab: 0,
	}
}
func (value *BrowserActionClosePayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionClosePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab"}, true); err != nil {
		return err
	}
	*value = BrowserActionClosePayload(decoded)
	return nil
}
func (value BrowserActionClosePayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionClosePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionFocusPayload struct {
	Tab uint64 `json:"tab"`
}

func DefaultBrowserActionFocusPayload() BrowserActionFocusPayload {
	return BrowserActionFocusPayload{
		Tab: 0,
	}
}
func (value *BrowserActionFocusPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionFocusPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab"}, true); err != nil {
		return err
	}
	*value = BrowserActionFocusPayload(decoded)
	return nil
}
func (value BrowserActionFocusPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionFocusPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionSelectPayload struct {
	Tab      uint64 `json:"tab"`
	Snapshot string `json:"snapshot"`
	Element  uint64 `json:"element"`
	Value    string `json:"value"`
}

func DefaultBrowserActionSelectPayload() BrowserActionSelectPayload {
	return BrowserActionSelectPayload{
		Tab:      0,
		Snapshot: "",
		Element:  0,
		Value:    "",
	}
}
func (value *BrowserActionSelectPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionSelectPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab", "snapshot", "element", "value"}, true); err != nil {
		return err
	}
	*value = BrowserActionSelectPayload(decoded)
	return nil
}
func (value BrowserActionSelectPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionSelectPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionHoverPayload struct {
	Tab      uint64 `json:"tab"`
	Snapshot string `json:"snapshot"`
	Element  uint64 `json:"element"`
}

func DefaultBrowserActionHoverPayload() BrowserActionHoverPayload {
	return BrowserActionHoverPayload{
		Tab:      0,
		Snapshot: "",
		Element:  0,
	}
}
func (value *BrowserActionHoverPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionHoverPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab", "snapshot", "element"}, true); err != nil {
		return err
	}
	*value = BrowserActionHoverPayload(decoded)
	return nil
}
func (value BrowserActionHoverPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionHoverPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionKeyPayload struct {
	Tab      uint64     `json:"tab"`
	Snapshot string     `json:"snapshot"`
	Element  uint64     `json:"element"`
	Key      BrowserKey `json:"key"`
}

func DefaultBrowserActionKeyPayload() BrowserActionKeyPayload {
	return BrowserActionKeyPayload{
		Tab:      0,
		Snapshot: "",
		Element:  0,
		Key:      BrowserKey(""),
	}
}
func (value *BrowserActionKeyPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionKeyPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab", "snapshot", "element", "key"}, true); err != nil {
		return err
	}
	*value = BrowserActionKeyPayload(decoded)
	return nil
}
func (value BrowserActionKeyPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionKeyPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionFramePayload struct {
	Tab      uint64  `json:"tab"`
	Snapshot string  `json:"snapshot"`
	Element  *uint64 `json:"element"`
}

func DefaultBrowserActionFramePayload() BrowserActionFramePayload {
	return BrowserActionFramePayload{
		Tab:      0,
		Snapshot: "",
		Element:  nil,
	}
}
func (value *BrowserActionFramePayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionFramePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab", "snapshot"}, true); err != nil {
		return err
	}
	*value = BrowserActionFramePayload(decoded)
	return nil
}
func (value BrowserActionFramePayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionFramePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionWaitPayload struct {
	Tab       uint64           `json:"tab"`
	Condition BrowserCondition `json:"condition"`
	TimeoutMs uint32           `json:"timeout_ms"`
}

func DefaultBrowserActionWaitPayload() BrowserActionWaitPayload {
	return BrowserActionWaitPayload{
		Tab:       0,
		Condition: BrowserCondition{},
		TimeoutMs: 0,
	}
}
func (value *BrowserActionWaitPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionWaitPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tab", "condition", "timeout_ms"}, true); err != nil {
		return err
	}
	*value = BrowserActionWaitPayload(decoded)
	return nil
}
func (value BrowserActionWaitPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionWaitPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserActionScreenshotPayload struct {
	Tab    uint64         `json:"tab"`
	Target *BrowserTarget `json:"target,omitempty"`
	Scale  BrowserScale   `json:"scale"`
	Format BrowserFormat  `json:"format"`
	Save   bool           `json:"save"`
}

func DefaultBrowserActionScreenshotPayload() BrowserActionScreenshotPayload {
	return BrowserActionScreenshotPayload{
		Tab:    0,
		Target: nil,
		Scale:  BrowserScaleCss,
		Format: BrowserFormatJpeg,
		Save:   false,
	}
}
func (value *BrowserActionScreenshotPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserActionScreenshotPayload
	decoded := plain{}
	decoded.Target = nil
	decoded.Scale = BrowserScaleCss
	decoded.Format = BrowserFormatJpeg
	decoded.Save = false
	if err := DecodeObject(data, &decoded, []string{"tab"}, true); err != nil {
		return err
	}
	*value = BrowserActionScreenshotPayload(decoded)
	return nil
}
func (value BrowserActionScreenshotPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserActionScreenshotPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var browserActionVariants = variantFactories{"tabs": nil,
	"read":       func() any { return new(BrowserActionReadPayload) },
	"navigate":   func() any { return new(BrowserActionNavigatePayload) },
	"click":      func() any { return new(BrowserActionClickPayload) },
	"input":      func() any { return new(BrowserActionInputPayload) },
	"scroll":     func() any { return new(BrowserActionScrollPayload) },
	"back":       func() any { return new(BrowserActionBackPayload) },
	"forward":    func() any { return new(BrowserActionForwardPayload) },
	"refresh":    func() any { return new(BrowserActionRefreshPayload) },
	"open":       func() any { return new(BrowserActionOpenPayload) },
	"close":      func() any { return new(BrowserActionClosePayload) },
	"focus":      func() any { return new(BrowserActionFocusPayload) },
	"select":     func() any { return new(BrowserActionSelectPayload) },
	"hover":      func() any { return new(BrowserActionHoverPayload) },
	"key":        func() any { return new(BrowserActionKeyPayload) },
	"frame":      func() any { return new(BrowserActionFramePayload) },
	"wait":       func() any { return new(BrowserActionWaitPayload) },
	"screenshot": func() any { return new(BrowserActionScreenshotPayload) }}

func (value BrowserAction) MarshalJSON() ([]byte, error) {
	return MarshalUnion("action", "", value.Kind, value.Data, browserActionVariants)
}
func (value *BrowserAction) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "action", "", browserActionVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type BrowserTarget struct {
	Kind string
	Data any
}
type BrowserTargetSelectorPayload struct {
	Selector string `json:"selector"`
}

func DefaultBrowserTargetSelectorPayload() BrowserTargetSelectorPayload {
	return BrowserTargetSelectorPayload{
		Selector: "",
	}
}
func (value *BrowserTargetSelectorPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserTargetSelectorPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"selector"}, true); err != nil {
		return err
	}
	*value = BrowserTargetSelectorPayload(decoded)
	return nil
}
func (value BrowserTargetSelectorPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserTargetSelectorPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserTargetElementPayload struct {
	Snapshot string `json:"snapshot"`
	Element  uint64 `json:"element"`
}

func DefaultBrowserTargetElementPayload() BrowserTargetElementPayload {
	return BrowserTargetElementPayload{
		Snapshot: "",
		Element:  0,
	}
}
func (value *BrowserTargetElementPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserTargetElementPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"snapshot", "element"}, true); err != nil {
		return err
	}
	*value = BrowserTargetElementPayload(decoded)
	return nil
}
func (value BrowserTargetElementPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserTargetElementPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserTargetRectPayload struct {
	X      uint32 `json:"x"`
	Y      uint32 `json:"y"`
	Width  uint32 `json:"width"`
	Height uint32 `json:"height"`
}

func DefaultBrowserTargetRectPayload() BrowserTargetRectPayload {
	return BrowserTargetRectPayload{
		X:      0,
		Y:      0,
		Width:  0,
		Height: 0,
	}
}
func (value *BrowserTargetRectPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserTargetRectPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"x", "y", "width", "height"}, true); err != nil {
		return err
	}
	*value = BrowserTargetRectPayload(decoded)
	return nil
}
func (value BrowserTargetRectPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserTargetRectPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var browserTargetVariants = variantFactories{"selector": func() any { return new(BrowserTargetSelectorPayload) },
	"element": func() any { return new(BrowserTargetElementPayload) },
	"rect":    func() any { return new(BrowserTargetRectPayload) }}

func (value BrowserTarget) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, browserTargetVariants)
}
func (value *BrowserTarget) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", browserTargetVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type BrowserScale string

const (
	BrowserScaleCss    BrowserScale = "css"
	BrowserScaleDevice BrowserScale = "device"
)

func (value BrowserScale) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *BrowserScale) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch BrowserScale(decoded) {
	case BrowserScaleCss, BrowserScaleDevice:
		*value = BrowserScale(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value BrowserScale) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *BrowserScale) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type BrowserFormat string

const (
	BrowserFormatJpeg BrowserFormat = "jpeg"
	BrowserFormatPng  BrowserFormat = "png"
)

func (value BrowserFormat) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *BrowserFormat) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch BrowserFormat(decoded) {
	case BrowserFormatJpeg, BrowserFormatPng:
		*value = BrowserFormat(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value BrowserFormat) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *BrowserFormat) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type BrowserKey string

const (
	BrowserKeyEnter      BrowserKey = "Enter"
	BrowserKeyEscape     BrowserKey = "Escape"
	BrowserKeyTab        BrowserKey = "Tab"
	BrowserKeyBackspace  BrowserKey = "Backspace"
	BrowserKeyArrowUp    BrowserKey = "ArrowUp"
	BrowserKeyArrowDown  BrowserKey = "ArrowDown"
	BrowserKeyArrowLeft  BrowserKey = "ArrowLeft"
	BrowserKeyArrowRight BrowserKey = "ArrowRight"
)

func (value BrowserKey) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *BrowserKey) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch BrowserKey(decoded) {
	case BrowserKeyEnter, BrowserKeyEscape, BrowserKeyTab, BrowserKeyBackspace, BrowserKeyArrowUp, BrowserKeyArrowDown, BrowserKeyArrowLeft, BrowserKeyArrowRight:
		*value = BrowserKey(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value BrowserKey) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *BrowserKey) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type BrowserCondition struct {
	Kind string
	Data any
}
type BrowserConditionTextPayload struct {
	Text string `json:"text"`
}

func DefaultBrowserConditionTextPayload() BrowserConditionTextPayload {
	return BrowserConditionTextPayload{
		Text: "",
	}
}
func (value *BrowserConditionTextPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserConditionTextPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text"}, true); err != nil {
		return err
	}
	*value = BrowserConditionTextPayload(decoded)
	return nil
}
func (value BrowserConditionTextPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserConditionTextPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserConditionVisiblePayload struct {
	Selector string `json:"selector"`
}

func DefaultBrowserConditionVisiblePayload() BrowserConditionVisiblePayload {
	return BrowserConditionVisiblePayload{
		Selector: "",
	}
}
func (value *BrowserConditionVisiblePayload) UnmarshalJSON(data []byte) error {
	type plain BrowserConditionVisiblePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"selector"}, true); err != nil {
		return err
	}
	*value = BrowserConditionVisiblePayload(decoded)
	return nil
}
func (value BrowserConditionVisiblePayload) MarshalJSON() ([]byte, error) {
	type plain BrowserConditionVisiblePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserConditionHiddenPayload struct {
	Selector string `json:"selector"`
}

func DefaultBrowserConditionHiddenPayload() BrowserConditionHiddenPayload {
	return BrowserConditionHiddenPayload{
		Selector: "",
	}
}
func (value *BrowserConditionHiddenPayload) UnmarshalJSON(data []byte) error {
	type plain BrowserConditionHiddenPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"selector"}, true); err != nil {
		return err
	}
	*value = BrowserConditionHiddenPayload(decoded)
	return nil
}
func (value BrowserConditionHiddenPayload) MarshalJSON() ([]byte, error) {
	type plain BrowserConditionHiddenPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var browserConditionVariants = variantFactories{"text": func() any { return new(BrowserConditionTextPayload) },
	"visible": func() any { return new(BrowserConditionVisiblePayload) },
	"hidden":  func() any { return new(BrowserConditionHiddenPayload) }}

func (value BrowserCondition) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, browserConditionVariants)
}
func (value *BrowserCondition) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", browserConditionVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type BrowserDirection string

const (
	BrowserDirectionUp   BrowserDirection = "up"
	BrowserDirectionDown BrowserDirection = "down"
)

func (value BrowserDirection) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *BrowserDirection) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch BrowserDirection(decoded) {
	case BrowserDirectionUp, BrowserDirectionDown:
		*value = BrowserDirection(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value BrowserDirection) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *BrowserDirection) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type BrowserCall struct {
	ID          RequestID     `json:"id"`
	Session     SessionID     `json:"session"`
	Action      BrowserAction `json:"action"`
	ExpiresAtMs uint64        `json:"expires_at_ms"`
}

func DefaultBrowserCall() BrowserCall {
	return BrowserCall{
		ID:          RequestID{},
		Session:     SessionID{},
		Action:      BrowserAction{},
		ExpiresAtMs: 0,
	}
}
func (value *BrowserCall) UnmarshalJSON(data []byte) error {
	type plain BrowserCall
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "session", "action", "expires_at_ms"}, false); err != nil {
		return err
	}
	*value = BrowserCall(decoded)
	return nil
}
func (value BrowserCall) MarshalJSON() ([]byte, error) {
	type plain BrowserCall
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BrowserResult = Result[json.RawMessage, Fault]
