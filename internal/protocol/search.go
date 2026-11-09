// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type SearchDriver string

const (
	SearchDriverExa        SearchDriver = "exa"
	SearchDriverTinyfish   SearchDriver = "tinyfish"
	SearchDriverTavily     SearchDriver = "tavily"
	SearchDriverBrave      SearchDriver = "brave"
	SearchDriverSearxng    SearchDriver = "searxng"
	SearchDriverBocha      SearchDriver = "bocha"
	SearchDriverVolcengine SearchDriver = "volcengine"
	SearchDriverDeepseek   SearchDriver = "deepseek"
	SearchDriverModel      SearchDriver = "model"
)

func (value SearchDriver) MarshalJSON() ([]byte, error) {
	switch value {
	case SearchDriverExa, SearchDriverTinyfish, SearchDriverTavily, SearchDriverBrave, SearchDriverSearxng, SearchDriverBocha, SearchDriverVolcengine, SearchDriverDeepseek, SearchDriverModel:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown SearchDriver %q", value)
	}
}
func (value *SearchDriver) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch SearchDriver(decoded) {
	case SearchDriverExa, SearchDriverTinyfish, SearchDriverTavily, SearchDriverBrave, SearchDriverSearxng, SearchDriverBocha, SearchDriverVolcengine, SearchDriverDeepseek, SearchDriverModel:
		*value = SearchDriver(decoded)
		return nil
	default:
		return fmt.Errorf("unknown SearchDriver %q", decoded)
	}
}
func (value SearchDriver) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *SearchDriver) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type SearchQuota string

const (
	SearchQuotaKeyless        SearchQuota = "keyless"
	SearchQuotaAnonymousQuota SearchQuota = "anonymous_quota"
	SearchQuotaFreeQuota      SearchQuota = "free_quota"
	SearchQuotaPaid           SearchQuota = "paid"
)

func (value SearchQuota) MarshalJSON() ([]byte, error) {
	switch value {
	case SearchQuotaKeyless, SearchQuotaAnonymousQuota, SearchQuotaFreeQuota, SearchQuotaPaid:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown SearchQuota %q", value)
	}
}
func (value *SearchQuota) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch SearchQuota(decoded) {
	case SearchQuotaKeyless, SearchQuotaAnonymousQuota, SearchQuotaFreeQuota, SearchQuotaPaid:
		*value = SearchQuota(decoded)
		return nil
	default:
		return fmt.Errorf("unknown SearchQuota %q", decoded)
	}
}
func (value SearchQuota) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *SearchQuota) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type SearchCapability struct {
	KeyRequired bool        `json:"key_required"`
	Quota       SearchQuota `json:"quota"`
}

func DefaultSearchCapability() SearchCapability {
	return SearchCapability{
		KeyRequired: false,
		Quota:       SearchQuota(""),
	}
}
func (value *SearchCapability) UnmarshalJSON(data []byte) error {
	type plain SearchCapability
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key_required", "quota"}, true); err != nil {
		return err
	}
	*value = SearchCapability(decoded)
	return nil
}
func (value SearchCapability) MarshalJSON() ([]byte, error) {
	type plain SearchCapability
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SearchSelection struct {
	Kind string
	Data any
}

var searchSelectionVariants = variantFactories{"auto": nil,
	"driver": func() any { return new(SearchDriver) }}

func (value SearchSelection) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "driver", value.Kind, value.Data, searchSelectionVariants)
}
func (value *SearchSelection) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "driver", searchSelectionVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type SearchConfig struct {
	Enabled         bool               `json:"enabled"`
	Endpoint        *string            `json:"endpoint"`
	KeyConfigured   bool               `json:"key_configured"`
	BasicConfigured bool               `json:"basic_configured"`
	Model           *SearchModelConfig `json:"model,omitempty"`
}

