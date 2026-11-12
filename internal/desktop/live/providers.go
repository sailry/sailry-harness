package live

import (
	"encoding/json"
	"fmt"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// initProviders prepares what this Runtime keeps of its Node's providers and
// connectors, once: Settings reads them when it turns to the Node
// (settings_target.go).
func (r *Runtime) initProviders() {
	if r.authorizations == nil {
		r.initAuthorization()
	}
	if r.providerPresets == nil {
		r.providerPresets = map[string]protocol.ConversationPresetsEntry{}
	}
	if r.providerModels == nil {
		r.providerModels = map[string]map[string]protocol.ConversationModel{}
	}
	if r.providerDraftBases == nil {
		r.providerDraftBases = map[string]protocol.ConversationProvider{}
	}
}

// refreshModelCatalog asks the Node shown to fetch its model catalog again.
func (r *Runtime) refreshModelCatalog() {
	r.App.Settings.SetBusy(settings.BusyCatalog, true)
	r.command(protocol.Command{Kind: "refresh_model_catalog"}, "provider_catalog_failed", func(_ protocol.Output, ok bool) {
		if !r.showsSettings() {
			return
		}
		r.App.Settings.SetBusy(settings.BusyCatalog, false)
		if ok {
			r.readProviders()
		}
	})
}

// readProviders reads the Node's preset directory and compaction threshold
// for the pages; an answer for a Node they left is dropped.
func (r *Runtime) readProviders() {
	r.compactionObserved = r.snapshot().CompactionSettingsRevision
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "list_channel_presets", Data: protocol.CommandListChannelPresetsPayload{}})
		r.enqueue(func(c *ui.Context) {
			if !r.showsSettings() {
				return
			}
			if err != nil {
				r.reportKeys(c, err, "provider_catalog_failed", "provider_catalog_failed", "provider_catalog_failed")
				return
			}
			if listing, ok := detail[protocol.ConversationPresetsListing](out.Data); ok {
				r.applyPresets(listing)
				r.projectProviderSnapshot()
			}
		})
		r.fetchCompaction()
	})
	r.projectProviderSnapshot()
}

// fetchCompaction reads the Node's compaction threshold off the UI thread.
// A draft the user holds stays; only the saved value and revision move, and
// never back to an older revision.
func (r *Runtime) fetchCompaction() {
	result, failed := r.controller.Do(r.ctx, protocol.Command{Kind: "read_compaction_settings"})
	r.enqueue(func(c *ui.Context) {
		if !r.showsSettings() {
			return
		}
		if failed != nil {
			r.reportKeys(c, failed, "compaction_settings_load_failed", "compaction_settings_conflict", "compaction_settings_load_failed")
			return
		}
		if value, ok := detail[protocol.ConversationCompactionSettings](result.Data); ok {
			field := &r.App.Settings.Compaction
			// A read the Node answered before a save can arrive after the
			// save's answer; the older revision does not replace the newer.
			if !field.Pending && value.Revision >= field.Revision {
				pristine := field.Draft == field.Saved
				field.Saved = float64(value.Percent)
				if pristine {
					field.Draft = field.Saved
				}
				field.Revision = value.Revision
			}
		}
	})
}

// followCompaction reads the threshold again when the Node reports a
// revision newer than the one the pages hold, as another controller's save
// is. A save under way or a draft waits, as in the reference.
func (r *Runtime) followCompaction() {
	field := &r.App.Settings.Compaction
	revision := r.snapshot().CompactionSettingsRevision
	if field.Pending || field.Draft != field.Saved || revision == r.compactionObserved {
		return
	}
	r.compactionObserved = revision
	if field.Revision < revision {
		r.async(r.fetchCompaction)
	}
}

