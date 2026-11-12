// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
	"fmt"
	"reflect"
)

type PluginSkillsSource struct {
	Repository string  `json:"repository"`
	GitRef     *string `json:"git_ref"`
	Path       *string `json:"path"`
}

func DefaultPluginSkillsSource() PluginSkillsSource {
	return PluginSkillsSource{
		Repository: "",
		GitRef:     nil,
		Path:       nil,
	}
}
func (value *PluginSkillsSource) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsSource
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"repository"}, false); err != nil {
		return err
	}
	*value = PluginSkillsSource(decoded)
	return nil
}
func (value PluginSkillsSource) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsSource
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsResolved struct {
	Repository string `json:"repository"`
	GitRef     string `json:"git_ref"`
	Commit     string `json:"commit"`
}

func DefaultPluginSkillsResolved() PluginSkillsResolved {
	return PluginSkillsResolved{
		Repository: "",
		GitRef:     "",
		Commit:     "",
	}
}
func (value *PluginSkillsResolved) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsResolved
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"repository", "git_ref", "commit"}, false); err != nil {
		return err
	}
	*value = PluginSkillsResolved(decoded)
	return nil
}
func (value PluginSkillsResolved) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsResolved
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsCandidate struct {
	Name  string      `json:"name"`
	Path  string      `json:"path"`
	Skill PluginSkill `json:"skill"`
}

func DefaultPluginSkillsCandidate() PluginSkillsCandidate {
	return PluginSkillsCandidate{
		Name:  "",
		Path:  "",
		Skill: DefaultPluginSkill(),
	}
}
func (value *PluginSkillsCandidate) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsCandidate
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "path", "skill"}, false); err != nil {
		return err
	}
	*value = PluginSkillsCandidate(decoded)
	return nil
}
func (value PluginSkillsCandidate) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsCandidate
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsDocumentSource struct {
	Kind string
	Data any
}
type PluginSkillsDocumentSourceGitPayload struct {
	Source PluginSkillsResolved `json:"source"`
	Path   string               `json:"path"`
}

func DefaultPluginSkillsDocumentSourceGitPayload() PluginSkillsDocumentSourceGitPayload {
	return PluginSkillsDocumentSourceGitPayload{
		Source: DefaultPluginSkillsResolved(),
		Path:   "",
	}
}
func (value *PluginSkillsDocumentSourceGitPayload) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsDocumentSourceGitPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "path"}, false); err != nil {
		return err
	}
	*value = PluginSkillsDocumentSourceGitPayload(decoded)
	return nil
}
func (value PluginSkillsDocumentSourceGitPayload) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsDocumentSourceGitPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsDocumentSourceBundledPayload struct {
	Package string `json:"package"`
	Skill   string `json:"skill"`
}

func DefaultPluginSkillsDocumentSourceBundledPayload() PluginSkillsDocumentSourceBundledPayload {
	return PluginSkillsDocumentSourceBundledPayload{
		Package: "",
		Skill:   "",
	}
}
func (value *PluginSkillsDocumentSourceBundledPayload) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsDocumentSourceBundledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "skill"}, false); err != nil {
		return err
	}
	*value = PluginSkillsDocumentSourceBundledPayload(decoded)
	return nil
}
func (value PluginSkillsDocumentSourceBundledPayload) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsDocumentSourceBundledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsDocumentSourceInstalledPayload struct {
	Package PluginReference `json:"package"`
	Skill   string          `json:"skill"`
}

func DefaultPluginSkillsDocumentSourceInstalledPayload() PluginSkillsDocumentSourceInstalledPayload {
	return PluginSkillsDocumentSourceInstalledPayload{
		Package: DefaultPluginReference(),
		Skill:   "",
	}
}
func (value *PluginSkillsDocumentSourceInstalledPayload) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsDocumentSourceInstalledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "skill"}, false); err != nil {
		return err
	}
	*value = PluginSkillsDocumentSourceInstalledPayload(decoded)
	return nil
}
func (value PluginSkillsDocumentSourceInstalledPayload) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsDocumentSourceInstalledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginSkillsDocumentSourceVariants = variantFactories{"git": func() any { return new(PluginSkillsDocumentSourceGitPayload) },
	"registry":  func() any { return new(PluginSkillsRegistryResolved) },
	"bundled":   func() any { return new(PluginSkillsDocumentSourceBundledPayload) },
	"installed": func() any { return new(PluginSkillsDocumentSourceInstalledPayload) }}

func (value PluginSkillsDocumentSource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, pluginSkillsDocumentSourceVariants)
}
func (value *PluginSkillsDocumentSource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", pluginSkillsDocumentSourceVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginSkillsDocument struct {
	Skill   PluginSkill `json:"skill"`
	Content string      `json:"content"`
}