func DefaultSearchConfig() SearchConfig {
	return SearchConfig{
		Enabled:         false,
		Endpoint:        nil,
		KeyConfigured:   false,
		BasicConfigured: false,
		Model:           nil,
	}
}
func (value *SearchConfig) UnmarshalJSON(data []byte) error {
	type plain SearchConfig
	decoded := plain{}
	decoded.Model = nil
	if err := DecodeObject(data, &decoded, []string{"enabled", "key_configured", "basic_configured"}, true); err != nil {
		return err
	}
	*value = SearchConfig(decoded)
	return nil
}
func (value SearchConfig) MarshalJSON() ([]byte, error) {
	type plain SearchConfig
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SearchModelConfig struct {
	Provider ProviderID `json:"provider"`
	Model    string     `json:"model"`
}

func DefaultSearchModelConfig() SearchModelConfig {
	return SearchModelConfig{
		Provider: ProviderID{},
		Model:    "",
	}
}
func (value *SearchModelConfig) UnmarshalJSON(data []byte) error {
	type plain SearchModelConfig
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "model"}, true); err != nil {
		return err
	}
	*value = SearchModelConfig(decoded)
	return nil
}
func (value SearchModelConfig) MarshalJSON() ([]byte, error) {
	type plain SearchModelConfig
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SearchSettings struct {
	Revision     uint64                            `json:"revision"`
	Driver       *SearchSelection                  `json:"driver"`
	Fallback     bool                              `json:"fallback"`
	Order        []SearchDriver                    `json:"order"`
	Providers    map[SearchDriver]SearchConfig     `json:"providers"`
	Capabilities map[SearchDriver]SearchCapability `json:"capabilities"`
}

func DefaultSearchSettings() SearchSettings {
	return SearchSettings{
		Revision:     0,
		Driver:       modelPointer(SearchSelection{Kind: "auto"}),
		Fallback:     true,
		Order:        defaultSearchOrder(),
		Providers:    defaultSearchProviders(),
		Capabilities: defaultSearchCapabilities(),
	}
}
func (value *SearchSettings) UnmarshalJSON(data []byte) error {
	type plain SearchSettings
	decoded := plain{}
	decoded.Order = defaultSearchOrder()
	decoded.Capabilities = defaultSearchCapabilities()
	if err := DecodeObject(data, &decoded, []string{"revision", "fallback", "providers"}, true); err != nil {
		return err
	}
	*value = SearchSettings(decoded)
	return nil
}
func (value SearchSettings) MarshalJSON() ([]byte, error) {
	type plain SearchSettings
	encoded := plain(value)
	encoded.Order = normalizeCollections(encoded.Order)
	encoded.Providers = normalizeCollections(encoded.Providers)
	encoded.Capabilities = normalizeCollections(encoded.Capabilities)
	return json.Marshal(encoded)
}

type SearchCredentials struct {
	Key   PluginSettingsSecretUpdate `json:"key"`
	Basic PluginSettingsSecretUpdate `json:"basic"`
}

func DefaultSearchCredentials() SearchCredentials {
	return SearchCredentials{
		Key:   PluginSettingsSecretUpdate{Kind: "keep"},
		Basic: PluginSettingsSecretUpdate{Kind: "keep"},
	}
}
func (value *SearchCredentials) UnmarshalJSON(data []byte) error {
	type plain SearchCredentials
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "basic"}, true); err != nil {
		return err
	}
	*value = SearchCredentials(decoded)
	return nil
}
func (value SearchCredentials) MarshalJSON() ([]byte, error) {
	type plain SearchCredentials
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SearchUpdate struct {
	Settings    SearchSettings                     `json:"settings"`
	Credentials map[SearchDriver]SearchCredentials `json:"credentials"`
}

func DefaultSearchUpdate() SearchUpdate {
	return SearchUpdate{
		Settings:    DefaultSearchSettings(),
		Credentials: map[SearchDriver]SearchCredentials{},
	}
}
func (value *SearchUpdate) UnmarshalJSON(data []byte) error {
	type plain SearchUpdate
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"settings", "credentials"}, true); err != nil {
		return err
	}
	*value = SearchUpdate(decoded)
	return nil
}
func (value SearchUpdate) MarshalJSON() ([]byte, error) {
	type plain SearchUpdate
	encoded := plain(value)
	encoded.Credentials = normalizeCollections(encoded.Credentials)
	return json.Marshal(encoded)
}

type SearchFreshness string

