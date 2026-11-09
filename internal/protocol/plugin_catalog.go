// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type PluginCatalogSource string

const (
	PluginCatalogSourceOfficial   PluginCatalogSource = "official"
	PluginCatalogSourceThirdParty PluginCatalogSource = "third_party"
)

func (value PluginCatalogSource) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginCatalogSourceOfficial, PluginCatalogSourceThirdParty:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginCatalogSource %q", value)
	}
}
func (value *PluginCatalogSource) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginCatalogSource(decoded) {
	case PluginCatalogSourceOfficial, PluginCatalogSourceThirdParty:
		*value = PluginCatalogSource(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginCatalogSource %q", decoded)
	}
}
func (value PluginCatalogSource) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginCatalogSource) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginCatalogSelection struct {
	Source PluginSkillsResolved `json:"source"`
	Path   string               `json:"path"`
	Info   PluginInfo           `json:"info"`
}

func DefaultPluginCatalogSelection() PluginCatalogSelection {
	return PluginCatalogSelection{
		Source: DefaultPluginSkillsResolved(),
		Path:   "",
		Info:   DefaultPluginInfo(),
	}
}
func (value *PluginCatalogSelection) UnmarshalJSON(data []byte) error {
	type plain PluginCatalogSelection
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "path", "info"}, false); err != nil {
		return err
	}
	*value = PluginCatalogSelection(decoded)
	return nil
}
func (value PluginCatalogSelection) MarshalJSON() ([]byte, error) {
	type plain PluginCatalogSelection
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginCatalogDetails struct {
	Kind string
	Data any
}

var pluginCatalogDetailsVariants = variantFactories{"plugin": func() any { return new(PluginInfo) },
	"skills":      func() any { return new(PluginCatalogSkillCollection) },
	"unsupported": func() any { return new(PluginCatalogExternal) }}

func (value PluginCatalogDetails) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, pluginCatalogDetailsVariants)
}
func (value *PluginCatalogDetails) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", pluginCatalogDetailsVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginCatalogMetadata struct {
	Name        string  `json:"name"`
	Version     *string `json:"version"`
	Description *string `json:"description"`
	Homepage    *string `json:"homepage"`
	License     *string `json:"license"`
	Author      *string `json:"author"`
}

func DefaultPluginCatalogMetadata() PluginCatalogMetadata {
	return PluginCatalogMetadata{
		Name:        "",
		Version:     nil,
		Description: nil,
		Homepage:    nil,
		License:     nil,
		Author:      nil,
	}
}
func (value *PluginCatalogMetadata) UnmarshalJSON(data []byte) error {
	type plain PluginCatalogMetadata
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, false); err != nil {
		return err
	}
	*value = PluginCatalogMetadata(decoded)
	return nil
}
func (value PluginCatalogMetadata) MarshalJSON() ([]byte, error) {
	type plain PluginCatalogMetadata
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginCatalogFormat string

const (
	PluginCatalogFormatCodex      PluginCatalogFormat = "codex"
	PluginCatalogFormatClaudeCode PluginCatalogFormat = "claude_code"
)

func (value PluginCatalogFormat) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginCatalogFormatCodex, PluginCatalogFormatClaudeCode:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginCatalogFormat %q", value)
	}
}
func (value *PluginCatalogFormat) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginCatalogFormat(decoded) {
	case PluginCatalogFormatCodex, PluginCatalogFormatClaudeCode:
		*value = PluginCatalogFormat(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginCatalogFormat %q", decoded)
	}
}
func (value PluginCatalogFormat) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginCatalogFormat) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginCatalogSkillCollection struct {
	Source      PluginSkillsResolved    `json:"source"`
	Metadata    PluginCatalogMetadata   `json:"metadata"`
	Skills      []PluginSkillsCandidate `json:"skills"`
	Unsupported []PluginCatalogFormat   `json:"unsupported"`
}

