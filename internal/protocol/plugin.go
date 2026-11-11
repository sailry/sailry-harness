// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PluginUploadSpec struct {
	Size     uint64 `json:"size"`
	Revision string `json:"revision"`
}

func DefaultPluginUploadSpec() PluginUploadSpec {
	return PluginUploadSpec{
		Size:     0,
		Revision: "",
	}
}
func (value *PluginUploadSpec) UnmarshalJSON(data []byte) error {
	type plain PluginUploadSpec
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"size", "revision"}, false); err != nil {
		return err
	}
	*value = PluginUploadSpec(decoded)
	return nil
}
func (value PluginUploadSpec) MarshalJSON() ([]byte, error) {
	type plain PluginUploadSpec
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginUpload struct {
	Stream StreamID         `json:"stream"`
	Spec   PluginUploadSpec `json:"spec"`
}

func DefaultPluginUpload() PluginUpload {
	return PluginUpload{
		Stream: StreamID{},
		Spec:   DefaultPluginUploadSpec(),
	}
}
func (value *PluginUpload) UnmarshalJSON(data []byte) error {
	type plain PluginUpload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "spec"}, false); err != nil {
		return err
	}
	*value = PluginUpload(decoded)
	return nil
}
func (value PluginUpload) MarshalJSON() ([]byte, error) {
	type plain PluginUpload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginContext struct {
	Invocation *RequestID           `json:"invocation,omitempty"`
	Turn       *TurnID              `json:"turn,omitempty"`
	Surface    PluginDesktopSurface `json:"surface"`
	Package    PluginReference      `json:"package"`
	Worktree   *WorktreeID          `json:"worktree"`
	Session    *SessionID           `json:"session"`
}

func DefaultPluginContext() PluginContext {
	return PluginContext{
		Invocation: nil,
		Turn:       nil,
		Surface:    PluginDesktopSurface{Kind: "workspace"},
		Package:    DefaultPluginReference(),
		Worktree:   nil,
		Session:    nil,
	}
}
func (value *PluginContext) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *PluginContext) DecodeJSON(decoder *Decoder) error {
	type plain PluginContext
	decoded := plain{}
	decoded.Invocation = nil
	decoded.Turn = nil
	decoded.Surface = PluginDesktopSurface{Kind: "workspace"}
	if err := decoder.Object(&decoded, []string{"package"}, false); err != nil {
		return err
	}
	*value = PluginContext(decoded)
	return nil
}
func (value PluginContext) MarshalJSON() ([]byte, error) {
	type plain PluginContext
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginText struct {
	Text   string `json:"text"`
	Model  string `json:"model"`
	Tokens uint64 `json:"tokens"`
}

func DefaultPluginText() PluginText {
	return PluginText{
		Text:   "",
		Model:  "",
		Tokens: 0,
	}
}
func (value *PluginText) UnmarshalJSON(data []byte) error {
	type plain PluginText
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text", "model", "tokens"}, false); err != nil {
		return err
	}
	*value = PluginText(decoded)
	return nil
}
func (value PluginText) MarshalJSON() ([]byte, error) {
	type plain PluginText
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginCategory string

const (
	PluginCategoryDevelopment   PluginCategory = "development"
	PluginCategoryDocuments     PluginCategory = "documents"
	PluginCategoryData          PluginCategory = "data"
	PluginCategoryAutomation    PluginCategory = "automation"
	PluginCategoryEntertainment PluginCategory = "entertainment"
)

func (value PluginCategory) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginCategory) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginCategory(decoded) {
	case PluginCategoryDevelopment, PluginCategoryDocuments, PluginCategoryData, PluginCategoryAutomation, PluginCategoryEntertainment:
		*value = PluginCategory(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginCategory) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginCategory) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginTint string

const (
	PluginTintBlue   PluginTint = "blue"
	PluginTintViolet PluginTint = "violet"
	PluginTintTeal   PluginTint = "teal"
	PluginTintAmber  PluginTint = "amber"
	PluginTintGray   PluginTint = "gray"
)

