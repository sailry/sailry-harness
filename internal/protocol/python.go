// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type PythonPreparation struct {
	Package string      `json:"package"`
	Stage   PythonStage `json:"stage"`
}

func DefaultPythonPreparation() PythonPreparation {
	return PythonPreparation{
		Package: "",
		Stage:   PythonStage(""),
	}
}
func (value *PythonPreparation) UnmarshalJSON(data []byte) error {
	type plain PythonPreparation
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "stage"}, true); err != nil {
		return err
	}
	*value = PythonPreparation(decoded)
	return nil
}
func (value PythonPreparation) MarshalJSON() ([]byte, error) {
	type plain PythonPreparation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PythonStage string

const (
	PythonStageCreating   PythonStage = "creating"
	PythonStageInstalling PythonStage = "installing"
	PythonStageValidating PythonStage = "validating"
)

func (value PythonStage) MarshalJSON() ([]byte, error) {
	switch value {
	case PythonStageCreating, PythonStageInstalling, PythonStageValidating:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PythonStage %q", value)
	}
}
func (value *PythonStage) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PythonStage(decoded) {
	case PythonStageCreating, PythonStageInstalling, PythonStageValidating:
		*value = PythonStage(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PythonStage %q", decoded)
	}
}
func (value PythonStage) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PythonStage) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PythonInfo struct {
	Settings PythonSettings `json:"settings"`
	Version  string         `json:"version"`
}

func DefaultPythonInfo() PythonInfo {
	return PythonInfo{
		Settings: DefaultPythonSettings(),
		Version:  "",
	}
}
func (value *PythonInfo) UnmarshalJSON(data []byte) error {
	type plain PythonInfo
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"settings", "version"}, true); err != nil {
		return err
	}
	*value = PythonInfo(decoded)
	return nil
}
func (value PythonInfo) MarshalJSON() ([]byte, error) {
	type plain PythonInfo
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PythonSettings struct {
	Revision uint64  `json:"revision"`
	Index    *string `json:"index"`
}

func DefaultPythonSettings() PythonSettings {
	return PythonSettings{
		Revision: 0,
		Index:    nil,
	}
}
func (value *PythonSettings) UnmarshalJSON(data []byte) error {
	type plain PythonSettings
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, true); err != nil {
		return err
	}
	*value = PythonSettings(decoded)
	return nil
}
func (value PythonSettings) MarshalJSON() ([]byte, error) {
	type plain PythonSettings
	encoded := plain(value)
	return json.Marshal(encoded)
}