// presetBrand is the family an entry belongs to: its declared brand, else
// itself, apart for each source.
func presetBrand(entry protocol.ConversationPresetsEntry) string {
	name := entry.Preset.ID
	if entry.Preset.Brand != nil {
		name = *entry.Preset.Brand
	}
	switch entry.Source.Kind {
	case "partner":
		return "partner:" + name
	case "package":
		if entry.Package != nil {
			return "package:" + entry.Package.Name + ":" + name
		}
	}
	return name
}

// applyPresets projects the Node's preset directory as the brands the
// editor and the family filter offer. An entry is one method of its brand,
// and its routes (protocol, product, region) are that method's variants,
// not methods of their own. A brand is named by the display its first
// offered entry declares, else by the entry the brand is named after
// (brand "openai" is the "openai" entry, whatever sign-in or compatible
// entry is listed first), else by that first entry. Brands are in name
// order. Every entry, offered or not, stays known for the channels saved
// with it.
func (r *Runtime) applyPresets(listing protocol.ConversationPresetsListing) {
	r.providerPresets = map[string]protocol.ConversationPresetsEntry{}
	r.providerBrands = map[string]string{}
	bases := map[string]string{}
	for _, entry := range listing.Entries {
		p := entry.Preset
		key := presetBrand(entry)
		if base := key[strings.LastIndex(key, ":")+1:]; p.ID == base {
			bases[key] = p.Name
			if p.BrandDisplay != nil {
				bases[key] = p.BrandDisplay.Label
			}
		}
	}
	var brands []viewmodel.Brand
	for _, entry := range listing.Entries {
		p := entry.Preset
		source := entry.Source.Kind
		if entry.Package != nil {
			source += ":" + entry.Package.Name
		}
		for i := range p.Routes {
			r.providerPresets[fmt.Sprintf("%s:%s/%d", source, p.ID, i)] = entry
		}
		key := presetBrand(entry)
		if _, named := r.providerBrands[key]; !named || p.Offered && !slices.ContainsFunc(brands, func(v viewmodel.Brand) bool { return v.ID == key }) {
			name := p.Name
			if p.BrandDisplay != nil {
				name = p.BrandDisplay.Label
			} else if base, ok := bases[key]; ok {
				name = base
			}
			r.providerBrands[key] = name
		}
		if !p.Offered {
			continue
		}
		index := slices.IndexFunc(brands, func(v viewmodel.Brand) bool { return v.ID == key })
		if index < 0 {
			brands = append(brands, viewmodel.Brand{ID: key, Name: r.providerBrands[key]})
			index = len(brands) - 1
		}
		method := viewmodel.Method{ID: source + ":" + p.ID, Name: p.Name, Icon: p.ID, Access: "provider_access_key"}
		switch p.Auth.Kind {
		case "builtin":
			method.Access, method.Login, method.OAuthFields = "provider_access_login", "provider_access_login", true
			auth, _ := detail[protocol.ConversationPresetsAuthBuiltinPayload](p.Auth.Data)
			if auth.Authentication == protocol.AuthenticationClaudeOAuth {
				method.Login = "provider_login_claude"
			} else if auth.Authentication == protocol.AuthenticationChatGpt || auth.Authentication == protocol.AuthenticationChatGptOAuth {
				method.Login = "provider_login_chat_gpt"
			}
		case "plugin":
			method.Access, method.Login = "provider_access_login", "provider_access_login"
		case "none":
			method.Access = "provider_access_endpoint"
		default:
			// An API key for an address the user gives.
			if len(p.Routes) > 0 && p.Routes[0].Endpoint == "" {
				method.Access, method.Keyed = "provider_access_endpoint", true
			}
		}
		if p.KeyUrl != nil {
			method.KeyURL = *p.KeyUrl
		}
		var static []string
		if p.Models.Kind == "static" {
			if models, ok := detail[protocol.ConversationPresetsModelsStaticPayload](p.Models.Data); ok {
				static = models.IDs
			}
		}
		for i, route := range p.Routes {
			v := viewmodel.Route{ID: fmt.Sprintf("%s/%d", method.ID, i), API: string(apiForProtocol(route.Protocol)), Endpoint: route.Endpoint, Custom: route.Endpoint == "", Models: static}
			if route.Region != nil {
				v.Region = string(*route.Region)
			}
			if route.Product != nil {
				v.Product = string(*route.Product)
				for _, product := range p.Products {
					if product.Kind != *route.Product {
						continue
					}
					if product.Display != nil {
						v.ProductName = product.Display.Label
					}
					// The product's own model first, then the preset's.
					if product.DefaultModel != nil {
						v.Models = append([]string{*product.DefaultModel}, slices.DeleteFunc(slices.Clone(static), func(id string) bool { return id == *product.DefaultModel })...)
					}
				}
			}
			method.Routes = append(method.Routes, v)
		}
		brands[index].Methods = append(brands[index].Methods, method)
	}
	slices.SortStableFunc(brands, func(a, b viewmodel.Brand) int {
		if order := strings.Compare(strings.ToLower(a.Name), strings.ToLower(b.Name)); order != 0 {
			return order
		}
		return strings.Compare(a.ID, b.ID)
	})
	r.App.Settings.Brands = brands
}

