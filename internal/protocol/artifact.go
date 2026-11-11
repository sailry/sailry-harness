// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ArtifactKind string

const (
	ArtifactKindDocument ArtifactKind = "document"
	ArtifactKindPreview  ArtifactKind = "preview"
)

func (value ArtifactKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ArtifactKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ArtifactKind(decoded) {
	case ArtifactKindDocument, ArtifactKindPreview:
		*value = ArtifactKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ArtifactKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ArtifactKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ArtifactSummary struct {
	ID        ArtifactID      `json:"id"`
	Kind      ArtifactKind    `json:"kind"`
	Session   SessionID       `json:"session"`
	Turn      TurnID          `json:"turn"`
	Package   PluginReference `json:"package"`
	Title     string          `json:"title"`
	Summary   string          `json:"summary"`
	Revision  uint32          `json:"revision"`
	Size      uint64          `json:"size"`
	CreatedAt int64           `json:"created_at"`
	UpdatedAt int64           `json:"updated_at"`
}

func DefaultArtifactSummary() ArtifactSummary {
	return ArtifactSummary{
		ID:        ArtifactID{},
		Kind:      ArtifactKindDocument,
		Session:   SessionID{},
		Turn:      TurnID{},
		Package:   DefaultPluginReference(),
		Title:     "",
		Summary:   "",
		Revision:  0,
		Size:      0,
		CreatedAt: 0,
		UpdatedAt: 0,
	}
}
func (value *ArtifactSummary) UnmarshalJSON(data []byte) error {
	type plain ArtifactSummary
	decoded := plain{}
	decoded.Kind = ArtifactKindDocument
	if err := DecodeObject(data, &decoded, []string{"id", "session", "turn", "package", "title", "summary", "revision", "size", "created_at", "updated_at"}, true); err != nil {
		return err
	}
	*value = ArtifactSummary(decoded)
	return nil
}
func (value ArtifactSummary) MarshalJSON() ([]byte, error) {
	type plain ArtifactSummary
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ArtifactContent struct {
	Artifact       ArtifactSummary      `json:"artifact"`
	LatestRevision uint32               `json:"latest_revision"`
	Html           string               `json:"html"`
	Warnings       []string             `json:"warnings"`
	View           *ArtifactViewContent `json:"view"`
}

func DefaultArtifactContent() ArtifactContent {
	return ArtifactContent{
		Artifact:       DefaultArtifactSummary(),
		LatestRevision: 0,
		Html:           "",
		Warnings:       []string{},
		View:           nil,
	}
}
func (value *ArtifactContent) UnmarshalJSON(data []byte) error {
	type plain ArtifactContent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"artifact", "latest_revision", "html", "warnings"}, true); err != nil {
		return err
	}
	*value = ArtifactContent(decoded)
	return nil
}
func (value ArtifactContent) MarshalJSON() ([]byte, error) {
	type plain ArtifactContent
	encoded := plain(value)
	encoded.Warnings = normalizeCollections(encoded.Warnings)
	return json.Marshal(encoded)
}

type ArtifactView struct {
	Template       string            `json:"template"`
	Styles         []string          `json:"styles"`
	Scripts        []string          `json:"scripts"`
	Libraries      []ArtifactLibrary `json:"libraries"`
	AllowedClasses []string          `json:"allowed_classes"`
}

func DefaultArtifactView() ArtifactView {
	return ArtifactView{
		Template:       "",
		Styles:         []string{},
		Scripts:        []string{},
		Libraries:      []ArtifactLibrary{},
		AllowedClasses: []string{},
	}
}
func (value *ArtifactView) UnmarshalJSON(data []byte) error {
	type plain ArtifactView
	decoded := plain{}
	decoded.Styles = []string{}
	decoded.Scripts = []string{}
	decoded.Libraries = []ArtifactLibrary{}
	decoded.AllowedClasses = []string{}
	if err := DecodeObject(data, &decoded, []string{"template"}, true); err != nil {
		return err
	}
	*value = ArtifactView(decoded)
	return nil
}
func (value ArtifactView) MarshalJSON() ([]byte, error) {
	type plain ArtifactView
	encoded := plain(value)
	encoded.Styles = normalizeCollections(encoded.Styles)
	encoded.Scripts = normalizeCollections(encoded.Scripts)
	encoded.Libraries = normalizeCollections(encoded.Libraries)
	encoded.AllowedClasses = normalizeCollections(encoded.AllowedClasses)
	return json.Marshal(encoded)
}

type ArtifactLibrary string

const (
	ArtifactLibraryMermaid ArtifactLibrary = "mermaid"
	ArtifactLibraryEcharts ArtifactLibrary = "echarts"
	ArtifactLibraryMathjax ArtifactLibrary = "mathjax"
)

func (value ArtifactLibrary) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ArtifactLibrary) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ArtifactLibrary(decoded) {
	case ArtifactLibraryMermaid, ArtifactLibraryEcharts, ArtifactLibraryMathjax:
		*value = ArtifactLibrary(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ArtifactLibrary) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ArtifactLibrary) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ArtifactViewContent struct {
	Template  string            `json:"template"`
	Styles    []string          `json:"styles"`
	Scripts   []string          `json:"scripts"`
	Libraries []ArtifactLibrary `json:"libraries"`
}