func (value PluginTint) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginTint) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginTint(decoded) {
	case PluginTintBlue, PluginTintViolet, PluginTintTeal, PluginTintAmber, PluginTintGray:
		*value = PluginTint(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginTint) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginTint) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginExtension struct {
	MinSailryVersion    *string                            `json:"min_sailry_version,omitempty"`
	ApiVersion          string                             `json:"api_version"`
	Permissions         []PermissionsContribution          `json:"permissions,omitempty"`
	Connectors          []PluginConnectorDeclaration       `json:"connectors,omitempty"`
	Category            *PluginCategory                    `json:"category,omitempty"`
	Tint                *PluginTint                        `json:"tint,omitempty"`
	Instructions        *string                            `json:"instructions,omitempty"`
	ContinuationMarker  *PluginDesktopNavigation           `json:"continuation_marker,omitempty"`
	ContinuationMarkers map[string]PluginDesktopNavigation `json:"continuation_markers,omitempty"`
	ArtifactView        *ArtifactView                      `json:"artifact_view,omitempty"`
	Display             *PluginDesktopNavigation           `json:"display"`
	Icon                *string                            `json:"icon,omitempty"`
	Description         *PluginDesktopNavigation           `json:"description,omitempty"`
	Scope               PluginScope                        `json:"scope"`
	Actions             []PluginAction                     `json:"actions"`
	EnabledService      *PluginService                     `json:"enabled_service,omitempty"`
	Desktop             *PluginDesktopManifest             `json:"desktop"`
	Host                *PluginHostManifest                `json:"host"`
	SettingsSchema      *string                            `json:"settings_schema"`
	ConversationOptions *json.RawMessage                   `json:"conversation_options,omitempty"`
	ConversationModes   []PluginModesDeclaration           `json:"conversation_modes,omitempty"`
	SettingsPage        *PluginDesktopSettings             `json:"settings_page"`
	Storage             *PluginStorageDeclaration          `json:"storage,omitempty"`
	Tools               []ToolDeclaration                  `json:"tools"`
	ModelProviders      []ConversationPresetsPreset        `json:"model_providers"`
	ModelAuth           []PluginModelAuthDeclaration       `json:"model_auth"`
	ModelResources      []PluginModelAuthResource          `json:"model_resources,omitempty"`
	ModelAdapters       []PluginModelAdapterDeclaration    `json:"model_adapters"`
	Ui                  []PluginUiContribution             `json:"ui"`
	Cards               []PluginCardDeclaration            `json:"cards,omitempty"`
}

