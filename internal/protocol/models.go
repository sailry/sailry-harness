// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ModelsHub string

const (
	ModelsHubHuggingFace ModelsHub = "hugging_face"
	ModelsHubModelScope  ModelsHub = "model_scope"
)

func (value ModelsHub) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ModelsHub) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ModelsHub(decoded) {
	case ModelsHubHuggingFace, ModelsHubModelScope:
		*value = ModelsHub(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ModelsHub) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ModelsHub) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ModelsKind string

const (
	ModelsKindEmbedding ModelsKind = "embedding"
	ModelsKindSpeech    ModelsKind = "speech"
	ModelsKindOcr       ModelsKind = "ocr"
	ModelsKindDecision  ModelsKind = "decision"
)

func (value ModelsKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ModelsKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ModelsKind(decoded) {
	case ModelsKindEmbedding, ModelsKindSpeech, ModelsKindOcr, ModelsKindDecision:
		*value = ModelsKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ModelsKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ModelsKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ModelsPaddleFamily string

const (
	ModelsPaddleFamilyV5 ModelsPaddleFamily = "v5"
	ModelsPaddleFamilyV6 ModelsPaddleFamily = "v6"
)

func (value ModelsPaddleFamily) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ModelsPaddleFamily) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ModelsPaddleFamily(decoded) {
	case ModelsPaddleFamilyV5, ModelsPaddleFamilyV6:
		*value = ModelsPaddleFamily(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ModelsPaddleFamily) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ModelsPaddleFamily) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ModelsPooling string

const (
	ModelsPoolingCls  ModelsPooling = "cls"
	ModelsPoolingMean ModelsPooling = "mean"
)

func (value ModelsPooling) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ModelsPooling) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ModelsPooling(decoded) {
	case ModelsPoolingCls, ModelsPoolingMean:
		*value = ModelsPooling(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ModelsPooling) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ModelsPooling) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ModelsFormat struct {
	Kind string
	Data any
}
type ModelsFormatTransformerPayload struct {
	Dimension  uint16        `json:"dimension"`
	Pooling    ModelsPooling `json:"pooling"`
	TokenTypes bool          `json:"token_types"`
}

