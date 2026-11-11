// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PluginCardReceipt struct {
	Session SessionID `json:"session"`
	Turn    TurnID    `json:"turn"`
	Call    RequestID `json:"call"`
	Block   uint8     `json:"block"`
}

func DefaultPluginCardReceipt() PluginCardReceipt {
	return PluginCardReceipt{
		Session: SessionID{},
		Turn:    TurnID{},
		Call:    RequestID{},
		Block:   0,
	}
}
func (value *PluginCardReceipt) UnmarshalJSON(data []byte) error {
	type plain PluginCardReceipt
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "call", "block"}, true); err != nil {
		return err
	}
	*value = PluginCardReceipt(decoded)
	return nil
}
func (value PluginCardReceipt) MarshalJSON() ([]byte, error) {
	type plain PluginCardReceipt
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginCardDeclaration struct {
	Key          string                 `json:"key"`
	Entry        string                 `json:"entry"`
	PropsSchema  json.RawMessage        `json:"props_schema"`
	Tool         *string                `json:"tool,omitempty"`
	McpServer    *string                `json:"mcp_server,omitempty"`
	Capabilities PluginCardCapabilities `json:"capabilities"`
}

func DefaultPluginCardDeclaration() PluginCardDeclaration {
	return PluginCardDeclaration{
		Key:          "",
		Entry:        "",
		PropsSchema:  json.RawMessage("null"),
		Tool:         nil,
		McpServer:    nil,
		Capabilities: DefaultPluginCardCapabilities(),
	}
}
func (value *PluginCardDeclaration) UnmarshalJSON(data []byte) error {
	type plain PluginCardDeclaration
	decoded := plain{}
	decoded.Tool = nil
	decoded.McpServer = nil
	decoded.Capabilities = DefaultPluginCardCapabilities()
	if err := DecodeObject(data, &decoded, []string{"key", "entry", "props_schema"}, true); err != nil {
		return err
	}
	*value = PluginCardDeclaration(decoded)
	return nil
}
func (value PluginCardDeclaration) MarshalJSON() ([]byte, error) {
	type plain PluginCardDeclaration
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginCardCapabilities struct {
	Qr       *PluginCardQr      `json:"qr,omitempty"`
	Refresh  bool               `json:"refresh"`
	Settings bool               `json:"settings"`
	Browser  []PluginCardTarget `json:"browser"`
}

func DefaultPluginCardCapabilities() PluginCardCapabilities {
	return PluginCardCapabilities{
		Qr:       nil,
		Refresh:  false,
		Settings: false,
		Browser:  []PluginCardTarget{},
	}
}
func (value *PluginCardCapabilities) UnmarshalJSON(data []byte) error {
	type plain PluginCardCapabilities
	decoded := plain{}
	decoded.Qr = nil
	decoded.Refresh = false
	decoded.Settings = false
	decoded.Browser = []PluginCardTarget{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginCardCapabilities(decoded)
	return nil
}
func (value PluginCardCapabilities) MarshalJSON() ([]byte, error) {
	type plain PluginCardCapabilities
	encoded := plain(value)
	encoded.Browser = normalizeCollections(encoded.Browser)
	return json.Marshal(encoded)
}

type PluginCardQr struct {
	Payload   string  `json:"payload"`
	ExpiresAt *string `json:"expires_at,omitempty"`
}

func DefaultPluginCardQr() PluginCardQr {
	return PluginCardQr{
		Payload:   "",
		ExpiresAt: nil,
	}
}
func (value *PluginCardQr) UnmarshalJSON(data []byte) error {
	type plain PluginCardQr
	decoded := plain{}
	decoded.ExpiresAt = nil
	if err := DecodeObject(data, &decoded, []string{"payload"}, true); err != nil {
		return err
	}
	*value = PluginCardQr(decoded)
	return nil
}
func (value PluginCardQr) MarshalJSON() ([]byte, error) {
	type plain PluginCardQr
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginCardTarget struct {
	Name    string           `json:"name"`
	Origins []string         `json:"origins"`
	Source  PluginCardSource `json:"source"`
	Modes   []PluginCardMode `json:"modes"`
}

func DefaultPluginCardTarget() PluginCardTarget {
	return PluginCardTarget{
		Name:    "",
		Origins: []string{},
		Source:  PluginCardSource{},
		Modes:   []PluginCardMode{},
	}
}
func (value *PluginCardTarget) UnmarshalJSON(data []byte) error {
	type plain PluginCardTarget
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "origins", "source", "modes"}, true); err != nil {
		return err
	}
	*value = PluginCardTarget(decoded)
	return nil
}
func (value PluginCardTarget) MarshalJSON() ([]byte, error) {
	type plain PluginCardTarget
	encoded := plain(value)
	encoded.Origins = normalizeCollections(encoded.Origins)
	encoded.Modes = normalizeCollections(encoded.Modes)
	return json.Marshal(encoded)
}

type PluginCardSource struct {
	Kind string
	Data any
}
type PluginCardSourcePropsPayload struct {
	Pointer string `json:"pointer"`
}

func DefaultPluginCardSourcePropsPayload() PluginCardSourcePropsPayload {
	return PluginCardSourcePropsPayload{
		Pointer: "",
	}
}
func (value *PluginCardSourcePropsPayload) UnmarshalJSON(data []byte) error {
	type plain PluginCardSourcePropsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"pointer"}, true); err != nil {
		return err
	}
	*value = PluginCardSourcePropsPayload(decoded)
	return nil
}
func (value PluginCardSourcePropsPayload) MarshalJSON() ([]byte, error) {
	type plain PluginCardSourcePropsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginCardSourceVariants = variantFactories{"props": func() any { return new(PluginCardSourcePropsPayload) },
	"mcp_authorization": nil}

func (value PluginCardSource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginCardSourceVariants)
}
func (value *PluginCardSource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginCardSourceVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginCardMode string

const (
	PluginCardModeEmbedded PluginCardMode = "embedded"
	PluginCardModeExternal PluginCardMode = "external"
)

func (value PluginCardMode) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginCardMode) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginCardMode(decoded) {
	case PluginCardModeEmbedded, PluginCardModeExternal:
		*value = PluginCardMode(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginCardMode) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginCardMode) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginCardDraft struct {
	Key      string                  `json:"key"`
	Props    json.RawMessage         `json:"props"`
	Fallback PluginDesktopNavigation `json:"fallback"`
}

