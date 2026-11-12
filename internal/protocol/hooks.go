// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type HooksScript struct {
	Name      string `json:"name"`
	Enabled   bool   `json:"enabled"`
	Command   string `json:"command"`
	TimeoutMs uint64 `json:"timeout_ms"`
}

func DefaultHooksScript() HooksScript {
	return HooksScript{
		Name:      "",
		Enabled:   true,
		Command:   "",
		TimeoutMs: 60000,
	}
}
func (value *HooksScript) UnmarshalJSON(data []byte) error {
	type plain HooksScript
	decoded := plain{}
	decoded.Name = ""
	decoded.Enabled = true
	if err := DecodeObject(data, &decoded, []string{"command", "timeout_ms"}, true); err != nil {
		return err
	}
	*value = HooksScript(decoded)
	return nil
}
func (value HooksScript) MarshalJSON() ([]byte, error) {
	type plain HooksScript
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HooksEntry struct {
	ID     string      `json:"id"`
	Phase  HooksPhase  `json:"phase"`
	Script HooksScript `json:"script"`
}

func DefaultHooksEntry() HooksEntry {
	return HooksEntry{
		ID:     "",
		Phase:  HooksPhase(""),
		Script: DefaultHooksScript(),
	}
}
func (value *HooksEntry) UnmarshalJSON(data []byte) error {
	type plain HooksEntry
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "phase", "script"}, true); err != nil {
		return err
	}
	*value = HooksEntry(decoded)
	return nil
}
func (value HooksEntry) MarshalJSON() ([]byte, error) {
	type plain HooksEntry
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HooksSettings struct {
	Revision   uint64       `json:"revision"`
	Enabled    bool         `json:"enabled"`
	BeforeTool *HooksScript `json:"before_tool"`
	AfterTool  *HooksScript `json:"after_tool"`
	TurnEnd    *HooksScript `json:"turn_end"`
	Additional []HooksEntry `json:"additional"`
}

func DefaultHooksSettings() HooksSettings {
	return HooksSettings{
		Revision:   0,
		Enabled:    false,
		BeforeTool: nil,
		AfterTool:  nil,
		TurnEnd:    nil,
		Additional: []HooksEntry{},
	}
}
func (value *HooksSettings) UnmarshalJSON(data []byte) error {
	type plain HooksSettings
	decoded := plain{}
	decoded.Additional = []HooksEntry{}
	if err := DecodeObject(data, &decoded, []string{"revision", "enabled"}, true); err != nil {
		return err
	}
	*value = HooksSettings(decoded)
	return nil
}
func (value HooksSettings) MarshalJSON() ([]byte, error) {
	type plain HooksSettings
	encoded := plain(value)
	encoded.Additional = normalizeCollections(encoded.Additional)
	return json.Marshal(encoded)
}

type HooksState struct {
	Worktree  WorktreeID    `json:"worktree"`
	Settings  HooksSettings `json:"settings"`
	Sandboxed bool          `json:"sandboxed"`
}

func DefaultHooksState() HooksState {
	return HooksState{
		Worktree:  WorktreeID{},
		Settings:  DefaultHooksSettings(),
		Sandboxed: false,
	}
}
func (value *HooksState) UnmarshalJSON(data []byte) error {
	type plain HooksState
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "settings", "sandboxed"}, true); err != nil {
		return err
	}
	*value = HooksState(decoded)
	return nil
}
func (value HooksState) MarshalJSON() ([]byte, error) {
	type plain HooksState
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HooksPhase string

const (
	HooksPhaseBeforeTool HooksPhase = "before_tool"
	HooksPhaseAfterTool  HooksPhase = "after_tool"
	HooksPhaseTurnEnd    HooksPhase = "turn_end"
)

func (value HooksPhase) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *HooksPhase) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch HooksPhase(decoded) {
	case HooksPhaseBeforeTool, HooksPhaseAfterTool, HooksPhaseTurnEnd:
		*value = HooksPhase(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value HooksPhase) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *HooksPhase) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