func DefaultPluginCatalogSkillCollection() PluginCatalogSkillCollection {
	return PluginCatalogSkillCollection{
		Source:      DefaultPluginSkillsResolved(),
		Metadata:    DefaultPluginCatalogMetadata(),
		Skills:      []PluginSkillsCandidate{},
		Unsupported: []PluginCatalogFormat{},
	}
}
func (value *PluginCatalogSkillCollection) UnmarshalJSON(data []byte) error {
	type plain PluginCatalogSkillCollection
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "metadata", "skills", "unsupported"}, false); err != nil {
		return err
	}
	*value = PluginCatalogSkillCollection(decoded)
	return nil
}
func (value PluginCatalogSkillCollection) MarshalJSON() ([]byte, error) {
	type plain PluginCatalogSkillCollection
	encoded := plain(value)
	encoded.Skills = normalizeCollections(encoded.Skills)
	encoded.Unsupported = normalizeCollections(encoded.Unsupported)
	return json.Marshal(encoded)
}

type PluginCatalogExternal struct {
	Source   PluginSkillsResolved  `json:"source"`
	Metadata PluginCatalogMetadata `json:"metadata"`
	Formats  []PluginCatalogFormat `json:"formats"`
}

func DefaultPluginCatalogExternal() PluginCatalogExternal {
	return PluginCatalogExternal{
		Source:   DefaultPluginSkillsResolved(),
		Metadata: DefaultPluginCatalogMetadata(),
		Formats:  []PluginCatalogFormat{},
	}
}
func (value *PluginCatalogExternal) UnmarshalJSON(data []byte) error {
	type plain PluginCatalogExternal
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "metadata", "formats"}, false); err != nil {
		return err
	}
	*value = PluginCatalogExternal(decoded)
	return nil
}
func (value PluginCatalogExternal) MarshalJSON() ([]byte, error) {
	type plain PluginCatalogExternal
	encoded := plain(value)
	encoded.Formats = normalizeCollections(encoded.Formats)
	return json.Marshal(encoded)
}

type PluginCatalogEntry struct {
	Compatibility      PluginCompatibility        `json:"compatibility"`
	Commit             *string                    `json:"commit,omitempty"`
	Versions           []PluginCatalogVersion     `json:"versions,omitempty"`
	Category           *PluginCategory            `json:"category,omitempty"`
	Tint               *PluginTint                `json:"tint,omitempty"`
	ReleaseNotes       *PluginCatalogReleaseNotes `json:"release_notes,omitempty"`
	ID                 string                     `json:"id"`
	Name               string                     `json:"name"`
	Version            *string                    `json:"version"`
	Description        *string                    `json:"description"`
	DescriptionLocales map[string]string          `json:"description_locales,omitempty"`
	Display            *PluginDesktopNavigation   `json:"display"`
	Icon               *string                    `json:"icon"`
	Repository         *string                    `json:"repository"`
	Bundled            bool                       `json:"bundled"`
	ProvidesModels     bool                       `json:"provides_models"`
	Permissions        []PluginAction             `json:"permissions"`
}