// apiBrands are the families of channels saved without a preset, by their
// wire protocol.
var apiBrands = map[protocol.ConversationModelApi]string{
	"responses": "openai", "chat_completions": "openai", "anthropic": "anthropic", "gemini": "gemini",
	"deepseek": "deepseek", "open_code_go": "opencode", "open_code_zen": "opencode", "azure_open_ai": "azure-openai", "azure_ai": "azure-ai",
}

func apiForProtocol(p protocol.ConversationPresetsProtocol) protocol.ConversationModelApi {
	switch p {
	case "openai-responses":
		return "responses"
	case "openai-chat-completions":
		return "chat_completions"
	case "azure-openai":
		return "azure_open_ai"
	case "azure-ai":
		return "azure_ai"
	case "opencode-go":
		return "open_code_go"
	case "opencode-zen":
		return "open_code_zen"
	default:
		return protocol.ConversationModelApi(p)
	}
}

// projectProviderSnapshot shows the Node's channels, catalog and media
// models on the pages that hold this Node.
func (r *Runtime) projectProviderSnapshot() {
	if !r.showsSettings() {
		return
	}
	r.readProviderCredentials()
	snapshot := r.snapshot()
	now := uint64(time.Now().UnixMilli())
	r.refreshQuotaReset(now, snapshot.ProviderQuotas)
	var channels []viewmodel.Channel
	for _, p := range snapshot.Providers {
		ch := viewmodel.Channel{ID: p.ID.String(), Revision: p.Revision, Name: p.Name, API: string(p.Api), Endpoint: p.Endpoint, Auth: viewmodel.AuthKey, Enabled: p.Enabled, KeySet: p.Credential != nil}
		if p.Authentication != "api_key" {
			ch.Auth = viewmodel.AuthOAuth
			if p.Credential != nil && r.authorizations != nil {
				if credential, ok := r.authorizations.credentials[p.Credential.ID]; ok {
					ch.SignedIn = !credential.Revoked
					if credential.ExpiresAtMs != nil && *credential.ExpiresAtMs <= uint64(time.Now().UnixMilli()) {
						ch.SignedIn = false
					}
					if credential.AccountLabel != nil {
						ch.Account = *credential.AccountLabel
					}
				}
			}
			if p.Oauth != nil {
				var options struct {
					CatalogVersion      *string `json:"catalog_version"`
					UserAgent           string  `json:"user_agent"`
					EditorVersion       string  `json:"editor_version"`
					EditorPluginVersion string  `json:"editor_plugin_version"`
				}
				body, _ := json.Marshal(p.Oauth)
				if json.Unmarshal(body, &options) == nil {
					if options.CatalogVersion != nil {
						ch.OAuth[0] = *options.CatalogVersion
					}
					ch.OAuth[1], ch.OAuth[2], ch.OAuth[3] = options.UserAgent, options.EditorVersion, options.EditorPluginVersion
				}
			}
		}
		ch.APIVersion = azureVersion(p)
		for _, quota := range snapshot.ProviderQuotas {
			if quota.Provider == p.ID && p.Credential != nil && quota.Credential == *p.Credential {
				value := &viewmodel.Quota{FiveHour: -1, SevenDay: -1}
				value.FiveHour, value.SevenDay = quotaRemaining(quota.FiveHour, now), quotaRemaining(quota.SevenDay, now)
				ch.Quota = value
			}
		}
		if p.Preset != nil {
			// Without a saved route every route of the preset matches; the first one is shown.
			route := -1
			for id, entry := range r.providerPresets {
				_, index, _ := strings.Cut(id, "/")
				i, _ := strconv.Atoi(index)
				if (route < 0 || i < route) && entry.Preset.ID == p.Preset.ID && reflect.DeepEqual(entry.Source, p.Preset.Source) && i < len(entry.Preset.Routes) && (p.Preset.Route == nil || reflect.DeepEqual(*p.Preset.Route, presetRouteKey(entry.Preset.Routes[i]))) {
					route, ch.Method = i, id
					if entry.Preset.Auth.Kind == "none" {
						ch.Auth = viewmodel.AuthNone
					}
					ch.Brand, ch.Icon = presetBrand(entry), entry.Preset.ID
				}
			}
		}
		if ch.Brand == "" {
			ch.Brand = apiBrands[p.Api]
		}
		ch.BrandName = r.providerBrands[ch.Brand]
		for i, m := range p.Models {
			ch.Models = append(ch.Models, channelModel(m))
			if m.ID == p.DefaultModel {
				ch.Default = i
			}
		}
		channels = append(channels, ch)
	}
	r.App.Settings.Channels = channels
	r.followMedia(snapshot)
	status := snapshot.ModelCatalog
	r.App.Settings.Catalog = viewmodel.Catalog{Updated: status.UpdatedAtMs != nil, Scores: status.Scores.UpdatedAtMs != nil}
	if status.UpdatedAtMs != nil {
		r.App.Settings.Catalog.Last = time.UnixMilli(int64(*status.UpdatedAtMs)).Local().Format("2006-01-02 15:04")
	}
}

