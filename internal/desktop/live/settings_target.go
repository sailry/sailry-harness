package live

import (
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settings edits one Node: the host selected in the shell (App.Host, the
// local Node by default), as the reference binds every page to its settings
// Node. Prevent sleep, providers, the compaction threshold, media models,
// teams, roles, child concurrency, connectors, MCP, the sandbox, the code
// index and hooks are that Node's, beside the terminal, backup, local model, plugin and
// Python pages. Language, appearance, shortcuts, notifications, dictation
// and telemetry consent are this desktop's and stay local.
//
// The pages share one view model, so only the Runtime shown writes it: an
// answer for a Node the pages left is dropped, or is a toast alone when it
// ends a save. Turning to another host drops the pages' drafts and editors
// and reads the new Node.

// settingsOwner is the Runtime of the Node Settings edits, nil while the
// selected host has no connection.
func (r *Runtime) settingsOwner() *Runtime {
	root := r.localRuntime()
	return root.runtimeFor(root.App.Host)
}

// shownSettings is the Runtime whose Node the pages hold: the local one
// until the pages turn to another host, nil for a host without a connection.
func (r *Runtime) shownSettings() *Runtime {
	root := r.localRuntime()
	if !root.settingsTurned {
		return root
	}
	return root.settingsShown
}

// showsSettings reports whether the pages hold this Runtime's Node.
func (r *Runtime) showsSettings() bool { return r.shownSettings() == r }

// initNodeSettings builds the pages' actions once, on the local Runtime.
// Each runs on the Node shown when it is called; with no Node shown a save
// answers that it did not happen.
func (r *Runtime) initNodeSettings() {
	on := func(act func(owner *Runtime)) bool {
		owner := r.shownSettings()
		if owner == nil {
			r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the settings host is unavailable"))
			return false
		}
		act(owner)
		return true
	}
	m := r.App.Settings
	m.SavePower = func() { on(func(owner *Runtime) { owner.savePower() }) }
	m.SaveMedia = func() { on(func(owner *Runtime) { owner.saveMedia() }) }
	m.SaveCodeIndex = func() { on(func(owner *Runtime) { owner.saveCodeIndex() }) }
	m.ProviderActions = settings.ProviderActions{
		NewID: func() string { return protocol.NewProviderID().String() },
		Available: func() bool {
			owner := r.shownSettings()
			return owner != nil && owner.controller.Connected()
		},
		Save: func(draft settings.ProviderDraft, done func(bool)) {
			if !on(func(owner *Runtime) { owner.saveProvider(draft, done) }) {
				done(false)
			}
		},
		Discover: func(draft settings.ProviderDraft, done func([]viewmodel.ChannelModel)) {
			if !on(func(owner *Runtime) { owner.discoverModels(draft, done) }) {
				done(nil)
			}
		},
		ReadKey: func(id string, done func(bool)) {
			if !on(func(owner *Runtime) { owner.readProviderKey(id, done) }) {
				done(false)
			}
		},
		Remove: func(id string, revision uint64) {
			on(func(owner *Runtime) { owner.removeProvider(id, revision) })
		},
		Enable: func(id string, revision uint64, enabled bool) {
			on(func(owner *Runtime) { owner.enableProvider(id, revision, enabled) })
		},
		SaveCompaction: func() { on(func(owner *Runtime) { owner.saveCompaction() }) },
		SignIn:         func(id string) { on(func(owner *Runtime) { owner.beginProviderLogin(id) }) },
		SubmitLogin:    func(callback string) { on(func(owner *Runtime) { owner.submitProviderLogin(callback) }) },
		CancelLogin:    func() { on(func(owner *Runtime) { owner.cancelProviderLogin() }) },
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

// nodeSettingsFrame keeps the pages on the selected host's Node and follows
// what that Node reports: its roles and teams, and newer power, compaction
// and team settings.
func (r *Runtime) nodeSettingsFrame() {
	root := r.localRuntime()
	owner := root.settingsOwner()
	if owner != root.shownSettings() {
		root.showNodeSettings(owner)
	}
	if owner == nil {
		return
	}
	owner.teamsFrame()
	if owner.snapshot().PowerSettingsRevision != owner.powerObservedRevision && !root.App.Settings.Power.Pending {
		owner.readPower()
	}
	owner.followCompaction()
}

// showNodeSettings turns the pages to owner's Node, or to none: the Node
// they held ends its sign-ins, the pages' values, drafts and editors are
// dropped, and owner's are read.
func (r *Runtime) showNodeSettings(owner *Runtime) {
	root := r.localRuntime()
	if previous := root.shownSettings(); previous != nil {
		previous.leaveNodeSettings()
	}
	root.settingsShown, root.settingsTurned = owner, true
	m := root.App.Settings
	m.CloseNodeDialogs()
	// Without a Node the controls wait, as the other host pages do.
	waiting := owner == nil
	m.Power, m.Compaction, m.Concurrency = settings.Stored[bool]{Pending: waiting}, settings.Stored[float64]{Pending: waiting}, settings.Stored[float64]{Pending: waiting}
	m.Media, m.MediaOptions, m.MediaUnavailable = settings.Stored[[4]string]{Pending: waiting}, nil, [4]bool{}
	m.CodeIndex = settings.Stored[bool]{Pending: waiting}
	m.Channels, m.Brands, m.Catalog = nil, nil, viewmodel.Catalog{}
	m.Roles, m.Teams = nil, nil
	m.Services, m.ConnTypes, m.ConnAccounts = nil, nil, nil
	if actions := m.ConnectorActions; actions != nil {
		actions.Authorization, actions.Consent = settings.Authorization{}, nil
	}
	m.SetBusy(settings.BusyCatalog, false)
	if owner != nil {
		owner.enterNodeSettings()
	}
}

// enterNodeSettings reads this Runtime's Node for the pages that now show it.
func (r *Runtime) enterNodeSettings() {
	r.initProviders()
	if r.teams == nil {
		r.teams = newTeamState()
	}
	r.media.seen = ""
	r.readPower()
	r.readCodeIndex()
	r.readProviders()
	r.teamsFrame()
	r.readTeamSettings()
	r.projectConnectors()
	r.readConnectorPackages()
}

// leaveNodeSettings ends what only the pages could finish on this Node: a
// provider or connector sign-in under way, and a concurrency save in doubt.
// A command already sent still ends in its toast.
func (r *Runtime) leaveNodeSettings() {
	if state := r.authorizations; state != nil {
		if attempt := state.provider; attempt != nil {
			r.cancelProviderLogin()
			attempt.cancel()
			state.provider = nil
		}
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
