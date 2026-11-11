// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type Effort struct {
	Kind string
	Data any
}

var effortVariants = variantFactories{"default": nil,
	"none":    nil,
	"minimal": nil,
	"low":     nil,
	"medium":  nil,
	"high":    nil,
	"xhigh":   nil,
	"max":     nil,
	"budget":  func() any { return new(int32) }}

func (value Effort) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, effortVariants)
}
func (value *Effort) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", effortVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
func (value *Effort) DecodeJSON(decoder *Decoder) error {
	kind, decoded, err := decoder.Union("", "", effortVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type Permission string

const (
	PermissionAsk     Permission = "ask"
	PermissionProject Permission = "project"
	PermissionFull    Permission = "full"
)

func (value Permission) MarshalJSON() ([]byte, error)     { return marshalEnum(value) }
func (value *Permission) UnmarshalJSON(data []byte) error { return DecodeValue(data, value) }
func (value *Permission) DecodeJSON(decoder *Decoder) error {
	decoded, err := decoder.String()
	if err != nil {
		return err
	}
	switch Permission(decoded) {
	case PermissionAsk, PermissionProject, PermissionFull:
		*value = Permission(decoded)
		return nil
	default:
		return settleEnum(decoder.tolerance, value, decoded)
	}
}
func (value Permission) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *Permission) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type WorkMode string

const (
	WorkModePlan     WorkMode = "plan"
	WorkModeCode     WorkMode = "code"
	WorkModeFlagship WorkMode = "flagship"
)

func (value WorkMode) MarshalJSON() ([]byte, error)     { return marshalEnum(value) }
func (value *WorkMode) UnmarshalJSON(data []byte) error { return DecodeValue(data, value) }
func (value *WorkMode) DecodeJSON(decoder *Decoder) error {
	decoded, err := decoder.String()
	if err != nil {
		return err
	}
	switch WorkMode(decoded) {
	case WorkModePlan, WorkModeCode, WorkModeFlagship:
		*value = WorkMode(decoded)
		return nil
	default:
		return settleEnum(decoder.tolerance, value, decoded)
	}
}
func (value WorkMode) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *WorkMode) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type CredentialRef struct {
	Node NodeID       `json:"node"`
	ID   CredentialID `json:"id"`
}

func DefaultCredentialRef() CredentialRef {
	return CredentialRef{
		Node: NodeID{},
		ID:   CredentialID{},
	}
}
func (value *CredentialRef) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *CredentialRef) DecodeJSON(decoder *Decoder) error {
	type plain CredentialRef
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"node", "id"}, false); err != nil {
		return err
	}
	*value = CredentialRef(decoded)
	return nil
}
func (value CredentialRef) MarshalJSON() ([]byte, error) {
	type plain CredentialRef
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SessionConfig struct {
	Assistant  *PluginConversationBinding            `json:"assistant,omitempty"`
	Resource   *ConnectionResource                   `json:"resource,omitempty"`
	Provider   ProviderID                            `json:"provider"`
	Model      string                                `json:"model"`
	Effort     Effort                                `json:"effort"`
	Features   map[string]string                     `json:"features,omitempty"`
	Mode       WorkMode                              `json:"mode"`
	Options    map[string]map[string]json.RawMessage `json:"options"`
	Permission Permission                            `json:"permission"`
	Credential *CredentialRef                        `json:"credential"`
}

func DefaultSessionConfig() SessionConfig {
	return SessionConfig{
		Assistant:  nil,
		Resource:   nil,
		Provider:   ProviderID{},
		Model:      "",
		Effort:     Effort{Kind: "default"},
		Features:   map[string]string{},
		Mode:       WorkMode(""),
		Options:    map[string]map[string]json.RawMessage{},
		Permission: Permission(""),
		Credential: nil,
	}
}
func (value *SessionConfig) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *SessionConfig) DecodeJSON(decoder *Decoder) error {
	type plain SessionConfig
	decoded := plain{}
	decoded.Effort = Effort{Kind: "default"}
	decoded.Features = map[string]string{}
	decoded.Options = map[string]map[string]json.RawMessage{}
	if err := decoder.Object(&decoded, []string{"provider", "model", "mode", "permission"}, false); err != nil {
		return err
	}
	*value = SessionConfig(decoded)
	return nil
}
func (value SessionConfig) MarshalJSON() ([]byte, error) {
	type plain SessionConfig
	encoded := plain(value)
	encoded.Features = normalizeCollections(encoded.Features)
	encoded.Options = normalizeCollections(encoded.Options)
	return json.Marshal(encoded)
}

