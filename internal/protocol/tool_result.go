// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ToolResultResultDisplay struct {
	Version     uint32                 `json:"version"`
	Diagnostics []ToolResultDiagnostic `json:"diagnostics"`
	Status      *ToolResultLocalText   `json:"status"`
	Expected    *ToolResultExpected    `json:"expected,omitempty"`
}

func DefaultToolResultResultDisplay() ToolResultResultDisplay {
	return ToolResultResultDisplay{
		Version:     0,
		Diagnostics: []ToolResultDiagnostic{},
		Status:      nil,
		Expected:    nil,
	}
}
func (value *ToolResultResultDisplay) UnmarshalJSON(data []byte) error {
	type plain ToolResultResultDisplay
	decoded := plain{}
	decoded.Expected = nil
	if err := DecodeObject(data, &decoded, []string{"version", "diagnostics"}, true); err != nil {
		return err
	}
	*value = ToolResultResultDisplay(decoded)
	return nil
}
func (value ToolResultResultDisplay) MarshalJSON() ([]byte, error) {
	type plain ToolResultResultDisplay
	encoded := plain(value)
	encoded.Diagnostics = normalizeCollections(encoded.Diagnostics)
	return json.Marshal(encoded)
}

type ToolResultExpected string

const (
	ToolResultExpectedDirectoryNotFound ToolResultExpected = "directory_not_found"
	ToolResultExpectedFileNotFound      ToolResultExpected = "file_not_found"
)

func (value ToolResultExpected) MarshalJSON() ([]byte, error)     { return marshalEnum(value) }
func (value *ToolResultExpected) UnmarshalJSON(data []byte) error { return DecodeValue(data, value) }
func (value *ToolResultExpected) DecodeJSON(decoder *Decoder) error {
	decoded, err := decoder.String()
	if err != nil {
		return err
	}
	switch ToolResultExpected(decoded) {
	case ToolResultExpectedDirectoryNotFound, ToolResultExpectedFileNotFound:
		*value = ToolResultExpected(decoded)
		return nil
	default:
		return settleEnum(decoder.tolerance, value, decoded)
	}
}
func (value ToolResultExpected) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolResultExpected) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ToolResultDiagnostic struct {
	Text  string `json:"text"`
	Error bool   `json:"error"`
}

func DefaultToolResultDiagnostic() ToolResultDiagnostic {
	return ToolResultDiagnostic{
		Text:  "",
		Error: false,
	}
}
func (value *ToolResultDiagnostic) UnmarshalJSON(data []byte) error {
	type plain ToolResultDiagnostic
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text", "error"}, true); err != nil {
		return err
	}
	*value = ToolResultDiagnostic(decoded)
	return nil
}
func (value ToolResultDiagnostic) MarshalJSON() ([]byte, error) {
	type plain ToolResultDiagnostic
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolResultLocalText struct {
	Label   string            `json:"label"`
	Locales map[string]string `json:"locales"`
}

func DefaultToolResultLocalText() ToolResultLocalText {
	return ToolResultLocalText{
		Label:   "",
		Locales: map[string]string{},
	}
}
func (value *ToolResultLocalText) UnmarshalJSON(data []byte) error {
	type plain ToolResultLocalText
	decoded := plain{}
	decoded.Locales = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"label"}, true); err != nil {
		return err
	}
	*value = ToolResultLocalText(decoded)
	return nil
}
func (value ToolResultLocalText) MarshalJSON() ([]byte, error) {
	type plain ToolResultLocalText
	encoded := plain(value)
	encoded.Locales = normalizeCollections(encoded.Locales)
	return json.Marshal(encoded)
}
