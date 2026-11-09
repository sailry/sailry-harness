package providers

import (
	"context"
	_ "embed"
	"encoding/json"
	"net/http"
	"net/url"
	"regexp"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const presetsURL = "https://sailry.dev/catalogs/model-presets.json"

//go:embed data/model-presets.json
var presetsJSON []byte

func Bundled() protocol.ConversationPresetsCatalog {
	var catalog protocol.ConversationPresetsCatalog
	if err := json.Unmarshal(presetsJSON, &catalog); err != nil {
		panic("bundled provider presets are invalid")
	}
	return catalog
}

type Directory struct {
	once        sync.Once
	gate        chan struct{}
	catalog     *protocol.ConversationPresetsCatalog
	attempted   time.Time
	unavailable bool
}

func (directory *Directory) Read(ctx context.Context) (protocol.ConversationPresetsListing, error) {
	directory.once.Do(func() { directory.gate = make(chan struct{}, 1); directory.gate <- struct{}{} })
	select {
	case <-ctx.Done():
		return protocol.ConversationPresetsListing{}, cancelled("channel preset catalog read cancelled")
	case <-directory.gate:
	}
	defer func() { directory.gate <- struct{}{} }()
	ttl := 15 * time.Minute
	if directory.unavailable {
		ttl = 30 * time.Second
	}
	if directory.catalog == nil || time.Since(directory.attempted) >= ttl {
		catalog, err := downloadPresets(ctx)
		if ctx.Err() != nil {
			return protocol.ConversationPresetsListing{}, cancelled("channel preset catalog read cancelled")
		}
		directory.attempted = time.Now()
		directory.unavailable = err != nil
		if err == nil {
			directory.catalog = &catalog
		} else if directory.catalog == nil {
			catalog = Bundled()
			directory.catalog = &catalog
		}
	}
	return Listing(*directory.catalog, directory.unavailable), nil
}
func Listing(partners protocol.ConversationPresetsCatalog, unavailable bool) protocol.ConversationPresetsListing {
	result := protocol.DefaultConversationPresetsListing()
	result.PartnersUnavailable = unavailable
	bundled := Bundled()
	for _, preset := range bundled.Builtin {
		entry := protocol.DefaultConversationPresetsEntry()
		entry.Source = protocol.ConversationPresetsSource{Kind: "builtin"}
		entry.Preset = preset
		entry.Provenance = protocol.ConversationPresetsProvenanceOfficial
		result.Entries = append(result.Entries, entry)
	}
	for _, preset := range partners.Partners {
		entry := protocol.DefaultConversationPresetsEntry()
		entry.Source = protocol.ConversationPresetsSource{Kind: "partner"}
		entry.Preset = preset
		entry.Provenance = protocol.ConversationPresetsProvenanceThirdParty
		result.Entries = append(result.Entries, entry)
	}
	sort.Slice(result.Entries, func(i, j int) bool {
		a, b := result.Entries[i], result.Entries[j]
		if a.Source.Kind != b.Source.Kind {
			return a.Source.Kind == "builtin"
		}
		if a.Preset.Name != b.Preset.Name {
			return a.Preset.Name < b.Preset.Name
		}
		return a.Preset.ID < b.Preset.ID
	})
	return result
}
func downloadPresets(ctx context.Context) (protocol.ConversationPresetsCatalog, error) {
	var catalog protocol.ConversationPresetsCatalog
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, presetsURL, nil)
	if err != nil {
		return catalog, err
	}
	request.Header.Set("Accept", "application/json")
	response, err := httpClient(15 * time.Second).Do(request)
	if err != nil {
		return catalog, unavailable("channel preset catalog is unavailable")
	}
	raw, err := readJSON(response, 1024*1024)
	if err != nil {
		return catalog, err
	}
	return ParsePresets(raw)
}
func ParsePresets(raw []byte) (protocol.ConversationPresetsCatalog, error) {
	var catalog protocol.ConversationPresetsCatalog
	if len(raw) > 1024*1024 {
		return catalog, invalid("invalid channel preset declaration")
	}
	var document map[string]json.RawMessage
	if json.Unmarshal(raw, &document) != nil {
		return catalog, invalid("invalid channel preset declaration")
	}
	if document["partners"] == nil {
		document["partners"] = json.RawMessage(`[]`)
	}
	var partners []json.RawMessage
	if json.Unmarshal(document["partners"], &partners) != nil {
		return catalog, invalid("invalid channel preset declaration")
	}
	valid := []protocol.ConversationPresetsPreset{}
	seen := map[string]bool{}
	groups := map[string]string{}
	for _, body := range partners {
		var preset protocol.ConversationPresetsPreset
		if json.Unmarshal(body, &preset) != nil || ValidatePreset(preset, false) != nil || seen[preset.ID] {
			continue
		}
		if preset.Brand != nil && preset.BrandDisplay != nil {
			display, _ := json.Marshal(preset.BrandDisplay)
			if before, ok := groups[*preset.Brand]; ok && before != string(display) {
				continue
			}
			groups[*preset.Brand] = string(display)
		}
		seen[preset.ID] = true
		valid = append(valid, preset)
	}
	document["partners"], _ = json.Marshal(valid)
	body, _ := json.Marshal(document)
	if json.Unmarshal(body, &catalog) != nil || catalog.Version != 1 {
		return catalog, invalid("invalid channel preset declaration")
	}
	catalog.Builtin = nil
	catalog.Policies = nil
	return catalog, nil
}

