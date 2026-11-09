// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type Authentication string

const (
	AuthenticationApiKey       Authentication = "api_key"
	AuthenticationChatGpt      Authentication = "chat_gpt"
	AuthenticationChatGptOAuth Authentication = "chat_gpt_oauth"
	AuthenticationClaudeOAuth  Authentication = "claude_oauth"
	AuthenticationCopilot      Authentication = "copilot"
	AuthenticationPlugin       Authentication = "plugin"
)

func (value Authentication) MarshalJSON() ([]byte, error) {
	switch value {
	case AuthenticationApiKey, AuthenticationChatGpt, AuthenticationChatGptOAuth, AuthenticationClaudeOAuth, AuthenticationCopilot, AuthenticationPlugin:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown Authentication %q", value)
	}
}
func (value *Authentication) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch Authentication(decoded) {
	case AuthenticationApiKey, AuthenticationChatGpt, AuthenticationChatGptOAuth, AuthenticationClaudeOAuth, AuthenticationCopilot, AuthenticationPlugin:
		*value = Authentication(decoded)
		return nil
	default:
		return fmt.Errorf("unknown Authentication %q", decoded)
	}
}
func (value Authentication) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *Authentication) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type Credential struct {
	ID             CredentialID   `json:"id"`
	Provider       ProviderID     `json:"provider"`
	Authentication Authentication `json:"authentication"`
	AccountLabel   *string        `json:"account_label,omitempty"`
	Revision       uint64         `json:"revision"`
	ExpiresAtMs    *uint64        `json:"expires_at_ms"`
	Revoked        bool           `json:"revoked"`
}

func DefaultCredential() Credential {
	return Credential{
		ID:             CredentialID{},
		Provider:       ProviderID{},
		Authentication: Authentication(""),
		AccountLabel:   nil,
		Revision:       0,
		ExpiresAtMs:    nil,
		Revoked:        false,
	}
}
func (value *Credential) UnmarshalJSON(data []byte) error {
	type plain Credential
	decoded := plain{}
	decoded.AccountLabel = nil
	if err := DecodeObject(data, &decoded, []string{"id", "provider", "authentication", "revision", "revoked"}, false); err != nil {
		return err
	}
	*value = Credential(decoded)
	return nil
}
func (value Credential) MarshalJSON() ([]byte, error) {
	type plain Credential
	encoded := plain(value)
	return json.Marshal(encoded)
}