func DefaultPluginSkillsDocument() PluginSkillsDocument {
	return PluginSkillsDocument{
		Skill:   DefaultPluginSkill(),
		Content: "",
	}
}
func (value *PluginSkillsDocument) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsDocument
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"skill", "content"}, false); err != nil {
		return err
	}
	*value = PluginSkillsDocument(decoded)
	return nil
}
func (value PluginSkillsDocument) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsDocument
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsDiscovery struct {
	Source PluginSkillsResolved    `json:"source"`
	Skills []PluginSkillsCandidate `json:"skills"`
}

func DefaultPluginSkillsDiscovery() PluginSkillsDiscovery {
	return PluginSkillsDiscovery{
		Source: DefaultPluginSkillsResolved(),
		Skills: []PluginSkillsCandidate{},
	}
}
func (value *PluginSkillsDiscovery) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsDiscovery
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "skills"}, false); err != nil {
		return err
	}
	*value = PluginSkillsDiscovery(decoded)
	return nil
}
func (value PluginSkillsDiscovery) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsDiscovery
	encoded := plain(value)
	encoded.Skills = normalizeCollections(encoded.Skills)
	return json.Marshal(encoded)
}

type PluginSkillsGitProvenance struct {
	Source PluginSkillsResolved `json:"source"`
	Path   string               `json:"path"`
}

func DefaultPluginSkillsGitProvenance() PluginSkillsGitProvenance {
	return PluginSkillsGitProvenance{
		Source: DefaultPluginSkillsResolved(),
		Path:   "",
	}
}
func (value *PluginSkillsGitProvenance) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsGitProvenance
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "path"}, false); err != nil {
		return err
	}
	*value = PluginSkillsGitProvenance(decoded)
	return nil
}
func (value PluginSkillsGitProvenance) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsGitProvenance
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsRegistrySource struct {
	Namespace *string `json:"namespace"`
	Slug      string  `json:"slug"`
	Version   *string `json:"version"`
}

func DefaultPluginSkillsRegistrySource() PluginSkillsRegistrySource {
	return PluginSkillsRegistrySource{
		Namespace: nil,
		Slug:      "",
		Version:   nil,
	}
}
func (value *PluginSkillsRegistrySource) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsRegistrySource
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"slug"}, false); err != nil {
		return err
	}
	*value = PluginSkillsRegistrySource(decoded)
	return nil
}
func (value PluginSkillsRegistrySource) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsRegistrySource
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsRegistryResolved struct {
	Namespace     string `json:"namespace"`
	Slug          string `json:"slug"`
	Version       string `json:"version"`
	VersionID     string `json:"version_id"`
	ContentHash   string `json:"content_hash"`
	PackageMd5    string `json:"package_md5"`
	ArchiveSha256 string `json:"archive_sha256"`
}

func DefaultPluginSkillsRegistryResolved() PluginSkillsRegistryResolved {
	return PluginSkillsRegistryResolved{
		Namespace:     "",
		Slug:          "",
		Version:       "",
		VersionID:     "",
		ContentHash:   "",
		PackageMd5:    "",
		ArchiveSha256: "",
	}
}
func (value *PluginSkillsRegistryResolved) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsRegistryResolved
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"namespace", "slug", "version", "version_id", "content_hash", "package_md5", "archive_sha256"}, false); err != nil {
		return err
	}
	*value = PluginSkillsRegistryResolved(decoded)
	return nil
}
func (value PluginSkillsRegistryResolved) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsRegistryResolved
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsRegistryDiscovery struct {
	Source PluginSkillsRegistryResolved `json:"source"`
	Skills []PluginSkillsCandidate      `json:"skills"`
}

func DefaultPluginSkillsRegistryDiscovery() PluginSkillsRegistryDiscovery {
	return PluginSkillsRegistryDiscovery{
		Source: DefaultPluginSkillsRegistryResolved(),
		Skills: []PluginSkillsCandidate{},
	}
}
func (value *PluginSkillsRegistryDiscovery) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsRegistryDiscovery
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "skills"}, false); err != nil {
		return err
	}
	*value = PluginSkillsRegistryDiscovery(decoded)
	return nil
}
func (value PluginSkillsRegistryDiscovery) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsRegistryDiscovery
	encoded := plain(value)
	encoded.Skills = normalizeCollections(encoded.Skills)
	return json.Marshal(encoded)
}

type PluginSkillsArchiveFormat string

const (
	PluginSkillsArchiveFormatZip   PluginSkillsArchiveFormat = "zip"
	PluginSkillsArchiveFormatTar   PluginSkillsArchiveFormat = "tar"
	PluginSkillsArchiveFormatTarGz PluginSkillsArchiveFormat = "tar_gz"
)

func (value PluginSkillsArchiveFormat) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginSkillsArchiveFormat) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginSkillsArchiveFormat(decoded) {
	case PluginSkillsArchiveFormatZip, PluginSkillsArchiveFormatTar, PluginSkillsArchiveFormatTarGz:
		*value = PluginSkillsArchiveFormat(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginSkillsArchiveFormat) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginSkillsArchiveFormat) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginSkillsImported struct {
	Digest string             `json:"digest"`
	Path   string             `json:"path"`
	Kind   PluginUploadSource `json:"kind"`
}