var presetID = regexp.MustCompile(`^[A-Za-z0-9_-]+$`)

func ValidatePreset(preset protocol.ConversationPresetsPreset, builtin bool) error {
	if !presetID.MatchString(preset.ID) || strings.TrimSpace(preset.Name) == "" || len(preset.Routes) == 0 || !builtin && preset.Auth.Kind == "builtin" || preset.Auth.Kind == "plugin" || preset.Adapter != nil {
		return invalid("invalid channel preset declaration")
	}
	for _, value := range []*string{preset.Homepage, preset.KeyUrl} {
		if value != nil && !publicURL(*value) {
			return invalid("invalid channel preset declaration")
		}
	}
	if preset.Brand != nil && !identifier(*preset.Brand, 256) {
		return invalid("invalid channel preset declaration")
	}
	if preset.Logo != nil {
		logo := *preset.Logo
		if strings.HasPrefix(logo, "icons/providers/") {
			name := strings.TrimSuffix(strings.TrimPrefix(logo, "icons/providers/"), ".svg")
			if !strings.HasSuffix(logo, ".svg") || !presetID.MatchString(name) {
				return invalid("invalid channel preset declaration")
			}
		} else if !publicURL(logo) {
			return invalid("invalid channel preset declaration")
		}
	}
	seen := map[string]bool{}
	for _, route := range preset.Routes {
		if route.Protocol == protocol.ConversationPresetsProtocolPlugin {
			return invalid("invalid channel preset declaration")
		}
		raw, _ := json.Marshal(protocol.ConversationPresetsRouteKey{Protocol: route.Protocol, Region: route.Region, Product: route.Product})
		if seen[string(raw)] {
			return invalid("invalid channel preset declaration")
		}
		seen[string(raw)] = true
		if !(builtin && route.Endpoint == "") {
			if _, err := Endpoint(route.Endpoint); err != nil {
				return invalid("invalid channel preset declaration")
			}
		}
	}
	if preset.Auth.Kind == "api_key" {
		var auth protocol.ConversationPresetsAuthApiKeyPayload
		if err := unionData(preset.Auth.Data, &auth); err != nil {
			return err
		}
		if auth.Header != nil || auth.Prefix != nil {
			name, prefix := "authorization", ""
			if auth.Header != nil {
				name = *auth.Header
			}
			if auth.Prefix != nil {
				prefix = *auth.Prefix
			}
			if err := validateInjection(protocol.PluginModelAuthInjection{Header: name, Template: prefix + "{key}"}); err != nil {
				return err
			}
			for _, route := range preset.Routes {
				if route.Protocol != protocol.ConversationPresetsProtocolOpenAiResponses && route.Protocol != protocol.ConversationPresetsProtocolOpenAiChatCompletions && route.Protocol != "anthropic" {
					return invalid("invalid channel preset declaration")
				}
			}
		}
	}
	if preset.Models.Kind == "static" {
		var models protocol.ConversationPresetsModelsStaticPayload
		if err := unionData(preset.Models.Data, &models); err != nil {
			return err
		}
		ids := map[string]bool{}
		if len(models.IDs) == 0 {
			return invalid("invalid channel preset declaration")
		}
		for _, id := range models.IDs {
			if strings.TrimSpace(id) == "" || ids[id] {
				return invalid("invalid channel preset declaration")
			}
			ids[id] = true
		}
	}
	for _, policy := range preset.Policies {
		if err := ValidatePolicy(policy); err != nil {
			return err
		}
	}
	return nil
}
func ValidatePolicy(policy protocol.ConversationPresetsRequestPolicy) error {
	if len(policy.Models) == 0 {
		return invalid("invalid channel preset declaration")
	}
	seen := map[string]bool{}
	for _, id := range policy.Models {
		if strings.TrimSpace(id) == "" || seen[id] {
			return invalid("invalid channel preset declaration")
		}
		seen[id] = true
	}
	for key := range policy.Body {
		switch key {
		case "model", "messages", "input", "tools", "stream", "instructions", "system", "tool_choice", "parallel_tool_calls", "functions", "function_call", "response_format":
			return invalid("invalid channel preset declaration")
		}
	}
	return nil
}
func validateInjection(injection protocol.PluginModelAuthInjection) error {
	header := strings.ToLower(injection.Header)
	if !regexp.MustCompile("^[!#$%&'*+.^_`|~0-9A-Za-z-]+$").MatchString(header) {
		return invalid("invalid channel preset declaration")
	}
	switch header {
	case "host", "connection", "content-length", "transfer-encoding", "proxy-authorization", "upgrade":
		return invalid("invalid channel preset declaration")
	}
	template := strings.ReplaceAll(injection.Template, "{key}", "fixture")
	for _, r := range template {
		if r < 32 && r != '\t' || r == 127 {
			return invalid("invalid channel preset declaration")
		}
	}
	if strings.ContainsAny(template, "{}\r\n") {
		return invalid("invalid channel preset declaration")
	}
	return nil
}
func publicURL(value string) bool {
	parsed, err := url.Parse(value)
	return err == nil && parsed.Scheme == "https" && parsed.Hostname() != "" && parsed.User == nil
}