const (
	SearchFreshnessHour  SearchFreshness = "hour"
	SearchFreshnessDay   SearchFreshness = "day"
	SearchFreshnessWeek  SearchFreshness = "week"
	SearchFreshnessMonth SearchFreshness = "month"
	SearchFreshnessYear  SearchFreshness = "year"
)

func (value SearchFreshness) MarshalJSON() ([]byte, error) {
	switch value {
	case SearchFreshnessHour, SearchFreshnessDay, SearchFreshnessWeek, SearchFreshnessMonth, SearchFreshnessYear:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown SearchFreshness %q", value)
	}
}
func (value *SearchFreshness) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch SearchFreshness(decoded) {
	case SearchFreshnessHour, SearchFreshnessDay, SearchFreshnessWeek, SearchFreshnessMonth, SearchFreshnessYear:
		*value = SearchFreshness(decoded)
		return nil
	default:
		return fmt.Errorf("unknown SearchFreshness %q", decoded)
	}
}
func (value SearchFreshness) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *SearchFreshness) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type SearchRequest struct {
	Query          string           `json:"query"`
	Limit          uint8            `json:"limit"`
	Freshness      *SearchFreshness `json:"freshness,omitempty"`
	IncludeDomains []string         `json:"include_domains,omitempty"`
	ExcludeDomains []string         `json:"exclude_domains,omitempty"`
}

func DefaultSearchRequest() SearchRequest {
	return SearchRequest{
		Query:          "",
		Limit:          5,
		Freshness:      nil,
		IncludeDomains: []string{},
		ExcludeDomains: []string{},
	}
}
func (value *SearchRequest) UnmarshalJSON(data []byte) error {
	type plain SearchRequest
	decoded := plain{}
	decoded.Limit = 5
	decoded.Freshness = nil
	decoded.IncludeDomains = []string{}
	decoded.ExcludeDomains = []string{}
	if err := DecodeObject(data, &decoded, []string{"query"}, true); err != nil {
		return err
	}
	*value = SearchRequest(decoded)
	return nil
}
func (value SearchRequest) MarshalJSON() ([]byte, error) {
	type plain SearchRequest
	encoded := plain(value)
	encoded.IncludeDomains = normalizeCollections(encoded.IncludeDomains)
	encoded.ExcludeDomains = normalizeCollections(encoded.ExcludeDomains)
	return json.Marshal(encoded)
}

type SearchResult struct {
	Title     string  `json:"title"`
	Url       string  `json:"url"`
	Snippet   string  `json:"snippet"`
	Published *string `json:"published,omitempty"`
}

func DefaultSearchResult() SearchResult {
	return SearchResult{
		Title:     "",
		Url:       "",
		Snippet:   "",
		Published: nil,
	}
}
func (value *SearchResult) UnmarshalJSON(data []byte) error {
	type plain SearchResult
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"title", "url", "snippet"}, false); err != nil {
		return err
	}
	*value = SearchResult(decoded)
	return nil
}
func (value SearchResult) MarshalJSON() ([]byte, error) {
	type plain SearchResult
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SearchPage struct {
	Driver    SearchDriver   `json:"driver"`
	Results   []SearchResult `json:"results"`
	Summary   *string        `json:"summary,omitempty"`
	Untrusted bool           `json:"untrusted"`
	FetchHint string         `json:"fetch_hint"`
}

func DefaultSearchPage() SearchPage {
	return SearchPage{
		Driver:    SearchDriver(""),
		Results:   []SearchResult{},
		Summary:   nil,
		Untrusted: false,
		FetchHint: "",
	}
}
func (value *SearchPage) UnmarshalJSON(data []byte) error {
	type plain SearchPage
	decoded := plain{}
	decoded.Summary = nil
	if err := DecodeObject(data, &decoded, []string{"driver", "results", "untrusted", "fetch_hint"}, false); err != nil {
		return err
	}
	*value = SearchPage(decoded)
	return nil
}
func (value SearchPage) MarshalJSON() ([]byte, error) {
	type plain SearchPage
	encoded := plain(value)
	encoded.Results = normalizeCollections(encoded.Results)
	return json.Marshal(encoded)
}