func DefaultPluginSkillsImported() PluginSkillsImported {
	return PluginSkillsImported{
		Digest: "",
		Path:   "",
		Kind:   PluginUploadSource(""),
	}
}
func (value *PluginSkillsImported) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsImported
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"digest", "path", "kind"}, false); err != nil {
		return err
	}
	*value = PluginSkillsImported(decoded)
	return nil
}
func (value PluginSkillsImported) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsImported
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsImportDiscovery struct {
	Stream StreamID                `json:"stream"`
	Digest string                  `json:"digest"`
	Kind   PluginUploadSource      `json:"kind"`
	Skills []PluginSkillsCandidate `json:"skills"`
}

func DefaultPluginSkillsImportDiscovery() PluginSkillsImportDiscovery {
	return PluginSkillsImportDiscovery{
		Stream: StreamID{},
		Digest: "",
		Kind:   PluginUploadSource(""),
		Skills: []PluginSkillsCandidate{},
	}
}
func (value *PluginSkillsImportDiscovery) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsImportDiscovery
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "digest", "kind", "skills"}, false); err != nil {
		return err
	}
	*value = PluginSkillsImportDiscovery(decoded)
	return nil
}
func (value PluginSkillsImportDiscovery) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsImportDiscovery
	encoded := plain(value)
	encoded.Skills = normalizeCollections(encoded.Skills)
	return json.Marshal(encoded)
}

type PluginSkillsProvenance struct {
	Kind string
	Data any
}
type PluginSkillsProvenanceRegistryPayload struct {
	Registry PluginSkillsRegistryResolved `json:"registry"`
}

func DefaultPluginSkillsProvenanceRegistryPayload() PluginSkillsProvenanceRegistryPayload {
	return PluginSkillsProvenanceRegistryPayload{
		Registry: DefaultPluginSkillsRegistryResolved(),
	}
}
func (value *PluginSkillsProvenanceRegistryPayload) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsProvenanceRegistryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"registry"}, false); err != nil {
		return err
	}
	*value = PluginSkillsProvenanceRegistryPayload(decoded)
	return nil
}
func (value PluginSkillsProvenanceRegistryPayload) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsProvenanceRegistryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginSkillsProvenanceImportedPayload struct {
	Imported PluginSkillsImported `json:"imported"`
}

func DefaultPluginSkillsProvenanceImportedPayload() PluginSkillsProvenanceImportedPayload {
	return PluginSkillsProvenanceImportedPayload{
		Imported: DefaultPluginSkillsImported(),
	}
}
func (value *PluginSkillsProvenanceImportedPayload) UnmarshalJSON(data []byte) error {
	type plain PluginSkillsProvenanceImportedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"imported"}, false); err != nil {
		return err
	}
	*value = PluginSkillsProvenanceImportedPayload(decoded)
	return nil
}
func (value PluginSkillsProvenanceImportedPayload) MarshalJSON() ([]byte, error) {
	type plain PluginSkillsProvenanceImportedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginSkillsProvenanceVariants = variantFactories{"Git": func() any { return new(PluginSkillsGitProvenance) },
	"Registry": func() any { return new(PluginSkillsProvenanceRegistryPayload) },
	"Imported": func() any { return new(PluginSkillsProvenanceImportedPayload) }}

func (value PluginSkillsProvenance) MarshalJSON() ([]byte, error) {
	switch value.Kind {
	case "Git", "Registry", "Imported":
		return json.Marshal(value.Data)
	default:
		return nil, fmt.Errorf("unknown PluginSkillsProvenance variant %q", value.Kind)
	}
}
func (value *PluginSkillsProvenance) UnmarshalJSON(data []byte) error {
	{
		var decoded PluginSkillsGitProvenance
		if err := checkJSONNulls(data, reflect.TypeFor[PluginSkillsGitProvenance]()); err == nil {
			if err := json.Unmarshal(data, &decoded); err == nil {
				value.Kind = "Git"
				value.Data = &decoded
				return nil
			}
		}
	}
	{
		var decoded PluginSkillsProvenanceRegistryPayload
		if err := checkJSONNulls(data, reflect.TypeFor[PluginSkillsProvenanceRegistryPayload]()); err == nil {
			if err := json.Unmarshal(data, &decoded); err == nil {
				value.Kind = "Registry"
				value.Data = &decoded
				return nil
			}
		}
	}
	{
		var decoded PluginSkillsProvenanceImportedPayload
		if err := checkJSONNulls(data, reflect.TypeFor[PluginSkillsProvenanceImportedPayload]()); err == nil {
			if err := json.Unmarshal(data, &decoded); err == nil {
				value.Kind = "Imported"
				value.Data = &decoded
				return nil
			}
		}
	}
	return fmt.Errorf("unrecognized PluginSkillsProvenance shape")
}
