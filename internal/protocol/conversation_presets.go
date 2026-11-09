// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type ConversationPresetsCatalog struct {
	Version  uint32                             `json:"version"`
	Builtin  []ConversationPresetsPreset        `json:"builtin"`
	Partners []ConversationPresetsPreset        `json:"partners"`
	Policies []ConversationPresetsRequestPolicy `json:"policies"`
}

func DefaultConversationPresetsCatalog() ConversationPresetsCatalog {
	return ConversationPresetsCatalog{
		Version:  0,
		Builtin:  []ConversationPresetsPreset{},
		Partners: []ConversationPresetsPreset{},
		Policies: []ConversationPresetsRequestPolicy{},
	}
}
func (value *ConversationPresetsCatalog) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsCatalog
	decoded := plain{}
	decoded.Builtin = []ConversationPresetsPreset{}
	decoded.Partners = []ConversationPresetsPreset{}
	decoded.Policies = []ConversationPresetsRequestPolicy{}
	if err := DecodeObject(data, &decoded, []string{"version"}, true); err != nil {
		return err
	}
	*value = ConversationPresetsCatalog(decoded)
	return nil
}
func (value ConversationPresetsCatalog) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsCatalog
	encoded := plain(value)
	encoded.Builtin = normalizeCollections(encoded.Builtin)
	encoded.Partners = normalizeCollections(encoded.Partners)
	encoded.Policies = normalizeCollections(encoded.Policies)
	return json.Marshal(encoded)
}

type ConversationPresetsPreset struct {
	ID           string                             `json:"id"`
	Name         string                             `json:"name"`
	Locales      map[string]string                  `json:"locales"`
	Logo         *string                            `json:"logo"`
	Description  *string                            `json:"description"`
	Homepage     *string                            `json:"homepage"`
	KeyUrl       *string                            `json:"key_url"`
	Badge        *string                            `json:"badge"`
	Brand        *string                            `json:"brand"`
	BrandDisplay *PluginDesktopNavigation           `json:"brand_display"`
	Offered      bool                               `json:"offered"`
	Routes       []ConversationPresetsRoute         `json:"routes"`
	Auth         ConversationPresetsAuth            `json:"auth"`
	Models       ConversationPresetsModels          `json:"models"`
	Policies     []ConversationPresetsRequestPolicy `json:"policies"`
	Products     []ConversationPresetsProduct       `json:"products"`
	Adapter      *string                            `json:"adapter"`
}

func DefaultConversationPresetsPreset() ConversationPresetsPreset {
	return ConversationPresetsPreset{
		ID:           "",
		Name:         "",
		Locales:      map[string]string{},
		Logo:         nil,
		Description:  nil,
		Homepage:     nil,
		KeyUrl:       nil,
		Badge:        nil,
		Brand:        nil,
		BrandDisplay: nil,
		Offered:      false,
		Routes:       []ConversationPresetsRoute{},
		Auth:         ConversationPresetsAuth{Kind: "api_key", Data: &ConversationPresetsAuthApiKeyPayload{}},
		Models:       ConversationPresetsModels{Kind: "discover"},
		Policies:     []ConversationPresetsRequestPolicy{},
		Products:     []ConversationPresetsProduct{},
		Adapter:      nil,
	}
}
func (value *ConversationPresetsPreset) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsPreset
	decoded := plain{}
	decoded.Locales = map[string]string{}
	decoded.BrandDisplay = nil
	decoded.Offered = true
	decoded.Auth = ConversationPresetsAuth{Kind: "api_key", Data: &ConversationPresetsAuthApiKeyPayload{}}
	decoded.Models = ConversationPresetsModels{Kind: "discover"}
	decoded.Policies = []ConversationPresetsRequestPolicy{}
	decoded.Products = []ConversationPresetsProduct{}
	decoded.Adapter = nil
	if err := DecodeObject(data, &decoded, []string{"id", "name", "routes"}, true); err != nil {
		return err
	}
	*value = ConversationPresetsPreset(decoded)
	return nil
}
func (value ConversationPresetsPreset) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsPreset
	encoded := plain(value)
	encoded.Locales = normalizeCollections(encoded.Locales)
	encoded.Routes = normalizeCollections(encoded.Routes)
	encoded.Policies = normalizeCollections(encoded.Policies)
	encoded.Products = normalizeCollections(encoded.Products)
	return json.Marshal(encoded)
}