// azureVersion is the API version an Azure OpenAI provider was saved with.
func azureVersion(p protocol.ConversationProvider) string {
	if p.Options == nil {
		return ""
	}
	options, _ := detail[protocol.ConversationConnectionOptionsAzureOpenAiPayload](p.Options.Data)
	return options.ApiVersion
}

func channelModel(m protocol.ConversationModel) viewmodel.ChannelModel {
	v := viewmodel.ChannelModel{ID: m.ID, Name: m.ID, Context: int(m.Context), Output: int(m.Output), DefaultEffort: effortName(m.DefaultEffort),
		Vision: m.Vision, Tools: m.Tools, Reasoning: m.Reasoning, CustomCapabilities: m.CustomCapabilities}
	for _, generated := range m.Generates {
		v.Generates = append(v.Generates, string(generated))
	}
	if m.DisplayName != nil {
		v.Name = *m.DisplayName
	}
	for _, e := range m.Efforts {
		v.Efforts = append(v.Efforts, effortName(e))
	}
	if m.Capabilities != nil {
		for _, feature := range m.Capabilities.Features {
			f := viewmodel.ModelFeature{ID: feature.ID, Name: feature.ID}
			if feature.Name != nil {
				f.Name = *feature.Name
			}
			for _, option := range feature.Options {
				o := viewmodel.FeatureOption{ID: option.ID, Name: option.ID}
				if option.Name != nil {
					o.Name = *option.Name
				}
				f.Options = append(f.Options, o)
			}
			v.Features = append(v.Features, f)
		}
	}
	return v
}
func (r *Runtime) provider(id string) (protocol.ConversationProvider, bool) {
	for _, p := range r.controller.Snapshot().Providers {
		if p.ID.String() == id {
			return p, true
		}
	}
	return protocol.ConversationProvider{}, false
}

