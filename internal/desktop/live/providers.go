package live

import (
	"encoding/json"
	"fmt"
	"maps"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	nodeproviders "github.com/sailry/sailry-harness/internal/node/providers"
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
		r.providerModels = map[string]providerDiscovery{}
	}
	if r.providerDraftBases == nil {
		r.providerDraftBases = map[string]protocol.ConversationProvider{}
	}
}

// refreshModelCatalog asks the Node shown to fetch its model catalog again.
func (r *Runtime) refreshModelCatalog() {
	if r.modelCatalogLoading {
		return
	}
	current := r.settingsCurrent()
	r.modelCatalogLoading = true
	r.App.Settings.SetBusy(settings.BusyCatalog, true)
	r.command(protocol.Command{Kind: "refresh_model_catalog"}, "provider_catalog_failed", func(_ protocol.Output, ok bool) {
		r.modelCatalogLoading = false
		if !current() {
			return
		}
		r.App.Settings.SetBusy(settings.BusyCatalog, false)
		if ok {
			r.readProviders()
		}
	})
}

// followModelCatalog refreshes missing network datasets once on entering the
// page. A failed fetch waits for another visit or the explicit Update action.
func (r *Runtime) followModelCatalog() {
	root := r.localRuntime()
	if root.App.Page != shell.SettingsPage || root.App.Settings.Section != settings.Providers {
		r.modelCatalogEntered = false
		return
	}
	root.App.Settings.SetBusy(settings.BusyCatalog, r.modelCatalogLoading)
	if r.modelCatalogEntered || !r.controller.Connected() {
		return
	}
	r.modelCatalogEntered = true
	status := r.snapshot().ModelCatalog
	if status.UpdatedAtMs == nil || status.Scores.UpdatedAtMs == nil {
		r.refreshModelCatalog()
	}
}

// readProviders reads the Node's preset directory and compaction threshold
// for the pages; an answer for a Node they left is dropped.
func (r *Runtime) readProviders() {
	r.compactionObserved = r.snapshot().CompactionSettingsRevision
	r.readProviderPresets()
	if r.showsSettings() {
		r.fetchCompaction()
	}
	r.projectProviderSnapshot()
}

func (r *Runtime) readProviderPresets() {
	root := r.localRuntime()
	if r.providerPresetsLoading || !r.controller.Connected() {
		return
	}
	packages := r.snapshot().Plugins
	if r.providerPresetsLoaded && samePresetPackages(r.providerPresetPackages, packages) {
		return
	}
	r.providerPresetsLoaded = false
	r.providerPresetsLoading = true
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "list_channel_presets", Data: protocol.CommandListChannelPresetsPayload{}})
		r.enqueue(func(c *ui.Context) {
			r.providerPresetsLoading = false
			if !samePresetPackages(packages, r.snapshot().Plugins) {
				r.readProviderPresets()
				return
			}
			if err != nil {
				if root.shownSettings() == r && root.App.Page == shell.SettingsPage && root.App.Settings.Section == settings.Providers {
					r.reportKeys(c, err, "provider_catalog_failed", "provider_catalog_failed", "provider_catalog_failed")
				}
				return
			}
			if listing, ok := detail[protocol.ConversationPresetsListing](out.Data); ok {
				r.applyPresets(listing)
				r.providerPresetsLoaded = true
				r.providerPresetPackages = packages
				root.projectProviderSnapshot()
				return
			}
			if root.shownSettings() == r && root.App.Page == shell.SettingsPage && root.App.Settings.Section == settings.Providers {
				r.reportKeys(c, protocol.NewConfigurationFault(protocol.ConfigurationProviderMissing, "model preset catalog response is invalid"), "provider_catalog_failed", "provider_catalog_failed", "provider_catalog_failed")
			}
		})
	})
}

// A preset captures its package and settings revision. Transient MCP status
// changes also publish plugin summaries but do not change the directory.
func samePresetPackages(a, b []protocol.PluginSummary) bool {
	return slices.EqualFunc(a, b, func(x, y protocol.PluginSummary) bool {
		return x.Name == y.Name && x.Digest == y.Digest && x.SettingsRevision == y.SettingsRevision &&
			x.Enabled == y.Enabled && x.Compatibility.Kind == y.Compatibility.Kind
	})
}

