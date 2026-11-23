// Hand-maintained v1 protocol extensions for compile-time native modules
// (computer use, external browser, memory management, and built-in skills).

package protocol

import "encoding/json"

type NativeSkill struct {
	Package string      `json:"package"`
	Skill   PluginSkill `json:"skill"`
}

func (value *NativeSkill) UnmarshalJSON(data []byte) error {
	type plain NativeSkill
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "skill"}, false); err != nil {
		return err
	}
	*value = NativeSkill(decoded)
	return nil
}

type ComputerSettings struct {
	Revision    uint64 `json:"revision"`
	Enabled     bool   `json:"enabled"`
	Available   bool   `json:"available,omitempty"`
	HelperError string `json:"helper_error,omitempty"`
}

func DefaultComputerSettings() ComputerSettings {
	return ComputerSettings{}
}

func (value *ComputerSettings) UnmarshalJSON(data []byte) error {
	type plain ComputerSettings
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "enabled"}, false); err != nil {
		return err
	}
	*value = ComputerSettings(decoded)
	return nil
}

type EventComputerSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func (value *EventComputerSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventComputerSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventComputerSettingsChangedPayload(decoded)
	return nil
}

type ExternalBrowserSettings struct {
	Revision  uint64 `json:"revision"`
	Enabled   bool   `json:"enabled"`
	Available bool   `json:"available,omitempty"`
}

func DefaultExternalBrowserSettings() ExternalBrowserSettings {
	return ExternalBrowserSettings{}
}

func (value *ExternalBrowserSettings) UnmarshalJSON(data []byte) error {
	type plain ExternalBrowserSettings
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "enabled"}, false); err != nil {
		return err
	}
	*value = ExternalBrowserSettings(decoded)
	return nil
}

type EventExternalBrowserSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func (value *EventExternalBrowserSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventExternalBrowserSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventExternalBrowserSettingsChangedPayload(decoded)
	return nil
}

type MemorySettings struct {
	Revision        uint64 `json:"revision"`
	Enabled         bool   `json:"enabled"`
	AutoWrite       bool   `json:"auto_write"`
	ContextBytes    uint32 `json:"context_bytes"`
	ReviewAfterDays uint32 `json:"review_after_days"`
}

func DefaultMemorySettings() MemorySettings {
	return MemorySettings{
		Enabled:         true,
		AutoWrite:       true,
		ContextBytes:    8192,
		ReviewAfterDays: 90,
	}
}

func (value *MemorySettings) UnmarshalJSON(data []byte) error {
	type plain MemorySettings
	decoded := plain(DefaultMemorySettings())
	if err := DecodeObject(data, &decoded, []string{"enabled", "auto_write"}, false); err != nil {
		return err
	}
	*value = MemorySettings(decoded)
	return nil
}

type MemorySummary struct {
	ID          string  `json:"id"`
	Project     *string `json:"project,omitempty"`
	Title       string  `json:"title"`
	Kind        string  `json:"kind"`
	Archived    bool    `json:"archived"`
	Revision    uint64  `json:"revision"`
	UpdatedAtMs int64   `json:"updated_at_ms"`
	DuplicateOf *string `json:"duplicate_of,omitempty"`
	NeedsReview bool    `json:"needs_review,omitempty"`
}

type MemoryEntry struct {
	Summary MemorySummary `json:"summary"`
	Body    string        `json:"body"`
}

type MemoryCatalog struct {
	Settings MemorySettings  `json:"settings"`
	Entries  []MemorySummary `json:"entries"`
}

func (value MemoryCatalog) MarshalJSON() ([]byte, error) {
	type plain MemoryCatalog
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type MemoryMergeSource struct {
	ID       string `json:"id"`
	Revision uint64 `json:"revision"`
}

type CommandReadMemoryCatalogPayload struct {
	View       string  `json:"view"`
	Query      string  `json:"query,omitempty"`
	Project    *string `json:"project,omitempty"`
	GlobalOnly bool    `json:"global_only,omitempty"`
}

type CommandReadMemoryEntryPayload struct {
	ID string `json:"id"`
}

type CommandSaveMemoryEntryPayload struct {
	ID               *string             `json:"id,omitempty"`
	ExpectedRevision uint64              `json:"expected_revision"`
	Project          *string             `json:"project,omitempty"`
	Title            string              `json:"title"`
	Kind             string              `json:"kind"`
	Body             string              `json:"body"`
	Archived         bool                `json:"archived"`
	MergeSources     []MemoryMergeSource `json:"merge_sources,omitempty"`
}

type CommandDeleteMemoryEntryPayload struct {
	ID               string `json:"id"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

type CommandSaveMemorySettingsPayload struct {
	ExpectedRevision uint64 `json:"expected_revision"`
	Enabled          bool   `json:"enabled"`
	AutoWrite        bool   `json:"auto_write"`
	ContextBytes     uint32 `json:"context_bytes"`
	ReviewAfterDays  uint32 `json:"review_after_days"`
}

type OutputMemoryDeletedPayload struct {
	ID string `json:"id"`
}

type EventMemoryChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func (value *EventMemoryChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventMemoryChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventMemoryChangedPayload(decoded)
	return nil
}