func DefaultModelsFormatTransformerPayload() ModelsFormatTransformerPayload {
	return ModelsFormatTransformerPayload{
		Dimension:  0,
		Pooling:    ModelsPooling(""),
		TokenTypes: false,
	}
}
func (value *ModelsFormatTransformerPayload) UnmarshalJSON(data []byte) error {
	type plain ModelsFormatTransformerPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"dimension", "pooling", "token_types"}, true); err != nil {
		return err
	}
	*value = ModelsFormatTransformerPayload(decoded)
	return nil
}
func (value ModelsFormatTransformerPayload) MarshalJSON() ([]byte, error) {
	type plain ModelsFormatTransformerPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ModelsFormatStaticPayload struct {
	Dimension uint16 `json:"dimension"`
}

func DefaultModelsFormatStaticPayload() ModelsFormatStaticPayload {
	return ModelsFormatStaticPayload{
		Dimension: 0,
	}
}
func (value *ModelsFormatStaticPayload) UnmarshalJSON(data []byte) error {
	type plain ModelsFormatStaticPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"dimension"}, true); err != nil {
		return err
	}
	*value = ModelsFormatStaticPayload(decoded)
	return nil
}
func (value ModelsFormatStaticPayload) MarshalJSON() ([]byte, error) {
	type plain ModelsFormatStaticPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ModelsFormatSenseVoicePayload struct {
}

func DefaultModelsFormatSenseVoicePayload() ModelsFormatSenseVoicePayload {
	return ModelsFormatSenseVoicePayload{}
}
func (value *ModelsFormatSenseVoicePayload) UnmarshalJSON(data []byte) error {
	type plain ModelsFormatSenseVoicePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ModelsFormatSenseVoicePayload(decoded)
	return nil
}
func (value ModelsFormatSenseVoicePayload) MarshalJSON() ([]byte, error) {
	type plain ModelsFormatSenseVoicePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ModelsFormatDecisionPayload struct {
}

func DefaultModelsFormatDecisionPayload() ModelsFormatDecisionPayload {
	return ModelsFormatDecisionPayload{}
}
func (value *ModelsFormatDecisionPayload) UnmarshalJSON(data []byte) error {
	type plain ModelsFormatDecisionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ModelsFormatDecisionPayload(decoded)
	return nil
}
func (value ModelsFormatDecisionPayload) MarshalJSON() ([]byte, error) {
	type plain ModelsFormatDecisionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ModelsFormatPaddleOcrPayload struct {
	Family      ModelsPaddleFamily `json:"family"`
	Orientation bool               `json:"orientation"`
}

func DefaultModelsFormatPaddleOcrPayload() ModelsFormatPaddleOcrPayload {
	return ModelsFormatPaddleOcrPayload{
		Family:      ModelsPaddleFamily(""),
		Orientation: false,
	}
}
func (value *ModelsFormatPaddleOcrPayload) UnmarshalJSON(data []byte) error {
	type plain ModelsFormatPaddleOcrPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"family", "orientation"}, true); err != nil {
		return err
	}
	*value = ModelsFormatPaddleOcrPayload(decoded)
	return nil
}
func (value ModelsFormatPaddleOcrPayload) MarshalJSON() ([]byte, error) {
	type plain ModelsFormatPaddleOcrPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var modelsFormatVariants = variantFactories{"transformer": func() any { return new(ModelsFormatTransformerPayload) },
	"static":      func() any { return new(ModelsFormatStaticPayload) },
	"sense_voice": func() any { return new(ModelsFormatSenseVoicePayload) },
	"decision":    func() any { return new(ModelsFormatDecisionPayload) },
	"paddle_ocr":  func() any { return new(ModelsFormatPaddleOcrPayload) }}

func (value ModelsFormat) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, modelsFormatVariants)
}
func (value *ModelsFormat) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", modelsFormatVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ModelsSource struct {
	Hub        ModelsHub `json:"hub"`
	Repository string    `json:"repository"`
	Revision   string    `json:"revision"`
}

