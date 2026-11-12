// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ConversationConsentScopes struct {
	Messages    bool `json:"messages"`
	Credentials bool `json:"credentials"`
}

func DefaultConversationConsentScopes() ConversationConsentScopes {
	return ConversationConsentScopes{
		Messages:    false,
		Credentials: false,
	}
}
func (value *ConversationConsentScopes) UnmarshalJSON(data []byte) error {
	type plain ConversationConsentScopes
	decoded := plain{}
	decoded.Messages = false
	decoded.Credentials = false
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ConversationConsentScopes(decoded)
	return nil
}
func (value ConversationConsentScopes) MarshalJSON() ([]byte, error) {
	type plain ConversationConsentScopes
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationConsentApproval struct {
	Package PluginReference            `json:"package"`
	Scopes  ConversationConsentScopes  `json:"scopes"`
	Origin  *ConversationConsentOrigin `json:"origin"`
}

func DefaultConversationConsentApproval() ConversationConsentApproval {
	return ConversationConsentApproval{
		Package: DefaultPluginReference(),
		Scopes:  DefaultConversationConsentScopes(),
		Origin:  nil,
	}
}
func (value *ConversationConsentApproval) UnmarshalJSON(data []byte) error {
	type plain ConversationConsentApproval
	decoded := plain{}
	decoded.Origin = nil
	if err := DecodeObject(data, &decoded, []string{"package", "scopes"}, true); err != nil {
		return err
	}
	*value = ConversationConsentApproval(decoded)
	return nil
}
func (value ConversationConsentApproval) MarshalJSON() ([]byte, error) {
	type plain ConversationConsentApproval
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationConsentOrigin struct {
	Kind string
	Data any
}
type ConversationConsentOriginGitPayload struct {
	Repository string `json:"repository"`
	Path       string `json:"path"`
}

func DefaultConversationConsentOriginGitPayload() ConversationConsentOriginGitPayload {
	return ConversationConsentOriginGitPayload{
		Repository: "",
		Path:       "",
	}
}
func (value *ConversationConsentOriginGitPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationConsentOriginGitPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"repository", "path"}, true); err != nil {
		return err
	}
	*value = ConversationConsentOriginGitPayload(decoded)
	return nil
}
func (value ConversationConsentOriginGitPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationConsentOriginGitPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationConsentOriginRegistryPayload struct {
	Namespace string `json:"namespace"`
	Slug      string `json:"slug"`
}

func DefaultConversationConsentOriginRegistryPayload() ConversationConsentOriginRegistryPayload {
	return ConversationConsentOriginRegistryPayload{
		Namespace: "",
		Slug:      "",
	}
}
func (value *ConversationConsentOriginRegistryPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationConsentOriginRegistryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"namespace", "slug"}, true); err != nil {
		return err
	}
	*value = ConversationConsentOriginRegistryPayload(decoded)
	return nil
}
func (value ConversationConsentOriginRegistryPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationConsentOriginRegistryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationConsentOriginWorktreePayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
}

func DefaultConversationConsentOriginWorktreePayload() ConversationConsentOriginWorktreePayload {
	return ConversationConsentOriginWorktreePayload{
		Worktree: WorktreeID{},
		Path:     "",
	}
}
func (value *ConversationConsentOriginWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain ConversationConsentOriginWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, true); err != nil {
		return err
	}
	*value = ConversationConsentOriginWorktreePayload(decoded)
	return nil
}
func (value ConversationConsentOriginWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain ConversationConsentOriginWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationConsentOriginImportedPayload struct {
	Digest string `json:"digest"`
}

func DefaultConversationConsentOriginImportedPayload() ConversationConsentOriginImportedPayload {
	return ConversationConsentOriginImportedPayload{
		Digest: "",
	}
}
func (value *ConversationConsentOriginImportedPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationConsentOriginImportedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"digest"}, true); err != nil {
		return err
	}
	*value = ConversationConsentOriginImportedPayload(decoded)
	return nil
}
func (value ConversationConsentOriginImportedPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationConsentOriginImportedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationConsentOriginVariants = variantFactories{"bundled": nil,
	"git":      func() any { return new(ConversationConsentOriginGitPayload) },
	"registry": func() any { return new(ConversationConsentOriginRegistryPayload) },
	"worktree": func() any { return new(ConversationConsentOriginWorktreePayload) },
	"imported": func() any { return new(ConversationConsentOriginImportedPayload) }}

func (value ConversationConsentOrigin) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, conversationConsentOriginVariants)
}
func (value *ConversationConsentOrigin) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", conversationConsentOriginVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
