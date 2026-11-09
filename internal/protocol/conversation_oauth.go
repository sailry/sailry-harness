// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationOauthOptions struct {
	Kind string
	Data any
}
type ConversationOauthOptionsPluginPayload struct {
	Package  PluginReference               `json:"package"`
	Flow     string                        `json:"flow"`
	Endpoint string                        `json:"endpoint"`
	Consent  **ConversationConsentApproval `json:"consent"`
	Origin   *ConversationConsentOrigin    `json:"origin"`
}

func DefaultConversationOauthOptionsPluginPayload() ConversationOauthOptionsPluginPayload {
	return ConversationOauthOptionsPluginPayload{
		Package:  DefaultPluginReference(),
		Flow:     "",
		Endpoint: "",
		Consent:  nil,
		Origin:   nil,
	}
}
func (value *ConversationOauthOptionsPluginPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationOauthOptionsPluginPayload
	decoded := plain{}
	decoded.Consent = nil
	decoded.Origin = nil
	if err := DecodeObject(data, &decoded, []string{"package", "flow", "endpoint"}, true); err != nil {
		return err
	}
	*value = ConversationOauthOptionsPluginPayload(decoded)
	return nil
}
func (value ConversationOauthOptionsPluginPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationOauthOptionsPluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationOauthOptionsChatGptPayload struct {
	CatalogVersion *string `json:"catalog_version,omitempty"`
	UserAgent      string  `json:"user_agent"`
}

func DefaultConversationOauthOptionsChatGptPayload() ConversationOauthOptionsChatGptPayload {
	return ConversationOauthOptionsChatGptPayload{
		CatalogVersion: nil,
		UserAgent:      "",
	}
}
func (value *ConversationOauthOptionsChatGptPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationOauthOptionsChatGptPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"user_agent"}, true); err != nil {
		return err
	}
	*value = ConversationOauthOptionsChatGptPayload(decoded)
	return nil
}
func (value ConversationOauthOptionsChatGptPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationOauthOptionsChatGptPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationOauthOptionsChatGptOAuthPayload struct {
	CatalogVersion *string `json:"catalog_version,omitempty"`
	UserAgent      string  `json:"user_agent"`
}

func DefaultConversationOauthOptionsChatGptOAuthPayload() ConversationOauthOptionsChatGptOAuthPayload {
	return ConversationOauthOptionsChatGptOAuthPayload{
		CatalogVersion: nil,
		UserAgent:      "",
	}
}
func (value *ConversationOauthOptionsChatGptOAuthPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationOauthOptionsChatGptOAuthPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"user_agent"}, true); err != nil {
		return err
	}
	*value = ConversationOauthOptionsChatGptOAuthPayload(decoded)
	return nil
}
func (value ConversationOauthOptionsChatGptOAuthPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationOauthOptionsChatGptOAuthPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationOauthOptionsClaudeOAuthPayload struct {
	UserAgent string `json:"user_agent"`
}

func DefaultConversationOauthOptionsClaudeOAuthPayload() ConversationOauthOptionsClaudeOAuthPayload {
	return ConversationOauthOptionsClaudeOAuthPayload{
		UserAgent: "",
	}
}
func (value *ConversationOauthOptionsClaudeOAuthPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationOauthOptionsClaudeOAuthPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"user_agent"}, true); err != nil {
		return err
	}
	*value = ConversationOauthOptionsClaudeOAuthPayload(decoded)
	return nil
}
func (value ConversationOauthOptionsClaudeOAuthPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationOauthOptionsClaudeOAuthPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationOauthOptionsCopilotPayload struct {
	UserAgent           string `json:"user_agent"`
	EditorVersion       string `json:"editor_version"`
	EditorPluginVersion string `json:"editor_plugin_version"`
}

func DefaultConversationOauthOptionsCopilotPayload() ConversationOauthOptionsCopilotPayload {
	return ConversationOauthOptionsCopilotPayload{
		UserAgent:           "",
		EditorVersion:       "",
		EditorPluginVersion: "",
	}
}
func (value *ConversationOauthOptionsCopilotPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationOauthOptionsCopilotPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"user_agent", "editor_version", "editor_plugin_version"}, true); err != nil {
		return err
	}
	*value = ConversationOauthOptionsCopilotPayload(decoded)
	return nil
}
func (value ConversationOauthOptionsCopilotPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationOauthOptionsCopilotPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationOauthOptionsVariants = variantFactories{"plugin": func() any { return new(ConversationOauthOptionsPluginPayload) },
	"chat_gpt":       func() any { return new(ConversationOauthOptionsChatGptPayload) },
	"chat_gpt_oauth": func() any { return new(ConversationOauthOptionsChatGptOAuthPayload) },
	"claude_oauth":   func() any { return new(ConversationOauthOptionsClaudeOAuthPayload) },
	"copilot":        func() any { return new(ConversationOauthOptionsCopilotPayload) }}

func (value ConversationOauthOptions) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, conversationOauthOptionsVariants)
}
func (value *ConversationOauthOptions) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", conversationOauthOptionsVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
