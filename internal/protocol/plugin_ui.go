// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type PluginUiSlot string

const (
	PluginUiSlotHeader      PluginUiSlot = "header"
	PluginUiSlotComposer    PluginUiSlot = "composer"
	PluginUiSlotModel       PluginUiSlot = "model"
	PluginUiSlotContext     PluginUiSlot = "context"
	PluginUiSlotStatistics  PluginUiSlot = "statistics"
	PluginUiSlotFooter      PluginUiSlot = "footer"
	PluginUiSlotProvider    PluginUiSlot = "provider"
	PluginUiSlotStatus      PluginUiSlot = "status"
	PluginUiSlotProject     PluginUiSlot = "project"
	PluginUiSlotProjectMenu PluginUiSlot = "project_menu"
	PluginUiSlotCommands    PluginUiSlot = "commands"
)

func (value PluginUiSlot) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginUiSlotHeader, PluginUiSlotComposer, PluginUiSlotModel, PluginUiSlotContext, PluginUiSlotStatistics, PluginUiSlotFooter, PluginUiSlotProvider, PluginUiSlotStatus, PluginUiSlotProject, PluginUiSlotProjectMenu, PluginUiSlotCommands:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginUiSlot %q", value)
	}
}
func (value *PluginUiSlot) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginUiSlot(decoded) {
	case PluginUiSlotHeader, PluginUiSlotComposer, PluginUiSlotModel, PluginUiSlotContext, PluginUiSlotStatistics, PluginUiSlotFooter, PluginUiSlotProvider, PluginUiSlotStatus, PluginUiSlotProject, PluginUiSlotProjectMenu, PluginUiSlotCommands:
		*value = PluginUiSlot(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginUiSlot %q", decoded)
	}
}
func (value PluginUiSlot) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginUiSlot) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginUiKind string

const (
	PluginUiKindButton    PluginUiKind = "button"
	PluginUiKindPopover   PluginUiKind = "popover"
	PluginUiKindToggle    PluginUiKind = "toggle"
	PluginUiKindSelect    PluginUiKind = "select"
	PluginUiKindMenu      PluginUiKind = "menu"
	PluginUiKindPicker    PluginUiKind = "picker"
	PluginUiKindMetric    PluginUiKind = "metric"
	PluginUiKindIndicator PluginUiKind = "indicator"
)

func (value PluginUiKind) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginUiKindButton, PluginUiKindPopover, PluginUiKindToggle, PluginUiKindSelect, PluginUiKindMenu, PluginUiKindPicker, PluginUiKindMetric, PluginUiKindIndicator:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginUiKind %q", value)
	}
}
func (value *PluginUiKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginUiKind(decoded) {
	case PluginUiKindButton, PluginUiKindPopover, PluginUiKindToggle, PluginUiKindSelect, PluginUiKindMenu, PluginUiKindPicker, PluginUiKindMetric, PluginUiKindIndicator:
		*value = PluginUiKind(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginUiKind %q", decoded)
	}
}
func (value PluginUiKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginUiKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginUiOverflow string

const (
	PluginUiOverflowAuto PluginUiOverflow = "auto"
	PluginUiOverflowMenu PluginUiOverflow = "menu"
)

func (value PluginUiOverflow) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginUiOverflowAuto, PluginUiOverflowMenu:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginUiOverflow %q", value)
	}
}
func (value *PluginUiOverflow) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginUiOverflow(decoded) {
	case PluginUiOverflowAuto, PluginUiOverflowMenu:
		*value = PluginUiOverflow(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginUiOverflow %q", decoded)
	}
}
func (value PluginUiOverflow) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginUiOverflow) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginUiAlign string

const (
	PluginUiAlignStart PluginUiAlign = "start"
	PluginUiAlignEnd   PluginUiAlign = "end"
)