func DefaultModelsSource() ModelsSource {
	return ModelsSource{
		Hub:        ModelsHubHuggingFace,
		Repository: "",
		Revision:   "",
	}
}
func (value *ModelsSource) UnmarshalJSON(data []byte) error {
	type plain ModelsSource
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"hub", "repository", "revision"}, true); err != nil {
		return err
	}
	*value = ModelsSource(decoded)
	return nil
}
func (value ModelsSource) MarshalJSON() ([]byte, error) {
	type plain ModelsSource
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ModelsAsset struct {
	Source ModelsSource `json:"source"`
	Path   string       `json:"path"`
	Name   string       `json:"name"`
	Bytes  uint64       `json:"bytes"`
	Sha256 string       `json:"sha256"`
}

func DefaultModelsAsset() ModelsAsset {
	return ModelsAsset{
		Source: DefaultModelsSource(),
		Path:   "",
		Name:   "",
		Bytes:  0,
		Sha256: "",
	}
}
func (value *ModelsAsset) UnmarshalJSON(data []byte) error {
	type plain ModelsAsset
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "path", "name", "bytes", "sha256"}, true); err != nil {
		return err
	}
	*value = ModelsAsset(decoded)
	return nil
}
func (value ModelsAsset) MarshalJSON() ([]byte, error) {
	type plain ModelsAsset
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ModelsDefinition struct {
	Source         ModelsSource  `json:"source"`
	Name           string        `json:"name"`
	License        string        `json:"license"`
	Format         ModelsFormat  `json:"format"`
	QueryPrefix    *string       `json:"query_prefix,omitempty"`
	DocumentPrefix *string       `json:"document_prefix,omitempty"`
	Assets         []ModelsAsset `json:"assets"`
}

func DefaultModelsDefinition() ModelsDefinition {
	return ModelsDefinition{
		Source:         DefaultModelsSource(),
		Name:           "",
		License:        "",
		Format:         ModelsFormat{},
		QueryPrefix:    nil,
		DocumentPrefix: nil,
		Assets:         []ModelsAsset{},
	}
}
func (value *ModelsDefinition) UnmarshalJSON(data []byte) error {
	type plain ModelsDefinition
	decoded := plain{}
	decoded.QueryPrefix = nil
	decoded.DocumentPrefix = nil
	if err := DecodeObject(data, &decoded, []string{"source", "name", "license", "format", "assets"}, true); err != nil {
		return err
	}
	*value = ModelsDefinition(decoded)
	return nil
}
func (value ModelsDefinition) MarshalJSON() ([]byte, error) {
	type plain ModelsDefinition
	encoded := plain(value)
	encoded.Assets = normalizeCollections(encoded.Assets)
	return json.Marshal(encoded)
}

type ModelsSearch struct {
	Hub    ModelsHub  `json:"hub"`
	Kind   ModelsKind `json:"kind"`
	Query  string     `json:"query"`
	Cursor *string    `json:"cursor"`
}

func DefaultModelsSearch() ModelsSearch {
	return ModelsSearch{
		Hub:    ModelsHubHuggingFace,
		Kind:   ModelsKind(""),
		Query:  "",
		Cursor: nil,
	}
}
func (value *ModelsSearch) UnmarshalJSON(data []byte) error {
	type plain ModelsSearch
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"hub", "kind", "query"}, true); err != nil {
		return err
	}
	*value = ModelsSearch(decoded)
	return nil
}
func (value ModelsSearch) MarshalJSON() ([]byte, error) {
	type plain ModelsSearch
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ModelsItem struct {
	Source ModelsSource `json:"source"`
	Name   string       `json:"name"`
	Format ModelsFormat `json:"format"`
}

func DefaultModelsItem() ModelsItem {
	return ModelsItem{
		Source: DefaultModelsSource(),
		Name:   "",
		Format: ModelsFormat{},
	}
}
func (value *ModelsItem) UnmarshalJSON(data []byte) error {
	type plain ModelsItem
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "name", "format"}, false); err != nil {
		return err
	}
	*value = ModelsItem(decoded)
	return nil
}
func (value ModelsItem) MarshalJSON() ([]byte, error) {
	type plain ModelsItem
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ModelsPage struct {
	Items      []ModelsItem `json:"items"`
	NextCursor *string      `json:"next_cursor"`
	Partial    bool         `json:"partial"`
}

func DefaultModelsPage() ModelsPage {
	return ModelsPage{
		Items:      []ModelsItem{},
		NextCursor: nil,
		Partial:    false,
	}
}
func (value *ModelsPage) UnmarshalJSON(data []byte) error {
	type plain ModelsPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"items", "partial"}, false); err != nil {
		return err
	}
	*value = ModelsPage(decoded)
	return nil
}
func (value ModelsPage) MarshalJSON() ([]byte, error) {
	type plain ModelsPage
	encoded := plain(value)
	encoded.Items = normalizeCollections(encoded.Items)
	return json.Marshal(encoded)
}

type ModelsVersion struct {
	Name     string `json:"name"`
	Revision string `json:"revision"`
}

func DefaultModelsVersion() ModelsVersion {
	return ModelsVersion{
		Name:     "",
		Revision: "",
	}
}
func (value *ModelsVersion) UnmarshalJSON(data []byte) error {
	type plain ModelsVersion
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "revision"}, false); err != nil {
		return err
	}
	*value = ModelsVersion(decoded)
	return nil
}
func (value ModelsVersion) MarshalJSON() ([]byte, error) {
	type plain ModelsVersion
	encoded := plain(value)
	return json.Marshal(encoded)
}