type Defaults struct {
	Revision uint64         `json:"revision"`
	Config   *SessionConfig `json:"config"`
}

func DefaultDefaults() Defaults {
	return Defaults{
		Revision: 0,
		Config:   nil,
	}
}
func (value *Defaults) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *Defaults) DecodeJSON(decoder *Decoder) error {
	type plain Defaults
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = Defaults(decoded)
	return nil
}
func (value Defaults) MarshalJSON() ([]byte, error) {
	type plain Defaults
	encoded := plain(value)
	return json.Marshal(encoded)
}

type Session struct {
	ID            SessionID                `json:"id"`
	Association   *ConversationAssociation `json:"association,omitempty"`
	CreationOrder uint64                   `json:"creation_order"`
	Archived      bool                     `json:"archived"`
	Activity      ActivitySummary          `json:"activity"`
	Project       *ProjectID               `json:"project"`
	Worktree      WorktreeID               `json:"worktree"`
	Revision      uint64                   `json:"revision"`
	Config        SessionConfig            `json:"config"`
	Roles         RoleSnapshot             `json:"roles"`
	Profile       *SessionProfile          `json:"profile,omitempty"`
	Fork          *ConversationFork        `json:"fork,omitempty"`
	Delegation    **Delegation             `json:"delegation,omitempty"`
}

func DefaultSession() Session {
	return Session{
		ID:            SessionID{},
		Association:   nil,
		CreationOrder: 0,
		Archived:      false,
		Activity:      DefaultActivitySummary(),
		Project:       nil,
		Worktree:      WorktreeID{},
		Revision:      0,
		Config:        DefaultSessionConfig(),
		Roles:         DefaultRoleSnapshot(),
		Profile:       nil,
		Fork:          nil,
		Delegation:    nil,
	}
}
func (value *Session) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *Session) DecodeJSON(decoder *Decoder) error {
	type plain Session
	decoded := plain{}
	decoded.Association = nil
	decoded.CreationOrder = 0
	if err := decoder.Object(&decoded, []string{"id", "archived", "activity", "worktree", "revision", "config", "roles"}, false); err != nil {
		return err
	}
	*value = Session(decoded)
	return nil
}
func (value Session) MarshalJSON() ([]byte, error) {
	type plain Session
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SessionRevision struct {
	Session  SessionID `json:"session"`
	Revision uint64    `json:"revision"`
}

func DefaultSessionRevision() SessionRevision {
	return SessionRevision{
		Session:  SessionID{},
		Revision: 0,
	}
}
func (value *SessionRevision) UnmarshalJSON(data []byte) error {
	type plain SessionRevision
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "revision"}, false); err != nil {
		return err
	}
	*value = SessionRevision(decoded)
	return nil
}
func (value SessionRevision) MarshalJSON() ([]byte, error) {
	type plain SessionRevision
	encoded := plain(value)
	return json.Marshal(encoded)
}

type Delegation struct {
	Session SessionID `json:"session"`
	Turn    TurnID    `json:"turn"`
	Entry   string    `json:"entry"`
	Index   uint64    `json:"index"`
	Role    *RoleID   `json:"role"`
}

