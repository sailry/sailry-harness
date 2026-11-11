// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type TerminalToolsTool string

const (
	TerminalToolsToolCodex    TerminalToolsTool = "codex"
	TerminalToolsToolClaude   TerminalToolsTool = "claude"
	TerminalToolsToolGemini   TerminalToolsTool = "gemini"
	TerminalToolsToolAgy      TerminalToolsTool = "agy"
	TerminalToolsToolGrok     TerminalToolsTool = "grok"
	TerminalToolsToolOpencode TerminalToolsTool = "opencode"
	TerminalToolsToolKimi     TerminalToolsTool = "kimi"
)

func (value TerminalToolsTool) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *TerminalToolsTool) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch TerminalToolsTool(decoded) {
	case TerminalToolsToolCodex, TerminalToolsToolClaude, TerminalToolsToolGemini, TerminalToolsToolAgy, TerminalToolsToolGrok, TerminalToolsToolOpencode, TerminalToolsToolKimi:
		*value = TerminalToolsTool(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value TerminalToolsTool) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *TerminalToolsTool) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type TerminalToolsToolInfo struct {
	Tool      TerminalToolsTool `json:"tool"`
	Available bool              `json:"available"`
}

func DefaultTerminalToolsToolInfo() TerminalToolsToolInfo {
	return TerminalToolsToolInfo{
		Tool:      TerminalToolsTool(""),
		Available: false,
	}
}
func (value *TerminalToolsToolInfo) UnmarshalJSON(data []byte) error {
	type plain TerminalToolsToolInfo
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tool", "available"}, false); err != nil {
		return err
	}
	*value = TerminalToolsToolInfo(decoded)
	return nil
}
func (value TerminalToolsToolInfo) MarshalJSON() ([]byte, error) {
	type plain TerminalToolsToolInfo
	encoded := plain(value)
	return json.Marshal(encoded)
}