type ConversationPresetsProduct struct {
	Kind         ConversationConnectionProduct `json:"kind"`
	Display      *PluginDesktopNavigation      `json:"display"`
	DefaultModel *string                       `json:"default_model"`
	Discover     bool                          `json:"discover"`
}

func DefaultConversationPresetsProduct() ConversationPresetsProduct {
	return ConversationPresetsProduct{
		Kind:         ConversationConnectionProduct(""),
		Display:      nil,
		DefaultModel: nil,
		Discover:     false,
	}
}
func (value *ConversationPresetsProduct) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsProduct
	decoded := plain{}
	decoded.Display = nil
	decoded.Discover = true
	if err := DecodeObject(data, &decoded, []string{"kind"}, true); err != nil {
		return err
	}
	*value = ConversationPresetsProduct(decoded)
	return nil
}
func (value ConversationPresetsProduct) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsProduct
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationPresetsRoute struct {
	Protocol ConversationPresetsProtocol    `json:"protocol"`
	Endpoint string                         `json:"endpoint"`
	Region   *ConversationPresetsRegion     `json:"region"`
	Product  *ConversationConnectionProduct `json:"product"`
}

func DefaultConversationPresetsRoute() ConversationPresetsRoute {
	return ConversationPresetsRoute{
		Protocol: ConversationPresetsProtocol(""),
		Endpoint: "",
		Region:   nil,
		Product:  nil,
	}
}
func (value *ConversationPresetsRoute) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsRoute
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"protocol", "endpoint"}, true); err != nil {
		return err
	}
	*value = ConversationPresetsRoute(decoded)
	return nil
}
func (value ConversationPresetsRoute) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsRoute
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationPresetsRouteKey struct {
	Protocol ConversationPresetsProtocol    `json:"protocol"`
	Region   *ConversationPresetsRegion     `json:"region"`
	Product  *ConversationConnectionProduct `json:"product"`
}

func DefaultConversationPresetsRouteKey() ConversationPresetsRouteKey {
	return ConversationPresetsRouteKey{
		Protocol: ConversationPresetsProtocol(""),
		Region:   nil,
		Product:  nil,
	}
}
func (value *ConversationPresetsRouteKey) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsRouteKey
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"protocol"}, true); err != nil {
		return err
	}
	*value = ConversationPresetsRouteKey(decoded)
	return nil
}
func (value ConversationPresetsRouteKey) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsRouteKey
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationPresetsRegion string

const (
	ConversationPresetsRegionChina  ConversationPresetsRegion = "china"
	ConversationPresetsRegionGlobal ConversationPresetsRegion = "global"
)

func (value ConversationPresetsRegion) MarshalJSON() ([]byte, error) {
	switch value {
	case ConversationPresetsRegionChina, ConversationPresetsRegionGlobal:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ConversationPresetsRegion %q", value)
	}
}
func (value *ConversationPresetsRegion) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationPresetsRegion(decoded) {
	case ConversationPresetsRegionChina, ConversationPresetsRegionGlobal:
		*value = ConversationPresetsRegion(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ConversationPresetsRegion %q", decoded)
	}
}
func (value ConversationPresetsRegion) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationPresetsRegion) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationPresetsProtocol string

const (
	ConversationPresetsProtocolOpenAiResponses       ConversationPresetsProtocol = "openai-responses"
	ConversationPresetsProtocolOpenAiChatCompletions ConversationPresetsProtocol = "openai-chat-completions"
	ConversationPresetsProtocolAnthropic             ConversationPresetsProtocol = "anthropic"
	ConversationPresetsProtocolGemini                ConversationPresetsProtocol = "gemini"
	ConversationPresetsProtocolDeepSeek              ConversationPresetsProtocol = "deepseek"
	ConversationPresetsProtocolOpenCodeGo            ConversationPresetsProtocol = "opencode-go"
	ConversationPresetsProtocolOpenCodeZen           ConversationPresetsProtocol = "opencode-zen"
	ConversationPresetsProtocolAzureOpenAi           ConversationPresetsProtocol = "azure-openai"
	ConversationPresetsProtocolAzureAi               ConversationPresetsProtocol = "azure-ai"
	ConversationPresetsProtocolPlugin                ConversationPresetsProtocol = "plugin"
)

