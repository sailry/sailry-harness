// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type TerminalSettings struct {
	Revision       uint64            `json:"revision"`
	Shell          string            `json:"shell"`
	ShellArguments []string          `json:"shell_arguments,omitempty"`
	Environment    map[string]string `json:"environment"`
}

func DefaultTerminalSettings() TerminalSettings {
	return TerminalSettings{
		Revision:       0,
		Shell:          "",
		ShellArguments: []string{},
		Environment:    map[string]string{},
	}
}
func (value *TerminalSettings) UnmarshalJSON(data []byte) error {
	type plain TerminalSettings
	decoded := plain{}
	decoded.ShellArguments = []string{}
	if err := DecodeObject(data, &decoded, []string{"revision", "shell", "environment"}, false); err != nil {
		return err
	}
	*value = TerminalSettings(decoded)
	return nil
}
func (value TerminalSettings) MarshalJSON() ([]byte, error) {
	type plain TerminalSettings
	encoded := plain(value)
	encoded.ShellArguments = normalizeCollections(encoded.ShellArguments)
	encoded.Environment = normalizeCollections(encoded.Environment)
	return json.Marshal(encoded)
}