func (r *Runtime) providerDraft(d settings.ProviderDraft) protocol.ConversationProvider {
	key := fmt.Sprintf("%s/%d", d.Channel.ID, d.ExpectedRevision)
	p, captured := r.providerDraftBases[key]
	if !captured {
		var exists bool
		p, exists = r.provider(d.Channel.ID)
		if !exists {
			p = protocol.DefaultConversationProvider()
			p.ID, _ = protocol.ParseProviderID(d.Channel.ID)
			p.Enabled = true
			p.Authentication = "api_key"
		}
		p.Revision = d.ExpectedRevision
		r.providerDraftBases[key] = p
	}
	p.Name = d.Channel.Name
	p.Endpoint = d.Channel.Endpoint
	p.Api = protocol.ConversationModelApi(d.Channel.API)
	// The API version travels with an Azure OpenAI provider; a draft that
	// names none keeps the one the provider has.
	if p.Api == protocol.ConversationModelApiAzureOpenAi && d.Channel.APIVersion != "" {
		p.Options = &protocol.ConversationConnectionOptions{Kind: "azure_open_ai", Data: protocol.ConversationConnectionOptionsAzureOpenAiPayload{ApiVersion: d.Channel.APIVersion}}
	}
	if entry, ok := r.providerPresets[d.Channel.Method]; ok {
		ref := protocol.ConversationPresetsPresetRef{ID: entry.Preset.ID, Source: entry.Source}
		if _, index, ok := strings.Cut(d.Channel.Method, "/"); ok {
			if route, err := strconv.Atoi(index); err == nil && route >= 0 && route < len(entry.Preset.Routes) {
				value := presetRouteKey(entry.Preset.Routes[route])
				ref.Route = &value
				// A hosted service's product travels with the provider.
				if product := value.Product; product != nil && (p.Api == protocol.ConversationModelApiChatCompletions || p.Api == protocol.ConversationModelApiAnthropic || p.Api == protocol.ConversationModelApiDeepSeek) {
					p.Options = &protocol.ConversationConnectionOptions{Kind: "hosted", Data: protocol.ConversationConnectionOptionsHostedPayload{Product: *product}}
				}
			}
		}
		p.Preset = &ref
		p.Authentication = protocol.AuthenticationApiKey
		if entry.Preset.Auth.Kind == "builtin" {
			auth, _ := detail[protocol.ConversationPresetsAuthBuiltinPayload](entry.Preset.Auth.Data)
			p.Authentication = auth.Authentication
		} else if entry.Preset.Auth.Kind == "plugin" {
			p.Authentication = protocol.AuthenticationPlugin
		}
		if entry.Preset.Models.Kind == "static" && len(d.Channel.Models) == 0 && d.ExpectedRevision == 0 {
			if static, ok := detail[protocol.ConversationPresetsModelsStaticPayload](entry.Preset.Models.Data); ok {
				p.Models = nil
				for _, id := range static.IDs {
					model := protocol.DefaultConversationModel()
					model.ID, model.Context, model.Output, model.Tools, model.Vision = id, 128000, 16384, true, true
					p.Models = append(p.Models, model)
				}
				if len(p.Models) > 0 {
					p.DefaultModel = p.Models[0].ID
				}
			}
		}
	}
	if p.Authentication != protocol.AuthenticationApiKey && p.Authentication != protocol.AuthenticationPlugin {
		userAgent := d.OAuth[1]
		if userAgent == "" {
			userAgent = "Sailry/0.1"
		}
		var catalog *string
		if d.OAuth[0] != "" {
			value := d.OAuth[0]
			catalog = &value
		}
		options := protocol.ConversationOauthOptions{Kind: string(p.Authentication)}
		switch p.Authentication {
		case protocol.AuthenticationChatGpt:
			options.Data = protocol.ConversationOauthOptionsChatGptPayload{CatalogVersion: catalog, UserAgent: userAgent}
		case protocol.AuthenticationChatGptOAuth:
			options.Data = protocol.ConversationOauthOptionsChatGptOAuthPayload{CatalogVersion: catalog, UserAgent: userAgent}
		case protocol.AuthenticationClaudeOAuth:
			options.Data = protocol.ConversationOauthOptionsClaudeOAuthPayload{UserAgent: userAgent}
		case protocol.AuthenticationCopilot:
			editor, plugin := d.OAuth[2], d.OAuth[3]
			// The Node's defaults (providers.DefaultCopilot*).
			if d.OAuth[1] == "" {
				userAgent = "GitHubCopilotChat/0.68.0"
			}
			if editor == "" {
				editor = "vscode/1.140.0"
			}
			if plugin == "" {
				plugin = "copilot-chat/0.68.0"
			}
			options.Data = protocol.ConversationOauthOptionsCopilotPayload{UserAgent: userAgent, EditorVersion: editor, EditorPluginVersion: plugin}
		}
		p.Oauth = &options
	}
	old := p.Models
	p.Models = nil
	if len(d.Channel.Models) == 0 && d.ExpectedRevision == 0 {
		p.Models = old
	}
	for _, model := range d.Channel.Models {
		m := protocol.DefaultConversationModel()
		m.Context, m.Output, m.Tools, m.Vision = 128000, 16384, true, true
		m.ID = model.ID
		if known, ok := r.providerModels[d.Channel.ID][model.ID]; ok {
			m = known
		}
		if i := slices.IndexFunc(old, func(v protocol.ConversationModel) bool { return v.ID == model.ID }); i >= 0 {
			m = old[i]
			// What a later discovery recorded for the model's adapter replaces
			// the saved record; the rest of the saved model stays.
			if known, ok := r.providerModels[d.Channel.ID][model.ID]; ok && known.Capabilities != nil && known.Capabilities.Options != nil {
				capabilities := protocol.DefaultConversationDiscoveryCapabilities()
				if m.Capabilities != nil {
					capabilities = *m.Capabilities
				}
				capabilities.Options = known.Capabilities.Options
				m.Capabilities = &capabilities
			}
		}
		if model.Name != "" && model.Name != model.ID {
			name := model.Name
			m.DisplayName = &name
		}
		if model.Context > 0 {
			m.Context = uint32(model.Context)
		}
		if model.Output > 0 {
			m.Output = uint32(model.Output)
		}
		m.Efforts = nil
		for _, e := range model.Efforts {
			m.Efforts = append(m.Efforts, parseEffort(e))
		}
		m.Reasoning = len(m.Efforts) > 0
		// Tags the user chose in the form replace the catalog's and stay the
		// model's; without them the Node completes the tags from its catalog.
		if m.CustomCapabilities = model.CustomCapabilities; m.CustomCapabilities {
			m.Vision, m.Tools, m.Reasoning = model.Vision, model.Tools, model.Reasoning
			m.Generates = []protocol.MediaGeneration{}
			for _, generated := range model.Generates {
				m.Generates = append(m.Generates, protocol.MediaGeneration(generated))
			}
		}
		m.DefaultEffort = parseEffort(model.DefaultEffort)
		p.Models = append(p.Models, m)
	}
	if len(p.Models) > 0 {
		p.DefaultModel = p.Models[min(max(d.Channel.Default, 0), len(p.Models)-1)].ID
	} else {
		p.DefaultModel = ""
	}
	return p
}

func presetRouteKey(route protocol.ConversationPresetsRoute) protocol.ConversationPresetsRouteKey {
	return protocol.ConversationPresetsRouteKey{Protocol: route.Protocol, Region: route.Region, Product: route.Product}
}