func DefaultPluginExtension() PluginExtension {
	return PluginExtension{
		MinSailryVersion:    nil,
		ApiVersion:          "",
		Permissions:         []PermissionsContribution{},
		Connectors:          []PluginConnectorDeclaration{},
		Category:            nil,
		Tint:                nil,
		Instructions:        nil,
		ContinuationMarker:  nil,
		ContinuationMarkers: nil,
		ArtifactView:        nil,
		Display:             nil,
		Icon:                nil,
		Description:         nil,
		Scope:               PluginScopeHost,
		Actions:             []PluginAction{},
		EnabledService:      nil,
		Desktop:             nil,
		Host:                nil,
		SettingsSchema:      nil,
		ConversationOptions: nil,
		ConversationModes:   []PluginModesDeclaration{},
		SettingsPage:        nil,
		Storage:             nil,
		Tools:               []ToolDeclaration{},
		ModelProviders:      []ConversationPresetsPreset{},
		ModelAuth:           []PluginModelAuthDeclaration{},
		ModelResources:      []PluginModelAuthResource{},
		ModelAdapters:       []PluginModelAdapterDeclaration{},
		Ui:                  []PluginUiContribution{},
		Cards:               []PluginCardDeclaration{},
	}
}
func (value *PluginExtension) UnmarshalJSON(data []byte) error {
	type plain PluginExtension
	decoded := plain{}
	decoded.MinSailryVersion = nil
	decoded.Permissions = []PermissionsContribution{}
	decoded.Connectors = []PluginConnectorDeclaration{}
	decoded.Category = nil
	decoded.Tint = nil
	decoded.Instructions = nil
	decoded.ContinuationMarker = nil
	decoded.ContinuationMarkers = nil
	decoded.ArtifactView = nil
	decoded.Icon = nil
	decoded.Description = nil
	decoded.Scope = PluginScopeHost
	decoded.Actions = []PluginAction{}
	decoded.EnabledService = nil
	decoded.ConversationOptions = nil
	decoded.ConversationModes = []PluginModesDeclaration{}
	decoded.Storage = nil
	decoded.Tools = []ToolDeclaration{}
	decoded.ModelProviders = []ConversationPresetsPreset{}
	decoded.ModelAuth = []PluginModelAuthDeclaration{}
	decoded.ModelResources = []PluginModelAuthResource{}
	decoded.ModelAdapters = []PluginModelAdapterDeclaration{}
	decoded.Ui = []PluginUiContribution{}
	decoded.Cards = []PluginCardDeclaration{}
	if err := DecodeObject(data, &decoded, []string{"api_version"}, true); err != nil {
		return err
	}
	*value = PluginExtension(decoded)
	return nil
}
func (value PluginExtension) MarshalJSON() ([]byte, error) {
	type plain PluginExtension
	encoded := plain(value)
	encoded.Permissions = normalizeCollections(encoded.Permissions)
	encoded.Connectors = normalizeCollections(encoded.Connectors)
	encoded.Actions = normalizeCollections(encoded.Actions)
	encoded.ConversationModes = normalizeCollections(encoded.ConversationModes)
	encoded.Tools = normalizeCollections(encoded.Tools)
	encoded.ModelProviders = normalizeCollections(encoded.ModelProviders)
	encoded.ModelAuth = normalizeCollections(encoded.ModelAuth)
	encoded.ModelResources = normalizeCollections(encoded.ModelResources)
	encoded.ModelAdapters = normalizeCollections(encoded.ModelAdapters)
	encoded.Ui = normalizeCollections(encoded.Ui)
	encoded.Cards = normalizeCollections(encoded.Cards)
	return json.Marshal(encoded)
}

type PluginService string

const (
	PluginServiceCodeIndex PluginService = "code_index"
	PluginServiceLanguage  PluginService = "language"
)

func (value PluginService) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginService) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginService(decoded) {
	case PluginServiceCodeIndex, PluginServiceLanguage:
		*value = PluginService(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginService) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginService) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginScope string

const (
	PluginScopeHost    PluginScope = "host"
	PluginScopeDesktop PluginScope = "desktop"
)

func (value PluginScope) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginScope) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginScope(decoded) {
	case PluginScopeHost, PluginScopeDesktop:
		*value = PluginScope(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginScope) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginScope) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginReference struct {
	Name             string `json:"name"`
	Digest           string `json:"digest"`
	SettingsRevision uint64 `json:"settings_revision"`
}

func DefaultPluginReference() PluginReference {
	return PluginReference{
		Name:             "",
		Digest:           "",
		SettingsRevision: 0,
	}
}
func (value *PluginReference) UnmarshalJSON(data []byte) error {
	type plain PluginReference
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "digest", "settings_revision"}, false); err != nil {
		return err
	}
	*value = PluginReference(decoded)
	return nil
}
func (value PluginReference) MarshalJSON() ([]byte, error) {
	type plain PluginReference
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginCompatibility struct {
	Kind string
	Data any
}
type PluginCompatibilityRequiresNewerPayload struct {
	Required string `json:"required"`
}

func DefaultPluginCompatibilityRequiresNewerPayload() PluginCompatibilityRequiresNewerPayload {
	return PluginCompatibilityRequiresNewerPayload{
		Required: "",
	}
}
func (value *PluginCompatibilityRequiresNewerPayload) UnmarshalJSON(data []byte) error {
	type plain PluginCompatibilityRequiresNewerPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"required"}, false); err != nil {
		return err
	}
	*value = PluginCompatibilityRequiresNewerPayload(decoded)
	return nil
}
func (value PluginCompatibilityRequiresNewerPayload) MarshalJSON() ([]byte, error) {
	type plain PluginCompatibilityRequiresNewerPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginCompatibilityVariants = variantFactories{"compatible": nil,
	"requires_newer": func() any { return new(PluginCompatibilityRequiresNewerPayload) }}

func (value PluginCompatibility) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginCompatibilityVariants)
}
func (value *PluginCompatibility) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginCompatibilityVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginSummary struct {
	Compatibility    PluginCompatibility `json:"compatibility"`
	Name             string              `json:"name"`
	Revision         uint64              `json:"revision"`
	Digest           string              `json:"digest"`
	SettingsRevision uint64              `json:"settings_revision"`
	Enabled          bool                `json:"enabled"`
	Version          *string             `json:"version"`
	Description      *string             `json:"description"`
}

