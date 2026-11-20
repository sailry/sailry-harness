// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import "encoding/json"

// ConfigurationDocument is an exported configuration. The document is the
// Node's own versioned format; clients store and return it unchanged.
type ConfigurationDocument struct {
	Document json.RawMessage `json:"document"`
}

func DefaultConfigurationDocument() ConfigurationDocument {
	return ConfigurationDocument{Document: json.RawMessage("null")}
}
func (value *ConfigurationDocument) UnmarshalJSON(data []byte) error {
	type plain ConfigurationDocument
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"document"}, false); err != nil {
		return err
	}
	*value = ConfigurationDocument(decoded)
	return nil
}
func (value ConfigurationDocument) MarshalJSON() ([]byte, error) {
	type plain ConfigurationDocument
	return json.Marshal(plain(value))
}

// CommandImportConfigurationPayload merges a document into this Node's
// configuration. A preview applies it and rolls it back, reporting the same
// outcome without changing anything.
type CommandImportConfigurationPayload struct {
	Document json.RawMessage `json:"document"`
	Preview  bool            `json:"preview"`
}

func DefaultCommandImportConfigurationPayload() CommandImportConfigurationPayload {
	return CommandImportConfigurationPayload{Document: json.RawMessage("null"), Preview: false}
}
func (value *CommandImportConfigurationPayload) UnmarshalJSON(data []byte) error {
	type plain CommandImportConfigurationPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"document", "preview"}, false); err != nil {
		return err
	}
	*value = CommandImportConfigurationPayload(decoded)
	return nil
}
func (value CommandImportConfigurationPayload) MarshalJSON() ([]byte, error) {
	type plain CommandImportConfigurationPayload
	return json.Marshal(plain(value))
}

type ConfigurationImportAction string

const (
	ConfigurationImportAdded   ConfigurationImportAction = "added"
	ConfigurationImportUpdated ConfigurationImportAction = "updated"
	ConfigurationImportSkipped ConfigurationImportAction = "skipped"
)

func (value ConfigurationImportAction) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConfigurationImportAction) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConfigurationImportAction(decoded) {
	case ConfigurationImportAdded, ConfigurationImportUpdated, ConfigurationImportSkipped:
		*value = ConfigurationImportAction(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}

// ConfigurationImportItem is one imported entry. Area names the settings
// group; Name is the entry within it, empty for a single setting. Reason
// says why a skipped entry was left out and what restores it.
type ConfigurationImportItem struct {
	Area   string                    `json:"area"`
	Name   string                    `json:"name"`
	Action ConfigurationImportAction `json:"action"`
	Reason *string                   `json:"reason,omitempty"`
}

func DefaultConfigurationImportItem() ConfigurationImportItem {
	return ConfigurationImportItem{}
}
func (value *ConfigurationImportItem) UnmarshalJSON(data []byte) error {
	type plain ConfigurationImportItem
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"area", "name", "action"}, false); err != nil {
		return err
	}
	*value = ConfigurationImportItem(decoded)
	return nil
}
func (value ConfigurationImportItem) MarshalJSON() ([]byte, error) {
	type plain ConfigurationImportItem
	return json.Marshal(plain(value))
}

type ConfigurationImportReport struct {
	Preview bool                      `json:"preview"`
	Items   []ConfigurationImportItem `json:"items"`
}

func DefaultConfigurationImportReport() ConfigurationImportReport {
	return ConfigurationImportReport{Items: []ConfigurationImportItem{}}
}
func (value *ConfigurationImportReport) UnmarshalJSON(data []byte) error {
	type plain ConfigurationImportReport
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"preview", "items"}, false); err != nil {
		return err
	}
	*value = ConfigurationImportReport(decoded)
	return nil
}
func (value ConfigurationImportReport) MarshalJSON() ([]byte, error) {
	type plain ConfigurationImportReport
	encoded := plain(value)
	encoded.Items = normalizeCollections(encoded.Items)
	return json.Marshal(encoded)
}