func (value PluginUiAlign) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginUiAlignStart, PluginUiAlignEnd:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginUiAlign %q", value)
	}
}
func (value *PluginUiAlign) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginUiAlign(decoded) {
	case PluginUiAlignStart, PluginUiAlignEnd:
		*value = PluginUiAlign(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginUiAlign %q", decoded)
	}
}
func (value PluginUiAlign) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginUiAlign) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginUiIntent string

const (
	PluginUiIntentGitBranches    PluginUiIntent = "git_branches"
	PluginUiIntentWorktrees      PluginUiIntent = "worktrees"
	PluginUiIntentCreateWorktree PluginUiIntent = "create_worktree"
	PluginUiIntentForkWorktree   PluginUiIntent = "fork_worktree"
)

func (value PluginUiIntent) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginUiIntentGitBranches, PluginUiIntentWorktrees, PluginUiIntentCreateWorktree, PluginUiIntentForkWorktree:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginUiIntent %q", value)
	}
}
func (value *PluginUiIntent) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginUiIntent(decoded) {
	case PluginUiIntentGitBranches, PluginUiIntentWorktrees, PluginUiIntentCreateWorktree, PluginUiIntentForkWorktree:
		*value = PluginUiIntent(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginUiIntent %q", decoded)
	}
}
func (value PluginUiIntent) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginUiIntent) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginUiContribution struct {
	Intent   *string                 `json:"intent"`
	ID       string                  `json:"id"`
	Slot     PluginUiSlot            `json:"slot"`
	Kind     PluginUiKind            `json:"kind"`
	Label    PluginDesktopNavigation `json:"label"`
	Icon     *string                 `json:"icon"`
	Handler  *string                 `json:"handler"`
	Order    int16                   `json:"order"`
	Overflow PluginUiOverflow        `json:"overflow"`
	Align    PluginUiAlign           `json:"align"`
	Choices  []PluginUiChoice        `json:"choices"`
	Command  *PluginUiCommand        `json:"command,omitempty"`
	Width    *uint16                 `json:"width,omitempty"`
	Hover    *PluginUiHover          `json:"hover,omitempty"`
}

func DefaultPluginUiContribution() PluginUiContribution {
	return PluginUiContribution{
		Intent:   nil,
		ID:       "",
		Slot:     PluginUiSlot(""),
		Kind:     PluginUiKind(""),
		Label:    DefaultPluginDesktopNavigation(),
		Icon:     nil,
		Handler:  nil,
		Order:    0,
		Overflow: PluginUiOverflowAuto,
		Align:    PluginUiAlignStart,
		Choices:  []PluginUiChoice{},
		Command:  nil,
		Width:    nil,
		Hover:    nil,
	}
}
func (value *PluginUiContribution) UnmarshalJSON(data []byte) error {
	type plain PluginUiContribution
	decoded := plain{}
	decoded.Order = 0
	decoded.Overflow = PluginUiOverflowAuto
	decoded.Align = PluginUiAlignStart
	decoded.Choices = []PluginUiChoice{}
	decoded.Command = nil
	decoded.Width = nil
	decoded.Hover = nil
	if err := DecodeObject(data, &decoded, []string{"id", "slot", "kind", "label"}, true); err != nil {
		return err
	}
	*value = PluginUiContribution(decoded)
	return nil
}
func (value PluginUiContribution) MarshalJSON() ([]byte, error) {
	type plain PluginUiContribution
	encoded := plain(value)
	encoded.Choices = normalizeCollections(encoded.Choices)
	return json.Marshal(encoded)
}

type PluginUiHover struct {
	OpenMs  uint16 `json:"open_ms"`
	CloseMs uint16 `json:"close_ms"`
}

