// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type TerminalShell struct {
	Path      string   `json:"path"`
	Arguments []string `json:"arguments"`
}

func DefaultTerminalShell() TerminalShell {
	return TerminalShell{
		Path:      "",
		Arguments: []string{},
	}
}
func (value *TerminalShell) UnmarshalJSON(data []byte) error {
	type plain TerminalShell
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "arguments"}, false); err != nil {
		return err
	}
	*value = TerminalShell(decoded)
	return nil
}
func (value TerminalShell) MarshalJSON() ([]byte, error) {
	type plain TerminalShell
	encoded := plain(value)
	encoded.Arguments = normalizeCollections(encoded.Arguments)
	return json.Marshal(encoded)
}

type TerminalViewport struct {
	Columns     uint16 `json:"columns"`
	Rows        uint16 `json:"rows"`
	PixelWidth  uint32 `json:"pixel_width"`
	PixelHeight uint32 `json:"pixel_height"`
}

func DefaultTerminalViewport() TerminalViewport {
	return TerminalViewport{
		Columns:     0,
		Rows:        0,
		PixelWidth:  0,
		PixelHeight: 0,
	}
}
func (value *TerminalViewport) UnmarshalJSON(data []byte) error {
	type plain TerminalViewport
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"columns", "rows", "pixel_width", "pixel_height"}, false); err != nil {
		return err
	}
	*value = TerminalViewport(decoded)
	return nil
}
func (value TerminalViewport) MarshalJSON() ([]byte, error) {
	type plain TerminalViewport
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalRgb struct {
	Red   uint8 `json:"red"`
	Green uint8 `json:"green"`
	Blue  uint8 `json:"blue"`
}

func DefaultTerminalRgb() TerminalRgb {
	return TerminalRgb{
		Red:   0,
		Green: 0,
		Blue:  0,
	}
}
func (value *TerminalRgb) UnmarshalJSON(data []byte) error {
	type plain TerminalRgb
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"red", "green", "blue"}, false); err != nil {
		return err
	}
	*value = TerminalRgb(decoded)
	return nil
}
func (value TerminalRgb) MarshalJSON() ([]byte, error) {
	type plain TerminalRgb
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalColorScheme string

const (
	TerminalColorSchemeLight TerminalColorScheme = "light"
	TerminalColorSchemeDark  TerminalColorScheme = "dark"
)

func (value TerminalColorScheme) MarshalJSON() ([]byte, error) {
	switch value {
	case TerminalColorSchemeLight, TerminalColorSchemeDark:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown TerminalColorScheme %q", value)
	}
}
func (value *TerminalColorScheme) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch TerminalColorScheme(decoded) {
	case TerminalColorSchemeLight, TerminalColorSchemeDark:
		*value = TerminalColorScheme(decoded)
		return nil
	default:
		return fmt.Errorf("unknown TerminalColorScheme %q", decoded)
	}
}
func (value TerminalColorScheme) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *TerminalColorScheme) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type TerminalAppearance struct {
	Foreground  TerminalRgb         `json:"foreground"`
	Background  TerminalRgb         `json:"background"`
	Palette     [16]TerminalRgb     `json:"palette"`
	ColorScheme TerminalColorScheme `json:"color_scheme"`
}

func DefaultTerminalAppearance() TerminalAppearance {
	return TerminalAppearance{
		Foreground:  DefaultTerminalRgb(),
		Background:  DefaultTerminalRgb(),
		Palette:     [16]TerminalRgb{},
		ColorScheme: TerminalColorScheme(""),
	}
}
func (value *TerminalAppearance) UnmarshalJSON(data []byte) error {
	type plain TerminalAppearance
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"foreground", "background", "palette", "color_scheme"}, false); err != nil {
		return err
	}
	*value = TerminalAppearance(decoded)
	return nil
}
func (value TerminalAppearance) MarshalJSON() ([]byte, error) {
	type plain TerminalAppearance
	encoded := plain(value)
	encoded.Palette = normalizeCollections(encoded.Palette)
	return json.Marshal(encoded)
}

type TerminalLaunch struct {
	Worktree   WorktreeID         `json:"worktree"`
	Access     ProcessAccess      `json:"access"`
	Viewport   TerminalViewport   `json:"viewport"`
	Appearance TerminalAppearance `json:"appearance"`
}

func DefaultTerminalLaunch() TerminalLaunch {
	return TerminalLaunch{
		Worktree:   WorktreeID{},
		Access:     ProcessAccessWorkspace,
		Viewport:   DefaultTerminalViewport(),
		Appearance: DefaultTerminalAppearance(),
	}
}
func (value *TerminalLaunch) UnmarshalJSON(data []byte) error {
	type plain TerminalLaunch
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "access", "viewport", "appearance"}, false); err != nil {
		return err
	}
	*value = TerminalLaunch(decoded)
	return nil
}
func (value TerminalLaunch) MarshalJSON() ([]byte, error) {
	type plain TerminalLaunch
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalStatus struct {
	Kind string
	Data any
}
type TerminalStatusExitedPayload struct {
	Code uint32 `json:"code"`
}

func DefaultTerminalStatusExitedPayload() TerminalStatusExitedPayload {
	return TerminalStatusExitedPayload{
		Code: 0,
	}
}
func (value *TerminalStatusExitedPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalStatusExitedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"code"}, false); err != nil {
		return err
	}
	*value = TerminalStatusExitedPayload(decoded)
	return nil
}
func (value TerminalStatusExitedPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalStatusExitedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalStatusFailedPayload struct {
	Message string `json:"message"`
}

func DefaultTerminalStatusFailedPayload() TerminalStatusFailedPayload {
	return TerminalStatusFailedPayload{
		Message: "",
	}
}
func (value *TerminalStatusFailedPayload) UnmarshalJSON(data []byte) error {
	type plain TerminalStatusFailedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"message"}, false); err != nil {
		return err
	}
	*value = TerminalStatusFailedPayload(decoded)
	return nil
}
func (value TerminalStatusFailedPayload) MarshalJSON() ([]byte, error) {
	type plain TerminalStatusFailedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var terminalStatusVariants = variantFactories{"running": nil,
	"exited":  func() any { return new(TerminalStatusExitedPayload) },
	"closed":  nil,
	"stopped": nil,
	"failed":  func() any { return new(TerminalStatusFailedPayload) }}

func (value TerminalStatus) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, terminalStatusVariants)
}
func (value *TerminalStatus) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", terminalStatusVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type TerminalActivity string