func DefaultPluginCardDraft() PluginCardDraft {
	return PluginCardDraft{
		Key:      "",
		Props:    json.RawMessage("null"),
		Fallback: DefaultPluginDesktopNavigation(),
	}
}
func (value *PluginCardDraft) UnmarshalJSON(data []byte) error {
	type plain PluginCardDraft
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "props", "fallback"}, true); err != nil {
		return err
	}
	*value = PluginCardDraft(decoded)
	return nil
}
func (value PluginCardDraft) MarshalJSON() ([]byte, error) {
	type plain PluginCardDraft
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginCardSummary struct {
	Receipt  PluginCardReceipt       `json:"receipt"`
	Node     NodeID                  `json:"node"`
	Worktree WorktreeID              `json:"worktree"`
	Package  PluginReference         `json:"package"`
	Key      string                  `json:"key"`
	Props    json.RawMessage         `json:"props"`
	Fallback PluginDesktopNavigation `json:"fallback"`
}

func DefaultPluginCardSummary() PluginCardSummary {
	return PluginCardSummary{
		Receipt:  DefaultPluginCardReceipt(),
		Node:     NodeID{},
		Worktree: WorktreeID{},
		Package:  DefaultPluginReference(),
		Key:      "",
		Props:    json.RawMessage("null"),
		Fallback: DefaultPluginDesktopNavigation(),
	}
}
func (value *PluginCardSummary) UnmarshalJSON(data []byte) error {
	type plain PluginCardSummary
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"receipt", "node", "worktree", "package", "key", "props", "fallback"}, true); err != nil {
		return err
	}
	*value = PluginCardSummary(decoded)
	return nil
}
func (value PluginCardSummary) MarshalJSON() ([]byte, error) {
	type plain PluginCardSummary
	encoded := plain(value)
	return json.Marshal(encoded)
}