func (value ConversationPresetsProtocol) MarshalJSON() ([]byte, error) {
	switch value {
	case ConversationPresetsProtocolOpenAiResponses, ConversationPresetsProtocolOpenAiChatCompletions, ConversationPresetsProtocolAnthropic, ConversationPresetsProtocolGemini, ConversationPresetsProtocolDeepSeek, ConversationPresetsProtocolOpenCodeGo, ConversationPresetsProtocolOpenCodeZen, ConversationPresetsProtocolAzureOpenAi, ConversationPresetsProtocolAzureAi, ConversationPresetsProtocolPlugin:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ConversationPresetsProtocol %q", value)
	}
}
func (value *ConversationPresetsProtocol) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationPresetsProtocol(decoded) {
	case ConversationPresetsProtocolOpenAiResponses, ConversationPresetsProtocolOpenAiChatCompletions, ConversationPresetsProtocolAnthropic, ConversationPresetsProtocolGemini, ConversationPresetsProtocolDeepSeek, ConversationPresetsProtocolOpenCodeGo, ConversationPresetsProtocolOpenCodeZen, ConversationPresetsProtocolAzureOpenAi, ConversationPresetsProtocolAzureAi, ConversationPresetsProtocolPlugin:
		*value = ConversationPresetsProtocol(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ConversationPresetsProtocol %q", decoded)
	}
}
func (value ConversationPresetsProtocol) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationPresetsProtocol) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationPresetsAuth struct {
	Kind string
	Data any
}
type ConversationPresetsAuthApiKeyPayload struct {
	Header *string `json:"header"`
	Prefix *string `json:"prefix"`
}

func DefaultConversationPresetsAuthApiKeyPayload() ConversationPresetsAuthApiKeyPayload {
	return ConversationPresetsAuthApiKeyPayload{
		Header: nil,
		Prefix: nil,
	}
}
func (value *ConversationPresetsAuthApiKeyPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsAuthApiKeyPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = ConversationPresetsAuthApiKeyPayload(decoded)
	return nil
}
func (value ConversationPresetsAuthApiKeyPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsAuthApiKeyPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationPresetsAuthBuiltinPayload struct {
	Authentication Authentication `json:"authentication"`
}

func DefaultConversationPresetsAuthBuiltinPayload() ConversationPresetsAuthBuiltinPayload {
	return ConversationPresetsAuthBuiltinPayload{
		Authentication: Authentication(""),
	}
}
func (value *ConversationPresetsAuthBuiltinPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsAuthBuiltinPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"authentication"}, false); err != nil {
		return err
	}
	*value = ConversationPresetsAuthBuiltinPayload(decoded)
	return nil
}
func (value ConversationPresetsAuthBuiltinPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsAuthBuiltinPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationPresetsAuthPluginPayload struct {
	Flow string `json:"flow"`
}

func DefaultConversationPresetsAuthPluginPayload() ConversationPresetsAuthPluginPayload {
	return ConversationPresetsAuthPluginPayload{
		Flow: "",
	}
}
func (value *ConversationPresetsAuthPluginPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsAuthPluginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"flow"}, false); err != nil {
		return err
	}
	*value = ConversationPresetsAuthPluginPayload(decoded)
	return nil
}
func (value ConversationPresetsAuthPluginPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsAuthPluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationPresetsAuthVariants = variantFactories{"api_key": func() any { return new(ConversationPresetsAuthApiKeyPayload) },
	"none":    nil,
	"builtin": func() any { return new(ConversationPresetsAuthBuiltinPayload) },
	"plugin":  func() any { return new(ConversationPresetsAuthPluginPayload) }}

func (value ConversationPresetsAuth) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationPresetsAuthVariants)
}
func (value *ConversationPresetsAuth) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationPresetsAuthVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationPresetsModels struct {
	Kind string
	Data any
}
type ConversationPresetsModelsStaticPayload struct {
	IDs []string `json:"ids"`
}

func DefaultConversationPresetsModelsStaticPayload() ConversationPresetsModelsStaticPayload {
	return ConversationPresetsModelsStaticPayload{
		IDs: []string{},
	}
}
func (value *ConversationPresetsModelsStaticPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsModelsStaticPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"ids"}, false); err != nil {
		return err
	}
	*value = ConversationPresetsModelsStaticPayload(decoded)
	return nil
}
func (value ConversationPresetsModelsStaticPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsModelsStaticPayload
	encoded := plain(value)
	encoded.IDs = normalizeCollections(encoded.IDs)
	return json.Marshal(encoded)
}

