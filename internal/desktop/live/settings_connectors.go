package live

import "github.com/sailry/sailry-harness/internal/desktop/settings"

func (r *Runtime) BindConnectorSettings() settings.WindowBinding {
	m := r.App.Settings
	return settings.WindowBinding{Mount: func(target settings.PageTarget) settings.PageBinding {
		owner := r.runtimeFor(target.Node)
		if owner.authorizations == nil {
			owner.initAuthorization()
		}
		state := owner.authorizations
		on := func(act func()) bool {
			if !target.Current() {
				return false
			}
			act()
			return true
		}
		m.ConnectorActions = &settings.ConnectorActions{
			Save: func(draft settings.ConnectorDraft, done func(bool)) {
				if !on(func() { owner.saveConnector(draft, done) }) && done != nil {
					done(false)
				}
			},
			Enable:    func(id string, enabled bool) { on(func() { owner.enableConnector(id, enabled) }) },
			Remove:    func(id string) { on(func() { owner.removeConnector(id) }) },
			Confirm:   func(id string) { on(func() { owner.confirmConnector(id) }) },
			Probe:     func(id string) { on(func() { owner.probeConnection(id) }) },
			Reconnect: func(id string) { on(func() { owner.reconnectMCP(id) }) },
			InspectCLI: func(id string, done func([]string)) {
				if !on(func() { owner.inspectConnectorCLI(id, done) }) {
					done(nil)
				}
			},
			SaveMCP: func(draft settings.MCPDraft, done func(bool)) {
				if !on(func() { owner.saveMCP(draft, done) }) {
					done(false)
				}
			},
			AllowMCP: func(id, server string, allowed bool) { on(func() { owner.allowMCP(id, server, allowed) }) },
			Submit:   func(callback string) { on(func() { owner.submitConnectorLogin(callback) }) },
			Cancel:   func() { on(owner.cancelConnectorLogin) },
			Retry: func() {
				on(func() {
					if previous := state.connector; previous != nil {
						owner.saveConnector(previous.connector, nil)
					}
				})
			},
			ConfigureMCP: func(id string) { on(func() { owner.configureConnectorMCP(id) }) },
			Busy:         func(id string) bool { return target.Current() && state.connectorBusy[id] },
			Help: func(id, name string) string {
				if target.Current() {
					for _, field := range state.types[id].Declaration.Fields {
						if field.Name == name && field.Help != nil {
							return *field.Help
						}
					}
				}
				return ""
			},
		}
		owner.observeSettingsConnections(target)
		owner.readConnectorPackages()
		return settings.PageBinding{
			Follow: owner.followConnectorPackages,
			Recover: func() {
				if state.connectionsFailed {
					owner.observeSettingsConnections(target)
				}
			},
			Close: func() {
				if attempt := state.connector; attempt != nil {
					owner.cancelConnectorLogin()
					attempt.cancel()
					state.connector = nil
				}
				if page := m.ConnectorPageState; page != nil {
					page.CloseDialogs()
				}
				m.ConnectorActions = nil
				m.Services, m.ConnTypes, m.ConnAccounts = nil, nil, nil
				clear(state.types)
				clear(state.connections)
				clear(state.mcpDrafts)
			},
		}
	}}
}
