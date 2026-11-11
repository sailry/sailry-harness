// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PluginDesktopManifest struct {
	Entry             *string                         `json:"entry"`
	UiEntry           *string                         `json:"ui_entry"`
	UiOverlay         bool                            `json:"ui_overlay"`
	UiShared          bool                            `json:"ui_shared"`
	Resources         []string                        `json:"resources"`
	Navigation        *PluginDesktopNavigation        `json:"navigation"`
	NavigationOptions PluginDesktopNavigationOptions  `json:"navigation_options"`
	Panel             *PluginDesktopNavigation        `json:"panel"`
	Conversations     []PluginConversationDeclaration `json:"conversations"`
	Renderers         []PluginDesktopRenderer         `json:"renderers"`
	Previews          []string                        `json:"previews"`
}

func DefaultPluginDesktopManifest() PluginDesktopManifest {
	return PluginDesktopManifest{
		Entry:             nil,
		UiEntry:           nil,
		UiOverlay:         false,
		UiShared:          false,
		Resources:         []string{},
		Navigation:        nil,
		NavigationOptions: DefaultPluginDesktopNavigationOptions(),
		Panel:             nil,
		Conversations:     []PluginConversationDeclaration{},
		Renderers:         []PluginDesktopRenderer{},
		Previews:          []string{},
	}
}
func (value *PluginDesktopManifest) UnmarshalJSON(data []byte) error {
	type plain PluginDesktopManifest
	decoded := plain{}
	decoded.UiOverlay = false
	decoded.UiShared = false
	decoded.Resources = []string{}
	decoded.NavigationOptions = DefaultPluginDesktopNavigationOptions()
	decoded.Conversations = []PluginConversationDeclaration{}
	decoded.Renderers = []PluginDesktopRenderer{}
	decoded.Previews = []string{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginDesktopManifest(decoded)
	return nil
}
func (value PluginDesktopManifest) MarshalJSON() ([]byte, error) {
	type plain PluginDesktopManifest
	encoded := plain(value)
	encoded.Resources = normalizeCollections(encoded.Resources)
	encoded.Conversations = normalizeCollections(encoded.Conversations)
	encoded.Renderers = normalizeCollections(encoded.Renderers)
	encoded.Previews = normalizeCollections(encoded.Previews)
	return json.Marshal(encoded)
}

type PluginDesktopResourceKind string

const (
	PluginDesktopResourceKindTerminal  PluginDesktopResourceKind = "terminal"
	PluginDesktopResourceKindBrowser   PluginDesktopResourceKind = "browser"
	PluginDesktopResourceKindDocuments PluginDesktopResourceKind = "documents"
	PluginDesktopResourceKindGit       PluginDesktopResourceKind = "git"
)

func (value PluginDesktopResourceKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginDesktopResourceKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginDesktopResourceKind(decoded) {
	case PluginDesktopResourceKindTerminal, PluginDesktopResourceKindBrowser, PluginDesktopResourceKindDocuments, PluginDesktopResourceKindGit:
		*value = PluginDesktopResourceKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginDesktopResourceKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginDesktopResourceKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginDesktopRenderer struct {
	Resource PluginDesktopResourceKind `json:"resource"`
	Create   *string                   `json:"create"`
	Shortcut *string                   `json:"shortcut,omitempty"`
}

func DefaultPluginDesktopRenderer() PluginDesktopRenderer {
	return PluginDesktopRenderer{
		Resource: PluginDesktopResourceKind(""),
		Create:   nil,
		Shortcut: nil,
	}
}
func (value *PluginDesktopRenderer) UnmarshalJSON(data []byte) error {
	type plain PluginDesktopRenderer
	decoded := plain{}
	decoded.Shortcut = nil
	if err := DecodeObject(data, &decoded, []string{"resource"}, true); err != nil {
		return err
	}
	*value = PluginDesktopRenderer(decoded)
	return nil
}
func (value PluginDesktopRenderer) MarshalJSON() ([]byte, error) {
	type plain PluginDesktopRenderer
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginDesktopNavigationOptions struct {
	Pinned  bool                          `json:"pinned"`
	Order   uint16                        `json:"order"`
	Target  PluginDesktopNavigationTarget `json:"target"`
	Surface PluginDesktopSurface          `json:"surface"`
	Details bool                          `json:"details"`
}

func DefaultPluginDesktopNavigationOptions() PluginDesktopNavigationOptions {
	return PluginDesktopNavigationOptions{
		Pinned:  false,
		Order:   10000,
		Target:  PluginDesktopNavigationTargetNode,
		Surface: PluginDesktopSurface{Kind: "workspace"},
		Details: false,
	}
}
func (value *PluginDesktopNavigationOptions) UnmarshalJSON(data []byte) error {
	type plain PluginDesktopNavigationOptions
	decoded := plain(DefaultPluginDesktopNavigationOptions())
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginDesktopNavigationOptions(decoded)
	return nil
}
func (value PluginDesktopNavigationOptions) MarshalJSON() ([]byte, error) {
	type plain PluginDesktopNavigationOptions
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginDesktopNavigationTarget string

const (
	PluginDesktopNavigationTargetNode     PluginDesktopNavigationTarget = "node"
	PluginDesktopNavigationTargetWorktree PluginDesktopNavigationTarget = "worktree"
)

func (value PluginDesktopNavigationTarget) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginDesktopNavigationTarget) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginDesktopNavigationTarget(decoded) {
	case PluginDesktopNavigationTargetNode, PluginDesktopNavigationTargetWorktree:
		*value = PluginDesktopNavigationTarget(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginDesktopNavigationTarget) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginDesktopNavigationTarget) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginDesktopNavigation struct {
	Label   string             `json:"label"`
	Icon    *PluginDesktopIcon `json:"icon,omitempty"`
	Locales map[string]string  `json:"locales"`
}

func DefaultPluginDesktopNavigation() PluginDesktopNavigation {
	return PluginDesktopNavigation{
		Label:   "",
		Icon:    nil,
		Locales: map[string]string{},
	}
}
func (value *PluginDesktopNavigation) UnmarshalJSON(data []byte) error {
	type plain PluginDesktopNavigation
	decoded := plain{}
	decoded.Icon = nil
	decoded.Locales = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"label"}, true); err != nil {
		return err
	}
	*value = PluginDesktopNavigation(decoded)
	return nil
}
func (value PluginDesktopNavigation) MarshalJSON() ([]byte, error) {
	type plain PluginDesktopNavigation
	encoded := plain(value)
	encoded.Locales = normalizeCollections(encoded.Locales)
	return json.Marshal(encoded)
}

type PluginDesktopSettings struct {
	FullWidth      bool                           `json:"full_width"`
	EnabledControl bool                           `json:"enabled_control"`
	Placement      PluginDesktopSettingsPlacement `json:"placement"`
	Navigation     PluginDesktopNavigation        `json:"navigation"`
	Entry          *string                        `json:"entry"`
}

func DefaultPluginDesktopSettings() PluginDesktopSettings {
	return PluginDesktopSettings{
		FullWidth:      false,
		EnabledControl: false,
		Placement:      DefaultPluginDesktopSettingsPlacement(),
		Navigation:     DefaultPluginDesktopNavigation(),
		Entry:          nil,
	}
}
func (value *PluginDesktopSettings) UnmarshalJSON(data []byte) error {
	type plain PluginDesktopSettings
	decoded := plain{}
	decoded.FullWidth = false
	decoded.EnabledControl = false
	decoded.Placement = DefaultPluginDesktopSettingsPlacement()
	if err := DecodeObject(data, &decoded, []string{"navigation"}, true); err != nil {
		return err
	}
	*value = PluginDesktopSettings(decoded)
	return nil
}
func (value PluginDesktopSettings) MarshalJSON() ([]byte, error) {
	type plain PluginDesktopSettings
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginDesktopSettingsGroup string

const (
	PluginDesktopSettingsGroupApp    PluginDesktopSettingsGroup = "app"
	PluginDesktopSettingsGroupAi     PluginDesktopSettingsGroup = "ai"
	PluginDesktopSettingsGroupTools  PluginDesktopSettingsGroup = "tools"
	PluginDesktopSettingsGroupSystem PluginDesktopSettingsGroup = "system"
)

func (value PluginDesktopSettingsGroup) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginDesktopSettingsGroup) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginDesktopSettingsGroup(decoded) {
	case PluginDesktopSettingsGroupApp, PluginDesktopSettingsGroupAi, PluginDesktopSettingsGroupTools, PluginDesktopSettingsGroupSystem:
		*value = PluginDesktopSettingsGroup(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginDesktopSettingsGroup) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginDesktopSettingsGroup) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginDesktopSettingsPlacement struct {
	Group PluginDesktopSettingsGroup `json:"group"`
	Order uint16                     `json:"order"`
}

func DefaultPluginDesktopSettingsPlacement() PluginDesktopSettingsPlacement {
	return PluginDesktopSettingsPlacement{
		Group: PluginDesktopSettingsGroupTools,
		Order: 10000,
	}
}
func (value *PluginDesktopSettingsPlacement) UnmarshalJSON(data []byte) error {
	type plain PluginDesktopSettingsPlacement
	decoded := plain(DefaultPluginDesktopSettingsPlacement())
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginDesktopSettingsPlacement(decoded)
	return nil
}
func (value PluginDesktopSettingsPlacement) MarshalJSON() ([]byte, error) {
	type plain PluginDesktopSettingsPlacement
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginDesktopSurface struct {
	Kind string
	Data any
}

var pluginDesktopSurfaceVariants = variantFactories{"workspace": nil,
	"settings":    nil,
	"permissions": nil,
	"provider":    nil,
	"composer":    nil,
	"project":     nil,
	"card":        func() any { return new(PluginCardReceipt) }}

func (value PluginDesktopSurface) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, pluginDesktopSurfaceVariants)
}
func (value *PluginDesktopSurface) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", pluginDesktopSurfaceVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginDesktopBundle struct {
	Package PluginReference   `json:"package"`
	Entry   string            `json:"entry"`
	Files   map[string]string `json:"files"`
	Images  map[string]string `json:"images"`
}

func DefaultPluginDesktopBundle() PluginDesktopBundle {
	return PluginDesktopBundle{
		Package: DefaultPluginReference(),
		Entry:   "",
		Files:   map[string]string{},
		Images:  map[string]string{},
	}
}
func (value *PluginDesktopBundle) UnmarshalJSON(data []byte) error {
	type plain PluginDesktopBundle
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "entry", "files", "images"}, false); err != nil {
		return err
	}
	*value = PluginDesktopBundle(decoded)
	return nil
}
func (value PluginDesktopBundle) MarshalJSON() ([]byte, error) {
	type plain PluginDesktopBundle
	encoded := plain(value)
	encoded.Files = normalizeCollections(encoded.Files)
	encoded.Images = normalizeCollections(encoded.Images)
	return json.Marshal(encoded)
}
