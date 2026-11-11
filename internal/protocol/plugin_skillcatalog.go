// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PluginSkillcatalogSource string

const (
	PluginSkillcatalogSourceOfficial PluginSkillcatalogSource = "official"
	PluginSkillcatalogSourceSkillsSh PluginSkillcatalogSource = "skills_sh"
	PluginSkillcatalogSourceSkillhub PluginSkillcatalogSource = "skillhub"
)

func (value PluginSkillcatalogSource) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginSkillcatalogSource) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginSkillcatalogSource(decoded) {
	case PluginSkillcatalogSourceOfficial, PluginSkillcatalogSourceSkillsSh, PluginSkillcatalogSourceSkillhub:
		*value = PluginSkillcatalogSource(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginSkillcatalogSource) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginSkillcatalogSource) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginSkillcatalogInstallSource struct {
	Kind string
	Data any
}

var pluginSkillcatalogInstallSourceVariants = variantFactories{"git": func() any { return new(PluginSkillsSource) },
	"registry": func() any { return new(PluginSkillsRegistrySource) }}

func (value PluginSkillcatalogInstallSource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, pluginSkillcatalogInstallSourceVariants)
}
func (value *PluginSkillcatalogInstallSource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", pluginSkillcatalogInstallSourceVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginSkillcatalogEntry struct {
	ID                 string                           `json:"id"`
	Package            *string                          `json:"package"`
	Name               string                           `json:"name"`
	DisplayName        *string                          `json:"display_name"`
	Icon               *string                          `json:"icon"`
	DisplayLocales     map[string]string                `json:"display_locales"`
	Description        *string                          `json:"description"`
	DescriptionLocales map[string]string                `json:"description_locales"`
	Origin             string                           `json:"origin"`
	Url                *string                          `json:"url"`
	Source             *PluginSkillcatalogInstallSource `json:"source"`
	Bundled            bool                             `json:"bundled"`
}

func DefaultPluginSkillcatalogEntry() PluginSkillcatalogEntry {
	return PluginSkillcatalogEntry{
		ID:                 "",
		Package:            nil,
		Name:               "",
		DisplayName:        nil,
		Icon:               nil,
		DisplayLocales:     map[string]string{},
		Description:        nil,
		DescriptionLocales: map[string]string{},
		Origin:             "",
		Url:                nil,
		Source:             nil,
		Bundled:            false,
	}
}
func (value *PluginSkillcatalogEntry) UnmarshalJSON(data []byte) error {
	type plain PluginSkillcatalogEntry
	decoded := plain{}
	decoded.Icon = nil
	if err := DecodeObject(data, &decoded, []string{"id", "name", "display_locales", "description_locales", "origin", "bundled"}, true); err != nil {
		return err
	}
	*value = PluginSkillcatalogEntry(decoded)
	return nil
}
func (value PluginSkillcatalogEntry) MarshalJSON() ([]byte, error) {
	type plain PluginSkillcatalogEntry
	encoded := plain(value)
	encoded.DisplayLocales = normalizeCollections(encoded.DisplayLocales)
	encoded.DescriptionLocales = normalizeCollections(encoded.DescriptionLocales)
	return json.Marshal(encoded)
}

type PluginSkillcatalogQueryScope string

const (
	PluginSkillcatalogQueryScopeCatalog PluginSkillcatalogQueryScope = "catalog"
	PluginSkillcatalogQueryScopePage    PluginSkillcatalogQueryScope = "page"
)

func (value PluginSkillcatalogQueryScope) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginSkillcatalogQueryScope) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginSkillcatalogQueryScope(decoded) {
	case PluginSkillcatalogQueryScopeCatalog, PluginSkillcatalogQueryScopePage:
		*value = PluginSkillcatalogQueryScope(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginSkillcatalogQueryScope) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginSkillcatalogQueryScope) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginSkillcatalogPage struct {
	QueryScope  PluginSkillcatalogQueryScope `json:"query_scope"`
	Entries     []PluginSkillcatalogEntry    `json:"entries"`
	Page        uint32                       `json:"page"`
	Next        *uint32                      `json:"next"`
	Total       *uint64                      `json:"total"`
	Unavailable bool                         `json:"unavailable"`
}

func DefaultPluginSkillcatalogPage() PluginSkillcatalogPage {
	return PluginSkillcatalogPage{
		QueryScope:  PluginSkillcatalogQueryScope(""),
		Entries:     []PluginSkillcatalogEntry{},
		Page:        0,
		Next:        nil,
		Total:       nil,
		Unavailable: false,
	}
}
func (value *PluginSkillcatalogPage) UnmarshalJSON(data []byte) error {
	type plain PluginSkillcatalogPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"query_scope", "entries", "page", "unavailable"}, false); err != nil {
		return err
	}
	*value = PluginSkillcatalogPage(decoded)
	return nil
}
func (value PluginSkillcatalogPage) MarshalJSON() ([]byte, error) {
	type plain PluginSkillcatalogPage
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}