func DefaultPluginCatalogEntry() PluginCatalogEntry {
	return PluginCatalogEntry{
		Compatibility:      PluginCompatibility{Kind: "compatible"},
		Commit:             nil,
		Versions:           []PluginCatalogVersion{},
		Category:           nil,
		Tint:               nil,
		ReleaseNotes:       nil,
		ID:                 "",
		Name:               "",
		Version:            nil,
		Description:        nil,
		DescriptionLocales: map[string]string{},
		Display:            nil,
		Icon:               nil,
		Repository:         nil,
		Bundled:            false,
		ProvidesModels:     false,
		Permissions:        []PluginAction{},
	}
}
func (value *PluginCatalogEntry) UnmarshalJSON(data []byte) error {
	type plain PluginCatalogEntry
	decoded := plain{}
	decoded.Compatibility = PluginCompatibility{Kind: "compatible"}
	decoded.Commit = nil
	decoded.Versions = []PluginCatalogVersion{}
	decoded.Category = nil
	decoded.Tint = nil
	decoded.ReleaseNotes = nil
	decoded.DescriptionLocales = map[string]string{}
	decoded.ProvidesModels = false
	decoded.Permissions = []PluginAction{}
	if err := DecodeObject(data, &decoded, []string{"id", "name", "bundled"}, false); err != nil {
		return err
	}
	*value = PluginCatalogEntry(decoded)
	return nil
}
func (value PluginCatalogEntry) MarshalJSON() ([]byte, error) {
	type plain PluginCatalogEntry
	encoded := plain(value)
	encoded.Versions = normalizeCollections(encoded.Versions)
	encoded.DescriptionLocales = normalizeCollections(encoded.DescriptionLocales)
	encoded.Permissions = normalizeCollections(encoded.Permissions)
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

type PluginCatalogPage struct {
	Entries     []PluginCatalogEntry `json:"entries"`
	Page        uint32               `json:"page"`
	Pages       uint32               `json:"pages"`
	Unavailable bool                 `json:"unavailable"`
}

func DefaultPluginCatalogPage() PluginCatalogPage {
	return PluginCatalogPage{
		Entries:     []PluginCatalogEntry{},
		Page:        0,
		Pages:       0,
		Unavailable: false,
	}
}
func (value *PluginCatalogPage) UnmarshalJSON(data []byte) error {
	type plain PluginCatalogPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"entries", "page", "pages", "unavailable"}, false); err != nil {
		return err
	}
	*value = PluginCatalogPage(decoded)
	return nil
}
func (value PluginCatalogPage) MarshalJSON() ([]byte, error) {
	type plain PluginCatalogPage
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type PluginCatalogReleaseNotes struct {
	Text    string            `json:"text"`
	Locales map[string]string `json:"locales,omitempty"`
}

func DefaultPluginCatalogReleaseNotes() PluginCatalogReleaseNotes {
	return PluginCatalogReleaseNotes{
		Text:    "",
		Locales: map[string]string{},
	}
}
func (value *PluginCatalogReleaseNotes) UnmarshalJSON(data []byte) error {
	type plain PluginCatalogReleaseNotes
	decoded := plain{}
	decoded.Locales = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"text"}, true); err != nil {
		return err
	}
	*value = PluginCatalogReleaseNotes(decoded)
	return nil
}
func (value PluginCatalogReleaseNotes) MarshalJSON() ([]byte, error) {
	type plain PluginCatalogReleaseNotes
	encoded := plain(value)
	encoded.Locales = normalizeCollections(encoded.Locales)
	return json.Marshal(encoded)
}

type PluginCatalogVersion struct {
	Version          string                     `json:"version"`
	Commit           string                     `json:"commit"`
	MinSailryVersion *string                    `json:"min_sailry_version,omitempty"`
	ReleaseNotes     *PluginCatalogReleaseNotes `json:"release_notes,omitempty"`
	Compatibility    PluginCompatibility        `json:"compatibility"`
}

func DefaultPluginCatalogVersion() PluginCatalogVersion {
	return PluginCatalogVersion{
		Version:          "",
		Commit:           "",
		MinSailryVersion: nil,
		ReleaseNotes:     nil,
		Compatibility:    PluginCompatibility{Kind: "compatible"},
	}
}
func (value *PluginCatalogVersion) UnmarshalJSON(data []byte) error {
	type plain PluginCatalogVersion
	decoded := plain{}
	decoded.MinSailryVersion = nil
	decoded.ReleaseNotes = nil
	decoded.Compatibility = PluginCompatibility{Kind: "compatible"}
	if err := DecodeObject(data, &decoded, []string{"version", "commit"}, true); err != nil {
		return err
	}
	*value = PluginCatalogVersion(decoded)
	return nil
}
func (value PluginCatalogVersion) MarshalJSON() ([]byte, error) {
	type plain PluginCatalogVersion
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