var conversationPresetsModelsVariants = variantFactories{"discover": nil,
	"static": func() any { return new(ConversationPresetsModelsStaticPayload) }}

func (value ConversationPresetsModels) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationPresetsModelsVariants)
}
func (value *ConversationPresetsModels) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationPresetsModelsVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationPresetsRequestPolicy struct {
	Models          []string                        `json:"models"`
	ReasoningReplay *ConversationPresetsReplayField `json:"reasoning_replay"`
	Body            map[string]json.RawMessage      `json:"body"`
}

func DefaultConversationPresetsRequestPolicy() ConversationPresetsRequestPolicy {
	return ConversationPresetsRequestPolicy{
		Models:          []string{},
		ReasoningReplay: nil,
		Body:            map[string]json.RawMessage{},
	}
}
func (value *ConversationPresetsRequestPolicy) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsRequestPolicy
	decoded := plain{}
	decoded.ReasoningReplay = nil
	decoded.Body = map[string]json.RawMessage{}
	if err := DecodeObject(data, &decoded, []string{"models"}, true); err != nil {
		return err
	}
	*value = ConversationPresetsRequestPolicy(decoded)
	return nil
}
func (value ConversationPresetsRequestPolicy) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsRequestPolicy
	encoded := plain(value)
	encoded.Models = normalizeCollections(encoded.Models)
	encoded.Body = normalizeCollections(encoded.Body)
	return json.Marshal(encoded)
}

type ConversationPresetsReplayField string

const (
	ConversationPresetsReplayFieldReasoningContent ConversationPresetsReplayField = "reasoning_content"
)

func (value ConversationPresetsReplayField) MarshalJSON() ([]byte, error) {
	switch value {
	case ConversationPresetsReplayFieldReasoningContent:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ConversationPresetsReplayField %q", value)
	}
}
func (value *ConversationPresetsReplayField) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationPresetsReplayField(decoded) {
	case ConversationPresetsReplayFieldReasoningContent:
		*value = ConversationPresetsReplayField(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ConversationPresetsReplayField %q", decoded)
	}
}
func (value ConversationPresetsReplayField) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationPresetsReplayField) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationPresetsSource struct {
	Kind string
	Data any
}
type ConversationPresetsSourcePackagePayload struct {
	Name string `json:"name"`
}

func DefaultConversationPresetsSourcePackagePayload() ConversationPresetsSourcePackagePayload {
	return ConversationPresetsSourcePackagePayload{
		Name: "",
	}
}
func (value *ConversationPresetsSourcePackagePayload) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsSourcePackagePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, false); err != nil {
		return err
	}
	*value = ConversationPresetsSourcePackagePayload(decoded)
	return nil
}
func (value ConversationPresetsSourcePackagePayload) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsSourcePackagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationPresetsSourceVariants = variantFactories{"builtin": nil,
	"partner": nil,
	"package": func() any { return new(ConversationPresetsSourcePackagePayload) }}

func (value ConversationPresetsSource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationPresetsSourceVariants)
}
func (value *ConversationPresetsSource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationPresetsSourceVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationPresetsPresetRef struct {
	Source ConversationPresetsSource    `json:"source"`
	ID     string                       `json:"id"`
	Route  *ConversationPresetsRouteKey `json:"route"`
}

func DefaultConversationPresetsPresetRef() ConversationPresetsPresetRef {
	return ConversationPresetsPresetRef{
		Source: ConversationPresetsSource{},
		ID:     "",
		Route:  nil,
	}
}
func (value *ConversationPresetsPresetRef) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsPresetRef
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "id"}, true); err != nil {
		return err
	}
	*value = ConversationPresetsPresetRef(decoded)
	return nil
}
func (value ConversationPresetsPresetRef) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsPresetRef
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationPresetsBinding struct {
	Package      *PluginReference                   `json:"package"`
	Origin       *ConversationConsentOrigin         `json:"origin"`
	Auth         *PluginModelAuthBinding            `json:"auth"`
	Adapter      *PluginModelAdapterBinding         `json:"adapter"`
	Policies     []ConversationPresetsRequestPolicy `json:"policies"`
	KeyInjection *PluginModelAuthInjection          `json:"key_injection"`
}