func DefaultPluginSummary() PluginSummary {
	return PluginSummary{
		Compatibility:    PluginCompatibility{Kind: "compatible"},
		Name:             "",
		Revision:         0,
		Digest:           "",
		SettingsRevision: 0,
		Enabled:          false,
		Version:          nil,
		Description:      nil,
	}
}
func (value *PluginSummary) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *PluginSummary) DecodeJSON(decoder *Decoder) error {
	type plain PluginSummary
	decoded := plain{}
	decoded.Compatibility = PluginCompatibility{Kind: "compatible"}
	decoded.Enabled = false
	if err := decoder.Object(&decoded, []string{"name", "revision", "digest", "settings_revision"}, false); err != nil {
		return err
	}
	*value = PluginSummary(decoded)
	return nil
}
func (value PluginSummary) MarshalJSON() ([]byte, error) {
	type plain PluginSummary
	encoded := plain(value)
	data, err := json.Marshal(encoded)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return nil, err
	}
	if encoded.Compatibility.Kind == "compatible" {
		delete(fields, "compatibility")
	}
	return json.Marshal(fields)
}

type PluginInfo struct {
	Versions     []PluginCatalogVersion     `json:"versions,omitempty"`
	ReleaseNotes *PluginCatalogReleaseNotes `json:"release_notes,omitempty"`
	Summary      PluginSummary              `json:"summary"`
	Origin       *PluginOrigin              `json:"origin,omitempty"`
	Icon         *string                    `json:"icon"`
	Skill        *PluginSkillsProvenance    `json:"skill"`
	McpSource    *PluginMcpDefinition       `json:"mcp_source,omitempty"`
	Extension    *PluginExtension           `json:"extension"`
	Skills       []PluginSkill              `json:"skills"`
	Mcp          []PluginMcp                `json:"mcp"`
	Settings     *PluginSettingsSchema      `json:"settings"`
	Issues       []PluginIssue              `json:"issues"`
}

func DefaultPluginInfo() PluginInfo {
	return PluginInfo{
		Versions:     []PluginCatalogVersion{},
		ReleaseNotes: nil,
		Summary:      DefaultPluginSummary(),
		Origin:       nil,
		Icon:         nil,
		Skill:        nil,
		McpSource:    nil,
		Extension:    nil,
		Skills:       []PluginSkill{},
		Mcp:          []PluginMcp{},
		Settings:     nil,
		Issues:       []PluginIssue{},
	}
}
func (value *PluginInfo) UnmarshalJSON(data []byte) error {
	type plain PluginInfo
	decoded := plain{}
	decoded.Versions = []PluginCatalogVersion{}
	decoded.ReleaseNotes = nil
	decoded.Origin = nil
	decoded.Skill = nil
	decoded.McpSource = nil
	decoded.Skills = []PluginSkill{}
	decoded.Mcp = []PluginMcp{}
	decoded.Issues = []PluginIssue{}
	if err := DecodeObject(data, &decoded, []string{"summary"}, false); err != nil {
		return err
	}
	*value = PluginInfo(decoded)
	return nil
}
func (value PluginInfo) MarshalJSON() ([]byte, error) {
	type plain PluginInfo
	encoded := plain(value)
	encoded.Versions = normalizeCollections(encoded.Versions)
	encoded.Skills = normalizeCollections(encoded.Skills)
	encoded.Mcp = normalizeCollections(encoded.Mcp)
	encoded.Issues = normalizeCollections(encoded.Issues)
	return json.Marshal(encoded)
}