// fetchCompaction reads the Node's compaction threshold off the UI thread.
// A draft the user holds stays; only the saved value and revision move, and
// never back to an older revision.
func (r *Runtime) fetchCompaction() {
	current := r.settingsCurrent()
	r.async(func() {
		result, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_compaction_settings"})
		r.enqueue(func(c *ui.Context) {
			if !current() {
				return
			}
			if err != nil {
				r.reportKeys(c, err, "compaction_settings_load_failed", "compaction_settings_conflict", "compaction_settings_load_failed")
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
		r.fetchCompaction()
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

func providerBrandKey(node string, entry protocol.ConversationPresetsEntry) string {
	brand := presetBrand(entry)
	if entry.Source.Kind == "package" {
		return node + "/" + brand
	}
	return brand
}

// Locally constructed catalogs and decoded snapshots use different Go forms
// of the same package source payload. Match the protocol identity in both.
func samePresetSource(a, b protocol.ConversationPresetsSource) bool {
	if a.Kind != b.Kind {
		return false
	}
	if a.Kind != "package" {
		return a.Kind == "builtin" || a.Kind == "partner"
	}
	x, okX := detail[protocol.ConversationPresetsSourcePackagePayload](a.Data)
	y, okY := detail[protocol.ConversationPresetsSourcePackagePayload](b.Data)
	return okX && okY && x.Name != "" && x.Name == y.Name
}

func presetRouteIndex(id string) int {
	separator := strings.LastIndex(id, "/")
	if separator < 0 {
		return -1
	}
	index, err := strconv.Atoi(id[separator+1:])
	if err != nil {
		return -1
	}
	return index
}

func (r *Runtime) providerPresetForMethod(method string) (protocol.ConversationPresetsEntry, bool) {
	root := r.localRuntime()
	if node, _, scoped := strings.Cut(method, "/"); scoped {
		if owner := root.runtimeFor(node); owner != nil {
			entry, exists := owner.providerPresets[method]
			return entry, exists
		}
	}
	entry, exists := r.providerPresets[method]
	return entry, exists
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
	node := r.controller.Target().String()
	bases := map[string]string{}
	for _, entry := range listing.Entries {
		p := entry.Preset
		brand := presetBrand(entry)
		key := providerBrandKey(node, entry)
		if p.BrandDisplay != nil && p.BrandDisplay.Icon != nil && p.BrandDisplay.Icon.Kind == "Svg" {
			// The package's own mark, for a brand the collection lacks.
			var value protocol.PluginDesktopIconSvgPayload
			if body, err := json.Marshal(p.BrandDisplay.Icon); err == nil && json.Unmarshal(body, &value) == nil {
				if mark, err := ui.ParseSVG([]byte(value.Svg)); err == nil {
					icons.Declare(key, mark)
				}
			}
		}
		base := brand
		if separator := strings.LastIndex(brand, ":"); separator >= 0 {
			base = brand[separator+1:]
		}
		if p.ID == base {
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
			r.providerPresets[fmt.Sprintf("%s/%s:%s/%d", node, source, p.ID, i)] = entry
		}
		key := providerBrandKey(node, entry)
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
			brands = append(brands, viewmodel.Brand{Node: node, ID: key, Name: r.providerBrands[key]})
			index = len(brands) - 1
		}
		method := viewmodel.Method{Node: node, ID: node + "/" + source + ":" + p.ID, Name: p.Name, Icon: p.ID, Access: "provider_access_key", Auth: viewmodel.AuthKey}
		switch p.Auth.Kind {
		case "builtin":
			method.Access, method.Login = "provider_access_login", "provider_access_login"
			auth, _ := detail[protocol.ConversationPresetsAuthBuiltinPayload](p.Auth.Data)
			if auth.Authentication == protocol.AuthenticationApiKey {
				method.Auth = viewmodel.AuthKey
			} else {
				method.Auth = viewmodel.AuthOAuth
			}
			method.OAuth = oauthDefaults(auth.Authentication)
			if auth.Authentication == protocol.AuthenticationClaudeOAuth {
				method.Login = "provider_login_claude"
			} else if auth.Authentication == protocol.AuthenticationChatGpt || auth.Authentication == protocol.AuthenticationChatGptOAuth {
				method.Login = "provider_login_chat_gpt"
			}
		case "plugin":
			method.Access, method.Login = "provider_access_login", "provider_access_login"
			method.Auth = viewmodel.AuthPlugin
		case "none":
			method.Access = "provider_access_endpoint"
			method.Auth = viewmodel.AuthNone
		default:
			// An API key for an address the user gives.
			if len(p.Routes) > 0 && p.Routes[0].Endpoint == "" {
				method.Access, method.Keyed = "provider_access_endpoint", true
			}
		}
		if p.KeyUrl != nil {
			method.KeyURL = *p.KeyUrl
		}
		// A sign-in asks its warning before each attempt (beginProviderLogin);
		// a method without one asks it when it is picked.
		if method.Login == "" {
			method.Warning = r.loginWarning(entry)
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
	r.providerCatalogBrands = brands
}

// oauthDefaults are the advanced settings a built-in sign-in has, at the
// values the Node uses for one saved without them (providers.OAuthFields),
// in Channel.OAuth's order: catalog version, user agent, editor version and
// editor plugin version.
func oauthDefaults(authentication protocol.Authentication) [4]string {
	switch authentication {
	case protocol.AuthenticationChatGpt:
		return [4]string{1: defaultUserAgent}
	case protocol.AuthenticationChatGptOAuth:
		return [4]string{nodeproviders.DefaultCatalogVersion, defaultUserAgent}
	case protocol.AuthenticationClaudeOAuth:
		return [4]string{1: nodeproviders.DefaultClaudeAgent}
	case protocol.AuthenticationCopilot:
		return [4]string{1: nodeproviders.DefaultCopilotAgent, 2: nodeproviders.DefaultCopilotEditor, 3: nodeproviders.DefaultCopilotEditorPlugin}
	}
	return [4]string{}
}

// defaultUserAgent is the Node's user agent for a ChatGPT sign-in.
const defaultUserAgent = "Sailry/0.1"

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

// projectProviderSnapshot combines channels owned by connected Nodes while
// model metadata and general settings remain those of the selected Node.
func (r *Runtime) projectProviderSnapshot() {
	root := r.localRuntime()
	loadAccountDetails := root.App.Page == shell.SettingsPage && root.App.Settings.Section == settings.Providers
	owners := []*Runtime{root}
	for _, id := range slices.Sorted(maps.Keys(root.bindings)) {
		if owner := root.bindings[id]; owner != nil && owner.controller.Connected() {
			owners = append(owners, owner)
		}
	}
	var brands []viewmodel.Brand
	channelsByNode := make(map[string][]viewmodel.Channel, len(owners)+1)
	revisions := map[preferences.ProviderMember]uint64{}
	for _, owner := range owners {
		owner.initProviders()
		if loadAccountDetails {
			owner.readProviderCredentials()
			owner.readProviderPresets()
		}
		rows := owner.projectNodeProviders()
		channelsByNode[owner.controller.Target().String()] = rows
		if owner == root {
			channelsByNode[""] = rows
		}
		for _, row := range rows {
			if member, ok := providerMember(row.Node, providerIDFromString(row.ID)); ok {
				revisions[member] = row.Revision
			}
		}
		brands = append(brands, owner.providerCatalogBrands...)
	}
	groups := map[preferences.ProviderMember]preferences.ProviderGroup{}
	if root.Preferences != nil {
		for _, group := range root.Preferences.Data.ProviderGroups {
			for _, member := range group.Members {
				groups[member] = group
			}
		}
	}
	seenGroups := map[protocol.RequestID]bool{}
	var channels []viewmodel.Channel
	for _, owner := range owners {
		node := owner.controller.Target()
		rows := channelsByNode[node.String()]
		for index := range rows {
			channel := &rows[index]
			group, grouped := groups[preferences.ProviderMember{Node: node, Provider: providerIDFromString(channel.ID)}]
			if grouped {
				channel.Group = group.ID.String()
				channel.Targets = nil
				channel.TargetIDs = map[string]string{}
				channel.TargetRevisions = map[string]uint64{}
				for _, member := range group.Members {
					host := member.Node.String()
					channel.Targets = append(channel.Targets, host)
					channel.TargetIDs[host] = member.Provider.String()
					channel.TargetRevisions[host] = revisions[member]
				}
				if seenGroups[group.ID] {
					continue
				}
				seenGroups[group.ID] = true
			} else {
				channel.TargetIDs = map[string]string{channel.Node: channel.ID}
				channel.TargetRevisions = map[string]uint64{channel.Node: channel.Revision}
			}
			channels = append(channels, *channel)
		}
	}
	root.App.Settings.Channels, root.App.Settings.ChannelsByNode = channels, channelsByNode
	root.App.Settings.Brands = brands
	if owner := root.shownSettings(); owner != nil {
		owner.followMedia(owner.snapshot())
	}
	catalogOwner := root
	if loadAccountDetails {
		catalogOwner = root.shownSettings()
	}
	root.App.Settings.Catalog = viewmodel.Catalog{}
	if catalogOwner != nil {
		root.App.Settings.Catalog = root.providerCatalogStatus(catalogOwner.controller.Target().String())
	}
}

func (r *Runtime) providerCatalogStatus(node string) viewmodel.Catalog {
	owner := r.providerOwner(node)
	if owner == nil {
		return viewmodel.Catalog{}
	}
	status := owner.snapshot().ModelCatalog
	catalog := viewmodel.Catalog{Updated: status.UpdatedAtMs != nil, Bundled: status.Bundled, Scores: status.Scores.UpdatedAtMs != nil}
	if status.UpdatedAtMs != nil {
		catalog.Last = time.UnixMilli(int64(*status.UpdatedAtMs)).Local().Format("2006-01-02 15:04")
	}
	return catalog
}

func (r *Runtime) projectNodeProviders() []viewmodel.Channel {
	snapshot := r.snapshot()
	now := uint64(time.Now().UnixMilli())
	r.refreshQuotaReset(now, snapshot.ProviderQuotas)
	credentialsKnown := r.authorizations != nil && r.authorizations.credentialKey == r.providerCredentialKey(snapshot.Providers)
	var channels []viewmodel.Channel
	node := r.controller.Target().String()
	for _, p := range snapshot.Providers {
		ch := viewmodel.Channel{Node: node, Targets: []string{node}, ID: p.ID.String(), Revision: p.Revision, Name: p.Name, API: string(p.Api), Endpoint: p.Endpoint, Auth: viewmodel.AuthKey, Enabled: p.Enabled, KeySet: p.Credential != nil}
		ch.ConfigurationReadOnly = p.Source != nil
		if p.Source != nil {
			ch.Source = p.Source.Node.String()
		}
		switch p.Authentication {
		case protocol.AuthenticationApiKey:
			ch.Auth = viewmodel.AuthKey
		default:
			ch.Auth = viewmodel.AuthOAuth
			if p.Credential != nil {
				ch.AccountPending = !credentialsKnown
				if credentialsKnown {
					credential, ok := r.authorizations.credentials[p.Credential.ID]
					ch.SignedIn = ok && !credential.Revoked && (credential.ExpiresAtMs == nil || *credential.ExpiresAtMs > now)
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
						if p.Authentication == protocol.AuthenticationChatGptOAuth && ch.OAuth[0] == nodeproviders.LegacyCatalogVersion {
							ch.OAuth[0] = nodeproviders.DefaultCatalogVersion
						}
					}
					ch.OAuth[1], ch.OAuth[2], ch.OAuth[3] = options.UserAgent, options.EditorVersion, options.EditorPluginVersion
				}
			}
		}
		ch.APIVersion = azureVersion(p)
		ch.Headers = maps.Clone(p.Headers)
		for _, quota := range snapshot.ProviderQuotas {
			if quota.Provider == p.ID && p.Credential != nil && quota.Credential == *p.Credential {
				value := &viewmodel.Quota{FiveHour: -1, SevenDay: -1}
				value.FiveHour, value.SevenDay = quotaRemaining(quota.FiveHour, now), quotaRemaining(quota.SevenDay, now)
				ch.Quota = value
			}
		}
		if p.Preset != nil {
			reference := *p.Preset
			// Package presets with auth none share the API-key wire value.
			// Only their declaration establishes whether a key is required.
			if reference.Source.Kind == "package" && p.Authentication == protocol.AuthenticationApiKey {
				ch.Auth = viewmodel.AuthUnresolved
			}
			if reference.Source.Kind == "builtin" && reference.ID == "compatible-chat" && p.Api == protocol.ConversationModelApiChatCompletions && p.Authentication == protocol.AuthenticationApiKey {
				if entry, ok := r.providerPresets[node+"/builtin:compatible-responses/0"]; ok && entry.Preset.Offered {
					// The retired Chat-only preset uses the offered compatible
					// editor so its protocol can change without rewriting saved state.
					reference.ID = entry.Preset.ID
				}
			}
			// Without a saved route, show the first route of the provider's protocol.
			route := -1
			for id, entry := range r.providerPresets {
				i := presetRouteIndex(id)
				if (route < 0 || i < route) && entry.Preset.ID == reference.ID && samePresetSource(entry.Source, reference.Source) && i < len(entry.Preset.Routes) && apiForProtocol(entry.Preset.Routes[i].Protocol) == p.Api && (reference.Route == nil || reflect.DeepEqual(*reference.Route, presetRouteKey(entry.Preset.Routes[i]))) {
					route, ch.Method = i, id
					switch entry.Preset.Auth.Kind {
					case "api_key", "builtin":
						if p.Authentication == protocol.AuthenticationApiKey {
							ch.Auth = viewmodel.AuthKey
						}
					case "none":
						ch.Auth = viewmodel.AuthNone
					case "plugin":
						ch.Auth = viewmodel.AuthPlugin
					}
					ch.Brand, ch.Icon = providerBrandKey(node, entry), entry.Preset.ID
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
	return channels
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
		if d.ExpectedRevision != 0 {
			p, exists = r.provider(d.Channel.ID)
		}
		if !exists {
			p = protocol.DefaultConversationProvider()
			p.ID, _ = protocol.ParseProviderID(d.Channel.ID)
			p.Enabled = true
			p.Authentication = "api_key"
		}
		p.Revision = d.ExpectedRevision
		r.providerDraftBases[key] = p
	}
	p.Shared = false
	p.Name = d.Channel.Name
	p.Endpoint = d.Channel.Endpoint
	p.Api = protocol.ConversationModelApi(d.Channel.API)
	// The API version travels with an Azure OpenAI provider; a draft that
	// names none keeps the one the provider has.
	if p.Api == protocol.ConversationModelApiAzureOpenAi && d.Channel.APIVersion != "" {
		p.Options = &protocol.ConversationConnectionOptions{Kind: "azure_open_ai", Data: protocol.ConversationConnectionOptionsAzureOpenAiPayload{ApiVersion: d.Channel.APIVersion}}
	}
	if entry, ok := r.providerPresetForMethod(d.Channel.Method); ok {
		ref := protocol.ConversationPresetsPresetRef{ID: entry.Preset.ID, Source: entry.Source}
		if route := presetRouteIndex(d.Channel.Method); route >= 0 && route < len(entry.Preset.Routes) {
			value := presetRouteKey(entry.Preset.Routes[route])
			ref.Route = &value
			// A hosted service's product travels with the provider.
			if product := value.Product; product != nil && (p.Api == protocol.ConversationModelApiChatCompletions || p.Api == protocol.ConversationModelApiAnthropic || p.Api == protocol.ConversationModelApiDeepSeek) {
				p.Options = &protocol.ConversationConnectionOptions{Kind: "hosted", Data: protocol.ConversationConnectionOptionsHostedPayload{Product: *product}}
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
	if p.Authentication != protocol.AuthenticationApiKey && p.Authentication != protocol.AuthenticationPlugin && p.Authentication != "" {
		userAgent := d.OAuth[1]
		if userAgent == "" {
			userAgent = defaultUserAgent
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
			if d.OAuth[1] == "" {
				userAgent = nodeproviders.DefaultCopilotAgent
			}
			if editor == "" {
				editor = nodeproviders.DefaultCopilotEditor
			}
			if plugin == "" {
				plugin = nodeproviders.DefaultCopilotEditorPlugin
			}
			options.Data = protocol.ConversationOauthOptionsCopilotPayload{UserAgent: userAgent, EditorVersion: editor, EditorPluginVersion: plugin}
		}
		p.Oauth = &options
	}
	// Only API-key providers carry their own request headers.
	p.Headers = nil
	if p.Authentication == protocol.AuthenticationApiKey {
		p.Headers = maps.Clone(d.Channel.Headers)
	}
	old := p.Models
	p.Models = nil
	discovery := r.providerModels[d.Channel.ID]
	if discovery.generation != d.EditorGeneration || discovery.revision != d.ExpectedRevision ||
		discovery.endpoint != p.Endpoint || discovery.authentication != p.Authentication || discovery.api != p.Api {
		discovery = providerDiscovery{}
	}
	if len(d.Channel.Models) == 0 && d.ExpectedRevision == 0 {
		p.Models = old
	}
	for _, model := range d.Channel.Models {
		m := protocol.DefaultConversationModel()
		m.Context, m.Output, m.Tools, m.Vision = 128000, 16384, true, true
		m.ID = model.ID
		known, discovered := discovery.models[model.ID]
		if discovered {
			m = known
		}
		if i := slices.IndexFunc(old, func(v protocol.ConversationModel) bool { return v.ID == model.ID }); i >= 0 {
			m = old[i]
			if discovered && known.Capabilities != nil {
				m = nodeproviders.RefreshAccountSpeed(p, m, known.Capabilities)
				// Refresh discovered adapter options alongside account speed.
				if known.Capabilities.Options != nil {
					capabilities := protocol.DefaultConversationDiscoveryCapabilities()
					if m.Capabilities != nil {
						capabilities = *m.Capabilities
					}
					capabilities.Options = known.Capabilities.Options
					m.Capabilities = &capabilities
				}
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