func DefaultConversationPresetsBinding() ConversationPresetsBinding {
	return ConversationPresetsBinding{
		Package:      nil,
		Origin:       nil,
		Auth:         nil,
		Adapter:      nil,
		Policies:     []ConversationPresetsRequestPolicy{},
		KeyInjection: nil,
	}
}
func (value *ConversationPresetsBinding) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsBinding
	decoded := plain{}
	decoded.Origin = nil
	decoded.Auth = nil
	decoded.Adapter = nil
	decoded.Policies = []ConversationPresetsRequestPolicy{}
	decoded.KeyInjection = nil
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = ConversationPresetsBinding(decoded)
	return nil
}
func (value ConversationPresetsBinding) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsBinding
	encoded := plain(value)
	encoded.Policies = normalizeCollections(encoded.Policies)
	return json.Marshal(encoded)
}

type ConversationPresetsListing struct {
	Entries             []ConversationPresetsEntry `json:"entries"`
	PartnersUnavailable bool                       `json:"partners_unavailable"`
}

func DefaultConversationPresetsListing() ConversationPresetsListing {
	return ConversationPresetsListing{
		Entries:             []ConversationPresetsEntry{},
		PartnersUnavailable: false,
	}
}
func (value *ConversationPresetsListing) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsListing
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"entries", "partners_unavailable"}, false); err != nil {
		return err
	}
	*value = ConversationPresetsListing(decoded)
	return nil
}
func (value ConversationPresetsListing) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsListing
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type ConversationPresetsEntry struct {
	Source     ConversationPresetsSource     `json:"source"`
	Preset     ConversationPresetsPreset     `json:"preset"`
	Provenance ConversationPresetsProvenance `json:"provenance"`
	Package    *PluginReference              `json:"package"`
	Origin     *ConversationConsentOrigin    `json:"origin"`
	Requested  ConversationConsentScopes     `json:"requested"`
	Icon       *string                       `json:"icon,omitempty"`
	Login      *PluginModelAuthPresentation  `json:"login,omitempty"`
}

func DefaultConversationPresetsEntry() ConversationPresetsEntry {
	return ConversationPresetsEntry{
		Source:     ConversationPresetsSource{},
		Preset:     DefaultConversationPresetsPreset(),
		Provenance: ConversationPresetsProvenance(""),
		Package:    nil,
		Origin:     nil,
		Requested:  DefaultConversationConsentScopes(),
		Icon:       nil,
		Login:      nil,
	}
}
func (value *ConversationPresetsEntry) UnmarshalJSON(data []byte) error {
	type plain ConversationPresetsEntry
	decoded := plain{}
	decoded.Origin = nil
	decoded.Requested = DefaultConversationConsentScopes()
	decoded.Icon = nil
	decoded.Login = nil
	if err := DecodeObject(data, &decoded, []string{"source", "preset", "provenance"}, false); err != nil {
		return err
	}
	*value = ConversationPresetsEntry(decoded)
	return nil
}
func (value ConversationPresetsEntry) MarshalJSON() ([]byte, error) {
	type plain ConversationPresetsEntry
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationPresetsProvenance string

const (
	ConversationPresetsProvenanceOfficial   ConversationPresetsProvenance = "official"
	ConversationPresetsProvenanceBundled    ConversationPresetsProvenance = "bundled"
	ConversationPresetsProvenanceThirdParty ConversationPresetsProvenance = "third_party"
)

func (value ConversationPresetsProvenance) MarshalJSON() ([]byte, error) {
	switch value {
	case ConversationPresetsProvenanceOfficial, ConversationPresetsProvenanceBundled, ConversationPresetsProvenanceThirdParty:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ConversationPresetsProvenance %q", value)
	}
}
func (value *ConversationPresetsProvenance) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationPresetsProvenance(decoded) {
	case ConversationPresetsProvenanceOfficial, ConversationPresetsProvenanceBundled, ConversationPresetsProvenanceThirdParty:
		*value = ConversationPresetsProvenance(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ConversationPresetsProvenance %q", decoded)
	}
}
func (value ConversationPresetsProvenance) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationPresetsProvenance) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