const (
	TerminalActivityIdle            TerminalActivity = "idle"
	TerminalActivityWorking         TerminalActivity = "working"
	TerminalActivityWaitingForInput TerminalActivity = "waiting_for_input"
	TerminalActivityCompleted       TerminalActivity = "completed"
	TerminalActivityFailed          TerminalActivity = "failed"
)

func (value TerminalActivity) MarshalJSON() ([]byte, error) {
	switch value {
	case TerminalActivityIdle, TerminalActivityWorking, TerminalActivityWaitingForInput, TerminalActivityCompleted, TerminalActivityFailed:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown TerminalActivity %q", value)
	}
}
func (value *TerminalActivity) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch TerminalActivity(decoded) {
	case TerminalActivityIdle, TerminalActivityWorking, TerminalActivityWaitingForInput, TerminalActivityCompleted, TerminalActivityFailed:
		*value = TerminalActivity(decoded)
		return nil
	default:
		return fmt.Errorf("unknown TerminalActivity %q", decoded)
	}
}
func (value TerminalActivity) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *TerminalActivity) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type TerminalActivityReport struct {
	State    TerminalActivity `json:"state"`
	Sequence uint64           `json:"sequence"`
}

func DefaultTerminalActivityReport() TerminalActivityReport {
	return TerminalActivityReport{
		State:    TerminalActivityIdle,
		Sequence: 0,
	}
}
func (value *TerminalActivityReport) UnmarshalJSON(data []byte) error {
	type plain TerminalActivityReport
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"state", "sequence"}, false); err != nil {
		return err
	}
	*value = TerminalActivityReport(decoded)
	return nil
}
func (value TerminalActivityReport) MarshalJSON() ([]byte, error) {
	type plain TerminalActivityReport
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalInfo struct {
	ID        TerminalID              `json:"id"`
	Worktree  *WorktreeID             `json:"worktree"`
	Ssh       *SshID                  `json:"ssh"`
	Tool      *TerminalToolsTool      `json:"tool"`
	Access    *ProcessAccess          `json:"access"`
	Status    TerminalStatus          `json:"status"`
	Activity  *TerminalActivityReport `json:"activity"`
	Title     *string                 `json:"title"`
	Directory *string                 `json:"directory"`
	Owner     *NodeID                 `json:"owner"`
	Revision  uint64                  `json:"revision"`
}

func DefaultTerminalInfo() TerminalInfo {
	return TerminalInfo{
		ID:        TerminalID{},
		Worktree:  nil,
		Ssh:       nil,
		Tool:      nil,
		Access:    nil,
		Status:    TerminalStatus{},
		Activity:  nil,
		Title:     nil,
		Directory: nil,
		Owner:     nil,
		Revision:  0,
	}
}
func (value *TerminalInfo) UnmarshalJSON(data []byte) error {
	type plain TerminalInfo
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "status", "revision"}, false); err != nil {
		return err
	}
	*value = TerminalInfo(decoded)
	return nil
}
func (value TerminalInfo) MarshalJSON() ([]byte, error) {
	type plain TerminalInfo
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalSnapshot struct {
	Node     NodeID         `json:"node"`
	Info     TerminalInfo   `json:"info"`
	Sequence uint64         `json:"sequence"`
	Screen   TerminalScreen `json:"screen"`
}

func DefaultTerminalSnapshot() TerminalSnapshot {
	return TerminalSnapshot{
		Node:     NodeID{},
		Info:     DefaultTerminalInfo(),
		Sequence: 0,
		Screen:   DefaultTerminalScreen(),
	}
}
func (value *TerminalSnapshot) UnmarshalJSON(data []byte) error {
	type plain TerminalSnapshot
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "info", "sequence", "screen"}, false); err != nil {
		return err
	}
	*value = TerminalSnapshot(decoded)
	return nil
}
func (value TerminalSnapshot) MarshalJSON() ([]byte, error) {
	type plain TerminalSnapshot
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalFrame struct {
	Node     NodeID                     `json:"node"`
	Info     TerminalInfo               `json:"info"`
	Sequence uint64                     `json:"sequence"`
	Screen   TerminalScreenScreenUpdate `json:"screen"`
}

func DefaultTerminalFrame() TerminalFrame {
	return TerminalFrame{
		Node:     NodeID{},
		Info:     DefaultTerminalInfo(),
		Sequence: 0,
		Screen:   TerminalScreenScreenUpdate{},
	}
}
func (value *TerminalFrame) UnmarshalJSON(data []byte) error {
	type plain TerminalFrame
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "info", "sequence", "screen"}, false); err != nil {
		return err
	}
	*value = TerminalFrame(decoded)
	return nil
}
func (value TerminalFrame) MarshalJSON() ([]byte, error) {
	type plain TerminalFrame
	encoded := plain(value)
	return json.Marshal(encoded)
}