func DefaultArtifactViewContent() ArtifactViewContent {
	return ArtifactViewContent{
		Template:  "",
		Styles:    []string{},
		Scripts:   []string{},
		Libraries: []ArtifactLibrary{},
	}
}
func (value *ArtifactViewContent) UnmarshalJSON(data []byte) error {
	type plain ArtifactViewContent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"template", "styles", "scripts", "libraries"}, true); err != nil {
		return err
	}
	*value = ArtifactViewContent(decoded)
	return nil
}
func (value ArtifactViewContent) MarshalJSON() ([]byte, error) {
	type plain ArtifactViewContent
	encoded := plain(value)
	encoded.Styles = normalizeCollections(encoded.Styles)
	encoded.Scripts = normalizeCollections(encoded.Scripts)
	encoded.Libraries = normalizeCollections(encoded.Libraries)
	return json.Marshal(encoded)
}

type ArtifactRevision struct {
	ID        ArtifactID   `json:"id"`
	Kind      ArtifactKind `json:"kind"`
	Revision  uint32       `json:"revision"`
	Turn      TurnID       `json:"turn"`
	Title     string       `json:"title"`
	Summary   string       `json:"summary"`
	Size      uint64       `json:"size"`
	CreatedAt int64        `json:"created_at"`
}

func DefaultArtifactRevision() ArtifactRevision {
	return ArtifactRevision{
		ID:        ArtifactID{},
		Kind:      ArtifactKindDocument,
		Revision:  0,
		Turn:      TurnID{},
		Title:     "",
		Summary:   "",
		Size:      0,
		CreatedAt: 0,
	}
}
func (value *ArtifactRevision) UnmarshalJSON(data []byte) error {
	type plain ArtifactRevision
	decoded := plain{}
	decoded.Kind = ArtifactKindDocument
	if err := DecodeObject(data, &decoded, []string{"id", "revision", "turn", "title", "summary", "size", "created_at"}, true); err != nil {
		return err
	}
	*value = ArtifactRevision(decoded)
	return nil
}
func (value ArtifactRevision) MarshalJSON() ([]byte, error) {
	type plain ArtifactRevision
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ArtifactPage struct {
	Items     []ArtifactSummary `json:"items"`
	NextAfter *ArtifactID       `json:"next_after"`
}

func DefaultArtifactPage() ArtifactPage {
	return ArtifactPage{
		Items:     []ArtifactSummary{},
		NextAfter: nil,
	}
}
func (value *ArtifactPage) UnmarshalJSON(data []byte) error {
	type plain ArtifactPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"items"}, true); err != nil {
		return err
	}
	*value = ArtifactPage(decoded)
	return nil
}
func (value ArtifactPage) MarshalJSON() ([]byte, error) {
	type plain ArtifactPage
	encoded := plain(value)
	encoded.Items = normalizeCollections(encoded.Items)
	return json.Marshal(encoded)
}

type ArtifactRevisionPage struct {
	Items      []ArtifactRevision `json:"items"`
	NextBefore *uint32            `json:"next_before"`
}

func DefaultArtifactRevisionPage() ArtifactRevisionPage {
	return ArtifactRevisionPage{
		Items:      []ArtifactRevision{},
		NextBefore: nil,
	}
}
func (value *ArtifactRevisionPage) UnmarshalJSON(data []byte) error {
	type plain ArtifactRevisionPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"items"}, true); err != nil {
		return err
	}
	*value = ArtifactRevisionPage(decoded)
	return nil
}
func (value ArtifactRevisionPage) MarshalJSON() ([]byte, error) {
	type plain ArtifactRevisionPage
	encoded := plain(value)
	encoded.Items = normalizeCollections(encoded.Items)
	return json.Marshal(encoded)
}

type ArtifactWrite struct {
	ID               *ArtifactID  `json:"id"`
	Kind             ArtifactKind `json:"kind"`
	ExpectedRevision *uint32      `json:"expected_revision"`
	Title            string       `json:"title"`
	Summary          string       `json:"summary"`
	Html             string       `json:"html"`
}

func DefaultArtifactWrite() ArtifactWrite {
	return ArtifactWrite{
		ID:               nil,
		Kind:             ArtifactKindDocument,
		ExpectedRevision: nil,
		Title:            "",
		Summary:          "",
		Html:             "",
	}
}
func (value *ArtifactWrite) UnmarshalJSON(data []byte) error {
	type plain ArtifactWrite
	decoded := plain{}
	decoded.Kind = ArtifactKindDocument
	decoded.Summary = ""
	if err := DecodeObject(data, &decoded, []string{"title", "html"}, true); err != nil {
		return err
	}
	*value = ArtifactWrite(decoded)
	return nil
}
func (value ArtifactWrite) MarshalJSON() ([]byte, error) {
	type plain ArtifactWrite
	encoded := plain(value)
	return json.Marshal(encoded)
}