type PluginUploadSource string

const (
	PluginUploadSourceDirectory PluginUploadSource = "directory"
	PluginUploadSourceArchive   PluginUploadSource = "archive"
)

func (value PluginUploadSource) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginUploadSource) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginUploadSource(decoded) {
	case PluginUploadSourceDirectory, PluginUploadSourceArchive:
		*value = PluginUploadSource(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginUploadSource) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginUploadSource) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginOrigin struct {
	Kind string
	Data any
}
type PluginOriginOnlinePayload struct {
	Source PluginSkillsResolved `json:"source"`
	Path   string               `json:"path"`
}

func DefaultPluginOriginOnlinePayload() PluginOriginOnlinePayload {
	return PluginOriginOnlinePayload{
		Source: DefaultPluginSkillsResolved(),
		Path:   "",
	}
}
func (value *PluginOriginOnlinePayload) UnmarshalJSON(data []byte) error {
	type plain PluginOriginOnlinePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "path"}, false); err != nil {
		return err
	}
	*value = PluginOriginOnlinePayload(decoded)
	return nil
}
func (value PluginOriginOnlinePayload) MarshalJSON() ([]byte, error) {
	type plain PluginOriginOnlinePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginOriginRegistryPayload struct {
	Source PluginSkillsRegistryResolved `json:"source"`
}

func DefaultPluginOriginRegistryPayload() PluginOriginRegistryPayload {
	return PluginOriginRegistryPayload{
		Source: DefaultPluginSkillsRegistryResolved(),
	}
}
func (value *PluginOriginRegistryPayload) UnmarshalJSON(data []byte) error {
	type plain PluginOriginRegistryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source"}, false); err != nil {
		return err
	}
	*value = PluginOriginRegistryPayload(decoded)
	return nil
}
func (value PluginOriginRegistryPayload) MarshalJSON() ([]byte, error) {
	type plain PluginOriginRegistryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginOriginWorktreePayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
}

func DefaultPluginOriginWorktreePayload() PluginOriginWorktreePayload {
	return PluginOriginWorktreePayload{
		Worktree: WorktreeID{},
		Path:     "",
	}
}
func (value *PluginOriginWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain PluginOriginWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, false); err != nil {
		return err
	}
	*value = PluginOriginWorktreePayload(decoded)
	return nil
}
func (value PluginOriginWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain PluginOriginWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginOriginVariants = variantFactories{"bundled": nil,
	"directory": nil,
	"archive":   nil,
	"online":    func() any { return new(PluginOriginOnlinePayload) },
	"registry":  func() any { return new(PluginOriginRegistryPayload) },
	"worktree":  func() any { return new(PluginOriginWorktreePayload) }}

func (value PluginOrigin) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginOriginVariants)
}
func (value *PluginOrigin) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginOriginVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginSkill struct {
	Name           string            `json:"name"`
	DisplayLocales map[string]string `json:"display_locales,omitempty"`
	DisplayName    *string           `json:"display_name,omitempty"`
	Description    string            `json:"description"`
	Path           string            `json:"path"`
	License        *string           `json:"license"`
	Compatibility  *string           `json:"compatibility"`
}

func DefaultPluginSkill() PluginSkill {
	return PluginSkill{
		Name:           "",
		DisplayLocales: map[string]string{},
		DisplayName:    nil,
		Description:    "",
		Path:           "",
		License:        nil,
		Compatibility:  nil,
	}
}
func (value *PluginSkill) UnmarshalJSON(data []byte) error {
	type plain PluginSkill
	decoded := plain{}
	decoded.DisplayLocales = map[string]string{}
	decoded.DisplayName = nil
	if err := DecodeObject(data, &decoded, []string{"name", "description", "path"}, false); err != nil {
		return err
	}
	*value = PluginSkill(decoded)
	return nil
}
func (value PluginSkill) MarshalJSON() ([]byte, error) {
	type plain PluginSkill
	encoded := plain(value)
	encoded.DisplayLocales = normalizeCollections(encoded.DisplayLocales)
	return json.Marshal(encoded)
}

