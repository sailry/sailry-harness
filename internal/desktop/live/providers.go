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

func (r *Runtime) initProviders() {
	r.initAuthorization()
	r.providerPresets = map[string]protocol.ConversationPresetsEntry{}
	r.providerModels = map[string]map[string]protocol.ConversationModel{}
	r.providerDraftBases = map[string]protocol.ConversationProvider{}
	r.App.Settings.ProviderActions = settings.ProviderActions{Save: r.saveProvider, Discover: r.discoverModels, ReadKey: r.readProviderKey, Remove: r.removeProvider, Enable: r.enableProvider, SaveCompaction: r.saveCompaction,
		NewID:     func() string { return protocol.NewProviderID().String() },
		Available: r.controller.Connected,
		SignIn:    r.beginProviderLogin, SubmitLogin: r.submitProviderLogin, CancelLogin: r.cancelProviderLogin,
		RefreshCatalog: func() {
			r.command(protocol.Command{Kind: "refresh_model_catalog"}, "provider_catalog_failed", func(_ protocol.Output, ok bool) {
				if ok {
					r.readProviders()
				}
			})
		},
	}
	r.readProviders()
}

func (r *Runtime) readProviders() {
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "list_channel_presets", Data: protocol.CommandListChannelPresetsPayload{}})
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				r.reportKeys(c, err, "provider_catalog_failed", "provider_catalog_failed", "provider_catalog_failed")
				return
			}
			if listing, ok := detail[protocol.ConversationPresetsListing](out.Data); ok {
				r.applyPresets(listing)
				r.projectProviderSnapshot()
			}
		})
		result, failed := r.controller.Do(r.ctx, protocol.Command{Kind: "read_compaction_settings"})
		r.enqueue(func(c *ui.Context) {
			if failed != nil {
				r.reportKeys(c, failed, "compaction_settings_load_failed", "compaction_settings_conflict", "compaction_settings_load_failed")
				return
			}
			if value, ok := detail[protocol.ConversationCompactionSettings](result.Data); ok {
				field := &r.App.Settings.Compaction
				if !field.Pending {
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
	r.projectProviderSnapshot()
}

func (r *Runtime) applyPresets(listing protocol.ConversationPresetsListing) {
	var brands []viewmodel.Brand
	for _, entry := range listing.Entries {
		p := entry.Preset
		if !p.Offered {
			continue
		}
		brand := p.ID
		if p.Brand != nil {
			brand = *p.Brand
		}
		index := slices.IndexFunc(brands, func(v viewmodel.Brand) bool { return v.ID == brand })
		if index < 0 {
			brands = append(brands, viewmodel.Brand{ID: brand, Name: p.Name})
			index = len(brands) - 1
		}
		for i, route := range p.Routes {
			source := entry.Source.Kind
			if entry.Package != nil {
				source += ":" + entry.Package.Name
			}
			id := fmt.Sprintf("%s:%s/%d", source, p.ID, i)
			method := viewmodel.Method{ID: id, Name: p.Name, Access: "provider_access_key", API: string(apiForProtocol(route.Protocol)), Endpoint: route.Endpoint, Custom: route.Endpoint == ""}
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
			}
			if p.KeyUrl != nil {
				method.KeyURL = *p.KeyUrl
			}
			brands[index].Methods = append(brands[index].Methods, method)
			r.providerPresets[id] = entry
		}
	}
	r.App.Settings.Brands = brands
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

func (r *Runtime) projectProviderSnapshot() {
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
					if entry.Preset.Brand != nil {
						ch.Brand = *entry.Preset.Brand
					} else {
						ch.Brand = entry.Preset.ID
					}
				}
			}
		}
		for i, m := range p.Models {
			ch.Models = append(ch.Models, channelModel(m))
			if m.ID == p.DefaultModel {
				ch.Default = i
			}
		}
		channels = append(channels, ch)
	}
	r.App.Settings.Channels = channels
	status := snapshot.ModelCatalog
	r.App.Settings.Catalog = viewmodel.Catalog{Updated: status.UpdatedAtMs != nil, Scores: status.Scores.UpdatedAtMs != nil}
	if status.UpdatedAtMs != nil {
		r.App.Settings.Catalog.Last = time.UnixMilli(int64(*status.UpdatedAtMs)).Local().Format("2006-01-02 15:04")
	}
}

func channelModel(m protocol.ConversationModel) viewmodel.ChannelModel {
	v := viewmodel.ChannelModel{ID: m.ID, Name: m.ID, Context: int(m.Context), Output: int(m.Output), DefaultEffort: effortName(m.DefaultEffort)}
	if m.DisplayName != nil {
		v.Name = *m.DisplayName
	}
	for _, e := range m.Efforts {
		v.Efforts = append(v.Efforts, effortName(e))
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
	if entry, ok := r.providerPresets[d.Channel.Method]; ok {
		ref := protocol.ConversationPresetsPresetRef{ID: entry.Preset.ID, Source: entry.Source}
		if _, index, ok := strings.Cut(d.Channel.Method, "/"); ok {
			if route, err := strconv.Atoi(index); err == nil && route >= 0 && route < len(entry.Preset.Routes) {
				value := presetRouteKey(entry.Preset.Routes[route])
				ref.Route = &value
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
			if editor == "" {
				editor = "vscode/1.107.0"
			}
			if plugin == "" {
				plugin = "copilot-chat/0.35.0"
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
