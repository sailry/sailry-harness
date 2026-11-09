// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"github.com/google/uuid"
)

type TelemetrySettings struct {
	Revision       uint64    `json:"revision"`
	Errors         bool      `json:"errors"`
	Metrics        bool      `json:"metrics"`
	InstallationID uuid.UUID `json:"installation_id"`
}

func DefaultTelemetrySettings() TelemetrySettings {
	return TelemetrySettings{
		Revision:       0,
		Errors:         true,
		Metrics:        true,
		InstallationID: uuid.New(),
	}
}
func (value *TelemetrySettings) UnmarshalJSON(data []byte) error {
	type plain TelemetrySettings
	decoded := plain{}
	decoded.Revision = 0
	decoded.Errors = true
	decoded.Metrics = true
	decoded.InstallationID = uuid.New()
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = TelemetrySettings(decoded)
	return nil
}
func (value TelemetrySettings) MarshalJSON() ([]byte, error) {
	type plain TelemetrySettings
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TelemetryUpdate struct {
	Revision uint64 `json:"revision"`
	Errors   bool   `json:"errors"`
	Metrics  bool   `json:"metrics"`
}

func DefaultTelemetryUpdate() TelemetryUpdate {
	return TelemetryUpdate{
		Revision: 0,
		Errors:   false,
		Metrics:  false,
	}
}
func (value *TelemetryUpdate) UnmarshalJSON(data []byte) error {
	type plain TelemetryUpdate
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "errors", "metrics"}, true); err != nil {
		return err
	}
	*value = TelemetryUpdate(decoded)
	return nil
}
func (value TelemetryUpdate) MarshalJSON() ([]byte, error) {
	type plain TelemetryUpdate
	encoded := plain(value)
	return json.Marshal(encoded)
}