func DefaultPluginUiHover() PluginUiHover {
	return PluginUiHover{
		OpenMs:  0,
		CloseMs: 0,
	}
}
func (value *PluginUiHover) UnmarshalJSON(data []byte) error {
	type plain PluginUiHover
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"open_ms", "close_ms"}, true); err != nil {
		return err
	}
	*value = PluginUiHover(decoded)
	return nil
}
func (value PluginUiHover) MarshalJSON() ([]byte, error) {
	type plain PluginUiHover
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUiCommand struct {
	Name string              `json:"name"`
	Kind PluginUiCommandKind `json:"kind"`
}

func DefaultPluginUiCommand() PluginUiCommand {
	return PluginUiCommand{
		Name: "",
		Kind: PluginUiCommandKind(""),
	}
}
func (value *PluginUiCommand) UnmarshalJSON(data []byte) error {
	type plain PluginUiCommand
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "kind"}, true); err != nil {
		return err
	}
	*value = PluginUiCommand(decoded)
	return nil
}
func (value PluginUiCommand) MarshalJSON() ([]byte, error) {
	type plain PluginUiCommand
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUiCommandKind string

const (
	PluginUiCommandKindInvoke  PluginUiCommandKind = "invoke"
	PluginUiCommandKindMessage PluginUiCommandKind = "message"
)

func (value PluginUiCommandKind) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginUiCommandKindInvoke, PluginUiCommandKindMessage:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginUiCommandKind %q", value)
	}
}
func (value *PluginUiCommandKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginUiCommandKind(decoded) {
	case PluginUiCommandKindInvoke, PluginUiCommandKindMessage:
		*value = PluginUiCommandKind(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginUiCommandKind %q", decoded)
	}
}
func (value PluginUiCommandKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginUiCommandKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginUiChoice struct {
	ID          string                   `json:"id"`
	Label       PluginDesktopNavigation  `json:"label"`
	Enabled     bool                     `json:"enabled"`
	Description *PluginDesktopNavigation `json:"description"`
	Icon        *string                  `json:"icon"`
	Group       *PluginDesktopNavigation `json:"group"`
	Children    []PluginUiChoice         `json:"children,omitempty"`
}

func DefaultPluginUiChoice() PluginUiChoice {
	return PluginUiChoice{
		ID:          "",
		Label:       DefaultPluginDesktopNavigation(),
		Enabled:     false,
		Description: nil,
		Icon:        nil,
		Group:       nil,
		Children:    []PluginUiChoice{},
	}
}
func (value *PluginUiChoice) UnmarshalJSON(data []byte) error {
	type plain PluginUiChoice
	decoded := plain{}
	decoded.Enabled = true
	decoded.Children = []PluginUiChoice{}
	if err := DecodeObject(data, &decoded, []string{"id", "label"}, true); err != nil {
		return err
	}
	*value = PluginUiChoice(decoded)
	return nil
}
func (value PluginUiChoice) MarshalJSON() ([]byte, error) {
	type plain PluginUiChoice
	encoded := plain(value)
	encoded.Children = normalizeCollections(encoded.Children)
	return json.Marshal(encoded)
}

type PluginUiDetail struct {
	Label    PluginDesktopNavigation `json:"label"`
	Value    string                  `json:"value"`
	Progress *json.Number            `json:"progress,omitempty"`
}

func DefaultPluginUiDetail() PluginUiDetail {
	return PluginUiDetail{
		Label:    DefaultPluginDesktopNavigation(),
		Value:    "",
		Progress: nil,
	}
}
func (value *PluginUiDetail) UnmarshalJSON(data []byte) error {
	type plain PluginUiDetail
	decoded := plain{}
	decoded.Progress = nil
	if err := DecodeObject(data, &decoded, []string{"label", "value"}, false); err != nil {
		return err
	}
	*value = PluginUiDetail(decoded)
	return nil
}
func (value PluginUiDetail) MarshalJSON() ([]byte, error) {
	type plain PluginUiDetail
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUiProviderScope struct {
	Provider   ProviderID    `json:"provider"`
	Revision   string        `json:"revision"`
	Credential CredentialRef `json:"credential"`
}

func DefaultPluginUiProviderScope() PluginUiProviderScope {
	return PluginUiProviderScope{
		Provider:   ProviderID{},
		Revision:   "",
		Credential: DefaultCredentialRef(),
	}
}
func (value *PluginUiProviderScope) UnmarshalJSON(data []byte) error {
	type plain PluginUiProviderScope
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "revision", "credential"}, true); err != nil {
		return err
	}
	*value = PluginUiProviderScope(decoded)
	return nil
}
func (value PluginUiProviderScope) MarshalJSON() ([]byte, error) {
	type plain PluginUiProviderScope
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUiState struct {
	ID       string                   `json:"id"`
	Scope    *PluginUiProviderScope   `json:"scope,omitempty"`
	Label    *PluginDesktopNavigation `json:"label"`
	Icon     *string                  `json:"icon"`
	Dropdown bool                     `json:"dropdown"`
	Visible  bool                     `json:"visible"`
	Enabled  bool                     `json:"enabled"`
	Loading  bool                     `json:"loading"`
	Value    json.RawMessage          `json:"value"`
	Choices  *[]PluginUiChoice        `json:"choices"`
	Details  []PluginUiDetail         `json:"details"`
	Segments []PluginUiSegment        `json:"segments"`
	ReplyTo  *uint64                  `json:"reply_to"`
}

func DefaultPluginUiState() PluginUiState {
	return PluginUiState{
		ID:       "",
		Scope:    nil,
		Label:    nil,
		Icon:     nil,
		Dropdown: false,
		Visible:  false,
		Enabled:  false,
		Loading:  false,
		Value:    json.RawMessage("null"),
		Choices:  nil,
		Details:  []PluginUiDetail{},
		Segments: []PluginUiSegment{},
		ReplyTo:  nil,
	}
}
func (value *PluginUiState) UnmarshalJSON(data []byte) error {
	type plain PluginUiState
	decoded := plain{}
	decoded.Scope = nil
	decoded.Dropdown = false
	decoded.Visible = true
	decoded.Enabled = true
	decoded.Loading = false
	decoded.Value = json.RawMessage("null")
	decoded.Choices = nil
	decoded.Details = []PluginUiDetail{}
	decoded.Segments = []PluginUiSegment{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = PluginUiState(decoded)
	return nil
}
func (value PluginUiState) MarshalJSON() ([]byte, error) {
	type plain PluginUiState
	encoded := plain(value)
	encoded.Choices = normalizeCollections(encoded.Choices)
	encoded.Details = normalizeCollections(encoded.Details)
	encoded.Segments = normalizeCollections(encoded.Segments)
	return json.Marshal(encoded)
}

type PluginUiEventKind string

const (
	PluginUiEventKindInvoke PluginUiEventKind = "invoke"
	PluginUiEventKindChange PluginUiEventKind = "change"
	PluginUiEventKindSearch PluginUiEventKind = "search"
)

func (value PluginUiEventKind) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginUiEventKindInvoke, PluginUiEventKindChange, PluginUiEventKindSearch:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginUiEventKind %q", value)
	}
}
func (value *PluginUiEventKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginUiEventKind(decoded) {
	case PluginUiEventKindInvoke, PluginUiEventKindChange, PluginUiEventKindSearch:
		*value = PluginUiEventKind(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginUiEventKind %q", decoded)
	}
}
func (value PluginUiEventKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginUiEventKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginUiEvent struct {
	Sequence uint64            `json:"sequence"`
	ID       string            `json:"id"`
	Handler  string            `json:"handler"`
	Kind     PluginUiEventKind `json:"kind"`
	Value    json.RawMessage   `json:"value"`
}

func DefaultPluginUiEvent() PluginUiEvent {
	return PluginUiEvent{
		Sequence: 0,
		ID:       "",
		Handler:  "",
		Kind:     PluginUiEventKind(""),
		Value:    json.RawMessage("null"),
	}
}
func (value *PluginUiEvent) UnmarshalJSON(data []byte) error {
	type plain PluginUiEvent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"sequence", "id", "handler", "kind", "value"}, false); err != nil {
		return err
	}
	*value = PluginUiEvent(decoded)
	return nil
}
func (value PluginUiEvent) MarshalJSON() ([]byte, error) {
	type plain PluginUiEvent
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUiIndicator struct {
	Percent *float64     `json:"percent"`
	Open    bool         `json:"open"`
	Loading bool         `json:"loading"`
	Tone    PluginUiTone `json:"tone"`
	Hint    string       `json:"hint"`
}

func DefaultPluginUiIndicator() PluginUiIndicator {
	return PluginUiIndicator{
		Percent: nil,
		Open:    false,
		Loading: false,
		Tone:    PluginUiTone(""),
		Hint:    "",
	}
}
func (value *PluginUiIndicator) UnmarshalJSON(data []byte) error {
	type plain PluginUiIndicator
	decoded := plain{}
	decoded.Open = false
	decoded.Loading = false
	if err := DecodeObject(data, &decoded, []string{"tone", "hint"}, true); err != nil {
		return err
	}
	*value = PluginUiIndicator(decoded)
	return nil
}
func (value PluginUiIndicator) MarshalJSON() ([]byte, error) {
	type plain PluginUiIndicator
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUiTone string

const (
	PluginUiToneMuted   PluginUiTone = "muted"
	PluginUiToneWarning PluginUiTone = "warning"
	PluginUiToneDanger  PluginUiTone = "danger"
)

func (value PluginUiTone) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginUiToneMuted, PluginUiToneWarning, PluginUiToneDanger:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginUiTone %q", value)
	}
}
func (value *PluginUiTone) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginUiTone(decoded) {
	case PluginUiToneMuted, PluginUiToneWarning, PluginUiToneDanger:
		*value = PluginUiTone(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginUiTone %q", decoded)
	}
}
func (value PluginUiTone) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginUiTone) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginUiSegment struct {
	Label PluginDesktopNavigation `json:"label"`
	Tone  PluginUiSegmentTone     `json:"tone"`
}