type PluginMcp struct {
	Host         *string               `json:"host,omitempty"`
	Name         string                `json:"name"`
	Transport    PluginMcpTransport    `json:"transport"`
	Allowed      bool                  `json:"allowed"`
	Availability PluginMcpAvailability `json:"availability"`
	Runtime      *PluginMcpRuntime     `json:"runtime,omitempty"`
}

func DefaultPluginMcp() PluginMcp {
	return PluginMcp{
		Host:         nil,
		Name:         "",
		Transport:    PluginMcpTransport(""),
		Allowed:      false,
		Availability: PluginMcpAvailabilityAvailable,
		Runtime:      nil,
	}
}
func (value *PluginMcp) UnmarshalJSON(data []byte) error {
	type plain PluginMcp
	decoded := plain{}
	decoded.Host = nil
	decoded.Allowed = true
	decoded.Availability = PluginMcpAvailabilityAvailable
	decoded.Runtime = nil
	if err := DecodeObject(data, &decoded, []string{"name", "transport"}, false); err != nil {
		return err
	}
	*value = PluginMcp(decoded)
	return nil
}
func (value PluginMcp) MarshalJSON() ([]byte, error) {
	type plain PluginMcp
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginMcpTransport string

const (
	PluginMcpTransportStdio          PluginMcpTransport = "stdio"
	PluginMcpTransportStreamableHttp PluginMcpTransport = "streamable-http"
	PluginMcpTransportSse            PluginMcpTransport = "sse"
)

func (value PluginMcpTransport) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginMcpTransport) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginMcpTransport(decoded) {
	case PluginMcpTransportStdio, PluginMcpTransportStreamableHttp, PluginMcpTransportSse:
		*value = PluginMcpTransport(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginMcpTransport) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginMcpTransport) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginIssueKind string

const (
	PluginIssueKindIgnoredManifestField PluginIssueKind = "ignored_manifest_field"
	PluginIssueKindInvalidExtensions    PluginIssueKind = "invalid_extensions"
	PluginIssueKindUnsupportedExtension PluginIssueKind = "unsupported_extension"
	PluginIssueKindUnavailablePath      PluginIssueKind = "unavailable_path"
	PluginIssueKindInvalidSkills        PluginIssueKind = "invalid_skills"
	PluginIssueKindInvalidSkill         PluginIssueKind = "invalid_skill"
	PluginIssueKindInvalidMcp           PluginIssueKind = "invalid_mcp"
	PluginIssueKindInvalidMcpServer     PluginIssueKind = "invalid_mcp_server"
)

func (value PluginIssueKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginIssueKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginIssueKind(decoded) {
	case PluginIssueKindIgnoredManifestField, PluginIssueKindInvalidExtensions, PluginIssueKindUnsupportedExtension, PluginIssueKindUnavailablePath, PluginIssueKindInvalidSkills, PluginIssueKindInvalidSkill, PluginIssueKindInvalidMcp, PluginIssueKindInvalidMcpServer:
		*value = PluginIssueKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginIssueKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginIssueKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginIssue struct {
	Path string          `json:"path"`
	Kind PluginIssueKind `json:"kind"`
}

func DefaultPluginIssue() PluginIssue {
	return PluginIssue{
		Path: "",
		Kind: PluginIssueKind(""),
	}
}
func (value *PluginIssue) UnmarshalJSON(data []byte) error {
	type plain PluginIssue
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "kind"}, false); err != nil {
		return err
	}
	*value = PluginIssue(decoded)
	return nil
}
func (value PluginIssue) MarshalJSON() ([]byte, error) {
	type plain PluginIssue
	encoded := plain(value)
	return json.Marshal(encoded)
}
