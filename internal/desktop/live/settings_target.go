package live

import (
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// settingsOwner is the local Runtime that owns controller configuration.
func (r *Runtime) settingsOwner() *Runtime { return r.localRuntime() }

func (r *Runtime) managedSettingsNode() *Runtime {
	root := r.localRuntime()
	owner := root.runtimeFor(root.App.Settings.ManagedNode)
	if owner != nil && (owner == root || owner.controller.Connected()) {
		return owner
	}
	return nil
}

func (r *Runtime) openNodeSettings(section settings.Section) {
	root := r.localRuntime()
	target := ""
	if r != root {
		target = r.controller.Target().String()
	}
	root.App.Settings.ManagedNode = target
	root.App.Settings.Open(section)
	root.App.Navigate(shell.SettingsPage)
}

// machineSettingsStatus follows the Node's administrator boundary for
// settings shared by every user of that machine.
func (r *Runtime) machineSettingsStatus() string {
	if r == nil {
		return "connections_offline"
	}
	if r.parent != nil {
		if !r.controller.Connected() {
			return "connections_offline"
		}
		if user := r.snapshot().User; user == nil || !user.Administrator {
			return "permission_denied"
		}
	}
	return ""
}

func (r *Runtime) nodeSettingsOwner() *Runtime {
	root := r.localRuntime()
	if root.App.Page != shell.SettingsPage {
		return root
	}
	section := root.App.Settings.Section
	switch section {
	case settings.General, settings.Conversation, settings.Providers, settings.Teams, settings.Connectors, settings.Media:
		owner := root.managedSettingsNode()
		if owner == nil && section == settings.Providers {
			bound := root.shownSettings()
			// A temporary outage of the same host leaves the editor and its
			// pending save in place. Other bindings must open their own drafts.
			if bound != nil && root.runtimeFor(root.App.Settings.ManagedNode) == bound {
				switch faultCode(bound.controller.Err()) {
				case protocol.ErrorCodeUnavailable, protocol.ErrorCodeBusy:
					return bound
				}
			}
		}
		return owner
	}
	return root
}

// shownSettings is the Runtime currently bound to the configuration pages.
// Its host selection is independent of the device monitor.
func (r *Runtime) shownSettings() *Runtime {
	root := r.localRuntime()
	if root.settingsGeneration == 0 {
		return root
	}
	return root.settingsShown
}

// showsSettings reports whether the pages hold this Runtime's Node.
func (r *Runtime) showsSettings() bool { return r.shownSettings() == r }

// settingsCurrent checks whether a response still belongs to the same
// binding, including when the user switches away and back before it arrives.
func (r *Runtime) settingsCurrent() func() bool {
	root := r.localRuntime()
	generation := root.settingsGeneration
	return func() bool { return root.settingsGeneration == generation && r.showsSettings() }
}

// initNodeSettings builds the pages' actions once, on the local Runtime.
// Each runs on the Node shown when it is called; with no Node shown a save
// answers that it did not happen.
func (r *Runtime) initNodeSettings() {
	on := func(act func(owner *Runtime)) bool {
		owner := r.shownSettings()
		if owner == nil || !owner.controller.Connected() {
			r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the settings host is unavailable"))
			return false
		}
		act(owner)
		return true
	}
	m := r.App.Settings
	onMachine := func(act func(owner *Runtime)) {
		on(func(owner *Runtime) {
			if owner.machineSettingsStatus() == "" {
				act(owner)
			}
		})
	}
	providerEditorHosts := func(draft settings.ProviderDraft) (editor, owner *Runtime) {
		node := draft.Channel.Node
		if draft.ExpectedRevision == 0 && len(draft.Targets) == 1 {
			node = draft.Targets[0]
		}
		return r.runtimeFor(draft.Channel.Node), r.providerOwner(node)
	}
	m.SavePower = func() { onMachine(func(owner *Runtime) { owner.savePower() }) }
	m.SaveMedia = func() { on(func(owner *Runtime) { owner.saveMedia() }) }
	m.SaveCodeIndex = func() { onMachine(func(owner *Runtime) { owner.saveCodeIndex() }) }
	m.ProviderActions = settings.ProviderActions{
		NewID: func() string { return protocol.NewProviderID().String() },
		Available: func() bool {
			owner := r.localRuntime()
			if owner.App.Page == shell.SettingsPage && owner.App.Settings.Section == settings.Providers {
				owner = owner.managedSettingsNode()
			}
			return owner != nil && owner.controller.Connected()
		},
		Hosts:         r.providerHosts,
		DefaultTarget: r.providerDefaultTarget,
		CatalogAt:     r.providerCatalogStatus,
		Save: func(draft settings.ProviderDraft, done func(settings.ProviderSaveResult)) {
			if root := r.localRuntime(); root != nil {
				root.saveManagedProvider(draft, done)
			} else {
				done(providerSaveFailure(draft.Targets))
			}
		},
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
		ReadKey: func(node, id string, done func(string)) {
			r.localRuntime().readManagedProviderKey(node, id, done)
		},
		Remove: func(node, id string, revision uint64) {
			r.localRuntime().removeManagedProvider(node, id, revision)
		},
		Enable: func(node, id string, revision uint64, enabled bool) {
			r.localRuntime().enableManagedProvider(node, id, revision, enabled)
		},
		SaveCompaction: func() { on(func(owner *Runtime) { owner.saveCompaction() }) },
		SignIn: func(node, id string) {
			if owner := r.providerOwner(node); owner != nil {
				owner.beginProviderLogin(id)
			}
		},
		RefreshCatalog: func() { on(func(owner *Runtime) { owner.refreshModelCatalog() }) },
		Usage:          r.providerUsage,
	}
	m.TeamActions = &settings.TeamActions{
		SaveConcurrency: func() { on(func(owner *Runtime) { owner.saveConcurrency() }) },
		SaveTeam: func(team viewmodel.Team, done func(kit.Outcome)) {
			if !on(func(owner *Runtime) { owner.saveTeam(team, done) }) {
				done(kit.Failed)
			}
		},
		SaveRole: func(role viewmodel.Role, done func(kit.Outcome)) {
			if !on(func(owner *Runtime) { owner.saveRole(role, done) }) {
				done(kit.Failed)
			}
		},
		RemoveTeam: func(team viewmodel.Team, done func(kit.Outcome)) {
			if !on(func(owner *Runtime) { owner.removeTeam(team, done) }) {
				done(kit.Failed)
			}
		},
		RemoveRole: func(role viewmodel.Role, done func(kit.Outcome)) {
			if !on(func(owner *Runtime) { owner.removeRole(role, done) }) {
				done(kit.Failed)
			}
		},
	}
	m.ConnectorActions = &settings.ConnectorActions{
		Save: func(draft settings.ConnectorDraft, done func(bool)) {
			if !on(func(owner *Runtime) { owner.saveConnector(draft, done) }) && done != nil {
				done(false)
			}
		},
		Enable:  func(id string, enabled bool) { on(func(owner *Runtime) { owner.enableConnector(id, enabled) }) },
		Remove:  func(id string) { on(func(owner *Runtime) { owner.removeConnector(id) }) },
		Confirm: func(id string) { on(func(owner *Runtime) { owner.confirmConnector(id) }) },
		Probe:   func(id string) { on(func(owner *Runtime) { owner.probeConnection(id) }) },
		Reconnect: func(id string) {
			on(func(owner *Runtime) { owner.reconnectMCP(id) })
		},
		InspectCLI: func(id string, done func([]string)) {
			if !on(func(owner *Runtime) { owner.inspectConnectorCLI(id, done) }) {
				done(nil)
			}
		},
		SaveMCP: func(draft settings.MCPDraft, done func(bool)) {
			if !on(func(owner *Runtime) { owner.saveMCP(draft, done) }) {
				done(false)
			}
		},
		AllowMCP: func(id, server string, allowed bool) {
			on(func(owner *Runtime) { owner.allowMCP(id, server, allowed) })
		},
		Submit: func(callback string) { on(func(owner *Runtime) { owner.submitConnectorLogin(callback) }) },
		Cancel: func() { on(func(owner *Runtime) { owner.cancelConnectorLogin() }) },
		Retry: func() {
			on(func(owner *Runtime) {
				if previous := owner.authorizations.connector; previous != nil {
					owner.saveConnector(previous.connector, nil)
				}
			})
		},
		ConfigureMCP: func(id string) { on(func(owner *Runtime) { owner.configureConnectorMCP(id) }) },
		Busy: func(id string) bool {
			owner := r.shownSettings()
			return owner != nil && owner.authorizations.connectorBusy[id]
		},
		Help: func(id, name string) string {
			owner := r.shownSettings()
			if owner == nil {
				return ""
			}
			for _, field := range owner.authorizations.types[id].Declaration.Fields {
				if field.Name == name && field.Help != nil {
					return *field.Help
				}
			}
			return ""
		},
	}
	r.enterNodeSettings()
}

// nodeSettingsFrame binds the pages to their selected owner and follows
// what that Node reports: its roles and teams, and newer power, compaction
// and team settings.
func (r *Runtime) nodeSettingsFrame() {
	root := r.localRuntime()
	owner := root.nodeSettingsOwner()
	if owner != root.shownSettings() {
		root.showNodeSettings(owner)
	}
	m := root.App.Settings
	status := owner.machineSettingsStatus()
	allowedNow := status == "" && m.ExecutionSettingsStatus != ""
	m.ExecutionSettingsStatus = status
	if owner == nil || owner != root && !owner.controller.Connected() {
		return
	}
	owner.teamsFrame()
	owner.followConnectorPackages()
	if status == "" {
		if allowedNow || owner.snapshot().PowerSettingsRevision != owner.powerObservedRevision && !m.Power.Pending {
			owner.readPower()
		}
		if allowedNow {
			owner.readCodeIndex()
		}
	}
	owner.followCompaction()
	owner.followModelCatalog()
}

// showNodeSettings turns the pages to owner's Node, or to none: the Node
// they held ends its sign-ins, the pages' values, drafts and editors are
// dropped, and owner's are read.
func (r *Runtime) showNodeSettings(owner *Runtime) {
	root := r.localRuntime()
	if previous := root.shownSettings(); previous != nil {
		previous.leaveNodeSettings()
	}
	root.settingsShown = owner
	root.settingsGeneration++
	m := root.App.Settings
	m.CloseManagedNodeDialogs()
	var nextNode string
	if owner != nil {
		nextNode = owner.controller.Target().String()
	}
	m.CloseProviderEditor(nextNode)
	// Without a Node the controls wait, as the other host pages do.
	waiting := owner == nil
	m.Power, m.Compaction, m.Concurrency = settings.Stored[bool]{Pending: waiting}, settings.Stored[float64]{Pending: waiting}, settings.Stored[float64]{Pending: waiting}
	m.Media, m.MediaOptions, m.MediaUnavailable = settings.Stored[[4]string]{Pending: waiting}, nil, [4]bool{}
	m.CodeIndex = settings.Stored[bool]{Pending: waiting}
	m.Roles, m.Teams = nil, nil
	m.Services, m.ConnTypes, m.ConnAccounts = nil, nil, nil
	if actions := m.ConnectorActions; actions != nil {
		actions.Authorization, actions.Consent = settings.Authorization{}, nil
	}
	m.SetBusy(settings.BusyCatalog, false)
	if owner != nil {
		owner.enterNodeSettings()
	} else {
		root.projectProviderSnapshot()
	}
}

// enterNodeSettings reads this Runtime's Node for the pages that now show it.
func (r *Runtime) enterNodeSettings() {
	r.initProviders()
	if r.teams == nil {
		r.teams = newTeamState()
	}
	r.media.seen = ""
	if r.machineSettingsStatus() == "" {
		r.readPower()
		r.readCodeIndex()
	}
	r.readProviders()
	r.teamsFrame()
	r.readTeamSettings()
	r.projectConnectors()
	r.readConnectorPackages()
}

// leaveNodeSettings ends what only the pages could finish on this Node: a
// connector sign-in under way and the view of a concurrency save in doubt.
// A command already sent still ends in its toast.
func (r *Runtime) leaveNodeSettings() {
	r.modelCatalogEntered = false
	if state := r.authorizations; state != nil {
		if attempt := state.connector; attempt != nil {
			r.cancelConnectorLogin()
			attempt.cancel()
			state.connector = nil
		}
	}
	if r.teams != nil {
		r.teams.settingsUnknown = nil
	}
}