func DefaultPluginUiSegment() PluginUiSegment {
	return PluginUiSegment{
		Label: DefaultPluginDesktopNavigation(),
		Tone:  PluginUiSegmentTone(""),
	}
}
func (value *PluginUiSegment) UnmarshalJSON(data []byte) error {
	type plain PluginUiSegment
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"label", "tone"}, false); err != nil {
		return err
	}
	*value = PluginUiSegment(decoded)
	return nil
}
func (value PluginUiSegment) MarshalJSON() ([]byte, error) {
	type plain PluginUiSegment
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUiSegmentTone string

const (
	PluginUiSegmentToneAccent  PluginUiSegmentTone = "accent"
	PluginUiSegmentToneMuted   PluginUiSegmentTone = "muted"
	PluginUiSegmentToneSuccess PluginUiSegmentTone = "success"
	PluginUiSegmentToneWarning PluginUiSegmentTone = "warning"
	PluginUiSegmentToneDanger  PluginUiSegmentTone = "danger"
)

func (value PluginUiSegmentTone) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginUiSegmentToneAccent, PluginUiSegmentToneMuted, PluginUiSegmentToneSuccess, PluginUiSegmentToneWarning, PluginUiSegmentToneDanger:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginUiSegmentTone %q", value)
	}
}
func (value *PluginUiSegmentTone) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginUiSegmentTone(decoded) {
	case PluginUiSegmentToneAccent, PluginUiSegmentToneMuted, PluginUiSegmentToneSuccess, PluginUiSegmentToneWarning, PluginUiSegmentToneDanger:
		*value = PluginUiSegmentTone(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginUiSegmentTone %q", decoded)
	}
}
func (value PluginUiSegmentTone) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginUiSegmentTone) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
