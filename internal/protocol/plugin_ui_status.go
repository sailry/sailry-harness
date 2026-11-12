// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type PluginUiStatusScope struct {
	Worktree *WorktreeID `json:"worktree"`
	Session  *SessionID  `json:"session"`
}

func DefaultPluginUiStatusScope() PluginUiStatusScope {
	return PluginUiStatusScope{
		Worktree: nil,
		Session:  nil,
	}
}
func (value *PluginUiStatusScope) UnmarshalJSON(data []byte) error {
	type plain PluginUiStatusScope
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginUiStatusScope(decoded)
	return nil
}
func (value PluginUiStatusScope) MarshalJSON() ([]byte, error) {
	type plain PluginUiStatusScope
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUiStatusEntry struct {
	Package     PluginReference      `json:"package"`
	Declaration PluginUiContribution `json:"declaration"`
	State       PluginUiState        `json:"state"`
	Invoke      *string              `json:"invoke,omitempty"`
}

func DefaultPluginUiStatusEntry() PluginUiStatusEntry {
	return PluginUiStatusEntry{
		Package:     DefaultPluginReference(),
		Declaration: DefaultPluginUiContribution(),
		State:       DefaultPluginUiState(),
		Invoke:      nil,
	}
}
func (value *PluginUiStatusEntry) UnmarshalJSON(data []byte) error {
	type plain PluginUiStatusEntry
	decoded := plain{}
	decoded.Invoke = nil
	if err := DecodeObject(data, &decoded, []string{"package", "declaration", "state"}, false); err != nil {
		return err
	}
	*value = PluginUiStatusEntry(decoded)
	return nil
}
func (value PluginUiStatusEntry) MarshalJSON() ([]byte, error) {
	type plain PluginUiStatusEntry
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUiStatusFailure struct {
	Package PluginReference `json:"package"`
	Fault   Fault           `json:"fault"`
}

func DefaultPluginUiStatusFailure() PluginUiStatusFailure {
	return PluginUiStatusFailure{
		Package: DefaultPluginReference(),
		Fault:   DefaultFault(),
	}
}
func (value *PluginUiStatusFailure) UnmarshalJSON(data []byte) error {
	type plain PluginUiStatusFailure
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "fault"}, false); err != nil {
		return err
	}
	*value = PluginUiStatusFailure(decoded)
	return nil
}
func (value PluginUiStatusFailure) MarshalJSON() ([]byte, error) {
	type plain PluginUiStatusFailure
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUiStatusSnapshot struct {
	Node     NodeID                  `json:"node"`
	Scope    PluginUiStatusScope     `json:"scope"`
	Entries  []PluginUiStatusEntry   `json:"entries"`
	Failures []PluginUiStatusFailure `json:"failures"`
}

func DefaultPluginUiStatusSnapshot() PluginUiStatusSnapshot {
	return PluginUiStatusSnapshot{
		Node:     NodeID{},
		Scope:    DefaultPluginUiStatusScope(),
		Entries:  []PluginUiStatusEntry{},
		Failures: []PluginUiStatusFailure{},
	}
}
func (value *PluginUiStatusSnapshot) UnmarshalJSON(data []byte) error {
	type plain PluginUiStatusSnapshot
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "scope", "entries", "failures"}, false); err != nil {
		return err
	}
	*value = PluginUiStatusSnapshot(decoded)
	return nil
}
func (value PluginUiStatusSnapshot) MarshalJSON() ([]byte, error) {
	type plain PluginUiStatusSnapshot
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	encoded.Failures = normalizeCollections(encoded.Failures)
	return json.Marshal(encoded)
}
