// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type PluginUpdatesRequest struct {
	Name             string `json:"name"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultPluginUpdatesRequest() PluginUpdatesRequest {
	return PluginUpdatesRequest{
		Name:             "",
		ExpectedRevision: 0,
	}
}
func (value *PluginUpdatesRequest) UnmarshalJSON(data []byte) error {
	type plain PluginUpdatesRequest
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "expected_revision"}, false); err != nil {
		return err
	}
	*value = PluginUpdatesRequest(decoded)
	return nil
}
func (value PluginUpdatesRequest) MarshalJSON() ([]byte, error) {
	type plain PluginUpdatesRequest
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUpdatesCandidate struct {
	Kind string
	Data any
}

var pluginUpdatesCandidateVariants = variantFactories{"package": func() any { return new(PluginInfo) },
	"official": func() any { return new(PluginCatalogEntry) }}

func (value PluginUpdatesCandidate) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "package", value.Kind, value.Data, pluginUpdatesCandidateVariants)
}
func (value *PluginUpdatesCandidate) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "package", pluginUpdatesCandidateVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginUpdatesReport struct {
	Blocked   *PluginUpdatesBlocked   `json:"blocked,omitempty"`
	Current   PluginSummary           `json:"current"`
	Available *PluginUpdatesCandidate `json:"available"`
	Manual    bool                    `json:"manual"`
}

func DefaultPluginUpdatesReport() PluginUpdatesReport {
	return PluginUpdatesReport{
		Blocked:   nil,
		Current:   DefaultPluginSummary(),
		Available: nil,
		Manual:    false,
	}
}
func (value *PluginUpdatesReport) UnmarshalJSON(data []byte) error {
	type plain PluginUpdatesReport
	decoded := plain{}
	decoded.Blocked = nil
	if err := DecodeObject(data, &decoded, []string{"current", "manual"}, false); err != nil {
		return err
	}
	*value = PluginUpdatesReport(decoded)
	return nil
}
func (value PluginUpdatesReport) MarshalJSON() ([]byte, error) {
	type plain PluginUpdatesReport
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUpdatesBlocked struct {
	Version      string                     `json:"version"`
	Required     string                     `json:"required"`
	ReleaseNotes *PluginCatalogReleaseNotes `json:"release_notes"`
}

func DefaultPluginUpdatesBlocked() PluginUpdatesBlocked {
	return PluginUpdatesBlocked{
		Version:      "",
		Required:     "",
		ReleaseNotes: nil,
	}
}
func (value *PluginUpdatesBlocked) UnmarshalJSON(data []byte) error {
	type plain PluginUpdatesBlocked
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"version", "required"}, false); err != nil {
		return err
	}
	*value = PluginUpdatesBlocked(decoded)
	return nil
}
func (value PluginUpdatesBlocked) MarshalJSON() ([]byte, error) {
	type plain PluginUpdatesBlocked
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUpdatesBatch struct {
	Reports  []PluginUpdatesReport  `json:"reports"`
	Failures []PluginUpdatesFailure `json:"failures"`
}

func DefaultPluginUpdatesBatch() PluginUpdatesBatch {
	return PluginUpdatesBatch{
		Reports:  []PluginUpdatesReport{},
		Failures: []PluginUpdatesFailure{},
	}
}
func (value *PluginUpdatesBatch) UnmarshalJSON(data []byte) error {
	type plain PluginUpdatesBatch
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"reports", "failures"}, false); err != nil {
		return err
	}
	*value = PluginUpdatesBatch(decoded)
	return nil
}
func (value PluginUpdatesBatch) MarshalJSON() ([]byte, error) {
	type plain PluginUpdatesBatch
	encoded := plain(value)
	encoded.Reports = normalizeCollections(encoded.Reports)
	encoded.Failures = normalizeCollections(encoded.Failures)
	return json.Marshal(encoded)
}

type PluginUpdatesFailure struct {
	Name  string `json:"name"`
	Error Fault  `json:"error"`
}

func DefaultPluginUpdatesFailure() PluginUpdatesFailure {
	return PluginUpdatesFailure{
		Name:  "",
		Error: DefaultFault(),
	}
}
func (value *PluginUpdatesFailure) UnmarshalJSON(data []byte) error {
	type plain PluginUpdatesFailure
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "error"}, false); err != nil {
		return err
	}
	*value = PluginUpdatesFailure(decoded)
	return nil
}
func (value PluginUpdatesFailure) MarshalJSON() ([]byte, error) {
	type plain PluginUpdatesFailure
	encoded := plain(value)
	return json.Marshal(encoded)
}
