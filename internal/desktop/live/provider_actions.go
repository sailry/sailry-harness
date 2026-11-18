package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) readProviderKey(id string, done func(string)) {
	p, ok := r.provider(id)
	if !ok {
		done("")
		return
	}
	r.execute(protocol.Command{Kind: "read_provider_key", Data: protocol.CommandReadProviderKeyPayload{Provider: p.ID, ExpectedRevision: p.Revision}}, func(c *ui.Context, out protocol.Output, err error) {
		if err != nil {
			r.reportKeys(c, err, "provider_key_failed", "provider_revision_changed", "provider_key_failed")
			return
		}
		if key, ok := detail[*protocol.Secret](out.Data); ok {
			value := ""
			if key != nil {
				value = key.Expose()
			}
			done(value)
		}
	})
}

func (r *Runtime) readProviderKeyForSave(p protocol.ConversationProvider, done func(string, bool)) {
	if p.Credential == nil {
		done("", true)
		return
	}
	r.execute(protocol.Command{Kind: "read_provider_key", Data: protocol.CommandReadProviderKeyPayload{Provider: p.ID, ExpectedRevision: p.Revision}}, func(c *ui.Context, out protocol.Output, err error) {
		if err != nil {
			r.reportKeys(c, err, "provider_key_failed", "provider_revision_changed", "provider_save_unknown")
			done("", false)
			return
		}
		key, valid := detail[*protocol.Secret](out.Data)
		if !valid {
			done("", false)
			return
		}
		if key == nil {
			done("", true)
			return
		}
		done(key.Expose(), true)
	})
}

func (r *Runtime) refreshProviderSnapshot() {
	root := r.localRuntime()
	r.async(func() {
		err := r.controller.Refresh(r.ctx)
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				r.reportKeys(c, err, "provider_catalog_read_failed", "provider_catalog_read_failed", "provider_catalog_read_failed")
				return
			}
			r.project()
			root.projectProviderSnapshot()
		})
	})
}

func (r *Runtime) discoverModels(editor *Runtime, d settings.ProviderDraft, done func([]viewmodel.ChannelModel)) {
	draft := editor.providerDraft(d)
	source := protocol.ConversationDiscoverySource{Kind: "draft", Data: protocol.ConversationDiscoveryDraft{Provider: draft.ID, Api: draft.Api, Endpoint: draft.Endpoint, Preset: draft.Preset, Credential: draft.Credential}}
	if draft.Authentication != protocol.AuthenticationApiKey && d.ExpectedRevision != 0 {
		source = protocol.ConversationDiscoverySource{Kind: "saved", Data: protocol.ConversationDiscoverySourceSavedPayload{Provider: draft.ID, ExpectedRevision: d.ExpectedRevision, Oauth: draft.Oauth}}
	}
	if d.Key != "" && source.Kind == "draft" {
		value := source.Data.(protocol.ConversationDiscoveryDraft)
		s := protocol.NewSecret(d.Key)
		value.Secret = &s
		source.Data = value
	}
	r.execute(protocol.Command{Kind: "discover_models", Data: source}, func(c *ui.Context, out protocol.Output, err error) {
		if err != nil {
			r.reportKeys(c, err, "provider_query_failed", "provider_revision_changed", "provider_query_failed")
			done(nil)
			return
		}
		var models []viewmodel.ChannelModel
		if result, ok := detail[protocol.ConversationDiscoveryCatalog](out.Data); ok {
			known := map[string]protocol.ConversationModel{}
			for _, m := range result.Models {
				model := protocol.DefaultConversationModel()
				model.ID = m.ID
				model.DisplayName = m.DisplayName
				model.Context = 128000
				model.Output = 16384
				model.Tools = true
				model.Vision = true
				model.Capabilities = m.Capabilities
				if m.Context != nil {
					model.Context = *m.Context
				}
				if m.Output != nil {
					model.Output = *m.Output
				}
				if caps := m.Capabilities; caps != nil {
					if caps.Vision != nil {
						model.Vision = *caps.Vision
					}
					if caps.Tools != nil {
						model.Tools = *caps.Tools
					}
					if caps.Reasoning != nil {
						model.Reasoning = *caps.Reasoning
					}
					if caps.Efforts != nil {
						model.Efforts = *caps.Efforts
					}
					if caps.DefaultEffort != nil {
						model.DefaultEffort = *caps.DefaultEffort
					}
				}
				known[model.ID] = model
				models = append(models, channelModel(model))
			}
			r.finishDiscovery(draft, result.Models, known, editor.providerModels, done)
			return
		}
		done(models)
	})
}
func (r *Runtime) saveCompaction() {
	s := &r.App.Settings.Compaction
	if s.Pending {
		return
	}
	draft := s.Draft
	revision := s.Revision
	s.Pending = true
	r.execute(protocol.Command{Kind: "save_compaction_settings", Data: protocol.ConversationCompactionSettings{Revision: revision, Percent: uint8(draft)}}, func(c *ui.Context, out protocol.Output, err error) {
		// The outcome is a toast wherever the pages are; the slider is this
		// Node's only while they show it.
		shown := r.showsSettings()
		if shown {
			s.Pending = false
		}
		if err != nil {
			r.reportKeysRetry(c, err, "compaction_settings_failed", "compaction_settings_conflict", "compaction_settings_unknown", func() {
				if r.showsSettings() {
					r.saveCompaction()
				}
			})
			return
		}
		if !shown {
			return
		}
		if value, ok := detail[protocol.ConversationCompactionSettings](out.Data); ok {
			s.Saved = float64(value.Percent)
			s.Revision = value.Revision
		} else {
			r.readProviders()
		}
	})
}
