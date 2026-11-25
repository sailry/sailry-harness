package live

import (
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) BindProviderSettings() settings.WindowBinding {
	m := r.App.Settings
	providerEditorHosts := func(draft settings.ProviderDraft) (editor, owner *Runtime) {
		node := draft.Channel.Node
		if draft.ExpectedRevision == 0 && len(draft.Targets) == 1 {
			node = draft.Targets[0]
		}
		editor, owner = r.runtimeFor(draft.Channel.Node), r.providerOwner(node)
		if editor != nil {
			editor.initProviders()
		}
		return editor, owner
	}
	withSelected := func(action func(*Runtime)) {
		if page := r.settingsPage(settings.Providers); page != nil && page.target.Current() && r.settingsSectionVisible(settings.Providers) {
			action(page.owner)
		}
	}
	m.ProviderActions = settings.ProviderActions{
		NewID: func() string { return protocol.NewProviderID().String() },
		Available: func() bool {
			if r.settingsSectionVisible(settings.Providers) {
				page := r.settingsPage(settings.Providers)
				return page != nil && page.target.Current()
			}
			return r.controller.Connected()
		},
		Hosts:         r.providerHosts,
		DefaultTarget: r.providerDefaultTarget,
		CatalogAt:     r.providerCatalogStatus,
		Save:          r.saveManagedProvider,
		Discover: func(draft settings.ProviderDraft, done func([]viewmodel.ChannelModel)) {
			editor, owner := providerEditorHosts(draft)
			if editor == nil || owner == nil {
				done(nil)
				return
			}
			owner.discoverModels(editor, draft, done)
		},
		LookupModel: func(draft settings.ProviderDraft, id string, done func(*viewmodel.ChannelModel)) {
			editor, owner := providerEditorHosts(draft)
			if editor == nil || owner == nil {
				done(nil)
				return
			}
			owner.lookupProviderModel(editor.providerDraft(draft), id, done)
		},
		ReadKey: r.readManagedProviderKey,
		Remove:  r.removeManagedProvider,
		Enable:  r.enableManagedProvider,
		SaveCompaction: func() {
			withSelected(func(owner *Runtime) { owner.saveCompaction() })
		},
		SignIn: func(node, id string) {
			if owner := r.providerOwner(node); owner != nil {
				owner.initProviders()
				owner.beginProviderLogin(id)
			}
		},
		RefreshCatalog: func() {
			withSelected(func(owner *Runtime) { owner.refreshModelCatalog() })
		},
		Usage: r.providerUsage,
	}
	clear := func() {
		m.Channels, m.ChannelsByNode, m.Brands = nil, nil, nil
		m.Catalog = viewmodel.Catalog{}
		m.Compaction = settings.Stored[float64]{}
		m.SetBusy(settings.BusyCatalog, false)
		if r.App.ProviderUI != nil {
			var next string
			if r.settingsSectionVisible(settings.Providers) {
				if owner := r.managedSettingsNode(); owner != nil && owner.controller.Connected() {
					next = owner.controller.Target().String()
				}
			}
			r.App.ProviderUI.CloseProviderEditor(next)
		}
	}
	return settings.WindowBinding{
		KeepOnOutage: true,
		AlsoVisible:  func() bool { return r.App.ProviderUI != nil && r.App.Welcome.Open },
		Blocked:      func(string) { clear() },
		Mount: func(target settings.PageTarget) settings.PageBinding {
			owner := r.runtimeFor(target.Node)
			r.resetProviderReadFailures()
			var entered, projected bool
			var delivered uint64
			return settings.PageBinding{
				Follow: func() {
					visible := r.settingsSectionVisible(settings.Providers)
					if !projected || delivered != r.delivered || entered != visible {
						r.followProviderSettings(target, visible)
						projected, delivered = true, r.delivered
					}
					if visible {
						if !entered {
							owner.compactionObserved = owner.snapshot().CompactionSettingsRevision
							owner.fetchCompaction()
						}
						owner.followCompaction()
						owner.followModelCatalog()
					} else {
						owner.modelCatalogEntered = false
					}
					entered = visible
				},
				Recover: func() {
					projected = false
					r.resetProviderReadFailures()
					if entered {
						owner.fetchCompaction()
					}
				},
				Close: func() {
					owner.modelCatalogEntered = false
					r.forEachProviderSettingsNode(func(node *Runtime) { node.releaseProviderQuotaTimer() })
					clear()
				},
			}
		},
	}
}

func (r *Runtime) forEachProviderSettingsNode(visit func(*Runtime)) {
	visit(r)
	for _, owner := range r.bindings {
		visit(owner)
	}
}

func (r *Runtime) resetProviderReadFailures() {
	r.forEachProviderSettingsNode(func(owner *Runtime) {
		owner.providerPresetsFailed = false
		if state := owner.authorizations; state != nil {
			state.credentialFailed = ""
		}
	})
}

func (r *Runtime) followProviderSettings(target settings.PageTarget, accounts bool) {
	r.forEachProviderSettingsNode(func(owner *Runtime) {
		if !owner.controller.Connected() {
			owner.releaseProviderQuotaTimer()
			return
		}
		owner.initProviders()
		owner.readProviderPresets(target)
		if accounts {
			owner.readProviderCredentials(target)
		}
	})
	r.projectProviderSnapshot()
}

func (r *Runtime) publishProviderSettings() {
	root := r.localRuntime()
	if page := root.settingsPage(settings.Providers); page != nil && page.target.Current() {
		root.projectProviderSnapshot()
	}
}

func (r *Runtime) providerSettingsScope() *Runtime {
	if page := r.settingsPage(settings.Providers); page != nil && r.settingsSectionVisible(settings.Providers) && page.retained() {
		return page.owner
	}
	return nil
}