func DefaultDelegation() Delegation {
	return Delegation{
		Session: SessionID{},
		Turn:    TurnID{},
		Entry:   "",
		Index:   0,
		Role:    nil,
	}
}
func (value *Delegation) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *Delegation) DecodeJSON(decoder *Decoder) error {
	type plain Delegation
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"session", "turn", "entry", "index"}, false); err != nil {
		return err
	}
	*value = Delegation(decoded)
	return nil
}
func (value Delegation) MarshalJSON() ([]byte, error) {
	type plain Delegation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SessionProfile struct {
	Source   NodeID               `json:"source"`
	Provider ConversationProvider `json:"provider"`
}

func DefaultSessionProfile() SessionProfile {
	return SessionProfile{
		Source:   NodeID{},
		Provider: DefaultConversationProvider(),
	}
}
func (value *SessionProfile) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *SessionProfile) DecodeJSON(decoder *Decoder) error {
	type plain SessionProfile
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"source", "provider"}, false); err != nil {
		return err
	}
	*value = SessionProfile(decoded)
	return nil
}
func (value SessionProfile) MarshalJSON() ([]byte, error) {
	type plain SessionProfile
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SessionImport struct {
	Source          NodeID               `json:"source"`
	Project         *ProjectID           `json:"project"`
	Worktree        *WorktreeID          `json:"worktree"`
	Config          SessionConfig        `json:"config"`
	Provider        ConversationProvider `json:"provider"`
	Secret          *Secret              `json:"secret"`
	ExpiresAtMs     *uint64              `json:"expires_at_ms"`
	Roles           RoleSnapshot         `json:"roles"`
	RoleCredentials []RoleProviderSecret `json:"role_credentials"`
}

func DefaultSessionImport() SessionImport {
	return SessionImport{
		Source:          NodeID{},
		Project:         nil,
		Worktree:        nil,
		Config:          DefaultSessionConfig(),
		Provider:        DefaultConversationProvider(),
		Secret:          nil,
		ExpiresAtMs:     nil,
		Roles:           DefaultRoleSnapshot(),
		RoleCredentials: []RoleProviderSecret{},
	}
}
func (value *SessionImport) UnmarshalJSON(data []byte) error {
	type plain SessionImport
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "config", "provider", "roles", "role_credentials"}, false); err != nil {
		return err
	}
	*value = SessionImport(decoded)
	return nil
}
func (value SessionImport) MarshalJSON() ([]byte, error) {
	type plain SessionImport
	encoded := plain(value)
	encoded.RoleCredentials = normalizeCollections(encoded.RoleCredentials)
	return json.Marshal(encoded)
}

type TurnSummary struct {
	ID      TurnID    `json:"id"`
	Session SessionID `json:"session"`
	Request RequestID `json:"request"`
}

func DefaultTurnSummary() TurnSummary {
	return TurnSummary{
		ID:      TurnID{},
		Session: SessionID{},
		Request: RequestID{},
	}
}
func (value *TurnSummary) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *TurnSummary) DecodeJSON(decoder *Decoder) error {
	type plain TurnSummary
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"id", "session", "request"}, false); err != nil {
		return err
	}
	*value = TurnSummary(decoded)
	return nil
}
func (value TurnSummary) MarshalJSON() ([]byte, error) {
	type plain TurnSummary
	encoded := plain(value)
	return json.Marshal(encoded)
}

type QueuedTurn struct {
	ID       TurnID              `json:"id"`
	Kind     ConversationRunKind `json:"kind"`
	Session  SessionID           `json:"session"`
	Request  RequestID           `json:"request"`
	Revision uint64              `json:"revision"`
	Config   SessionConfig       `json:"config"`
	Roles    RoleSnapshot        `json:"roles"`
	Plugins  []PluginReference   `json:"plugins"`
}

func DefaultQueuedTurn() QueuedTurn {
	return QueuedTurn{
		ID:       TurnID{},
		Kind:     ConversationRunKind(""),
		Session:  SessionID{},
		Request:  RequestID{},
		Revision: 0,
		Config:   DefaultSessionConfig(),
		Roles:    DefaultRoleSnapshot(),
		Plugins:  []PluginReference{},
	}
}
func (value *QueuedTurn) UnmarshalJSON(data []byte) error {
	type plain QueuedTurn
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "kind", "session", "request", "revision", "config", "roles", "plugins"}, false); err != nil {
		return err
	}
	*value = QueuedTurn(decoded)
	return nil
}
func (value QueuedTurn) MarshalJSON() ([]byte, error) {
	type plain QueuedTurn
	encoded := plain(value)
	encoded.Plugins = normalizeCollections(encoded.Plugins)
	return json.Marshal(encoded)
}
