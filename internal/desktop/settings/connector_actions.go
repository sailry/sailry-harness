package settings

import "github.com/sailry/sailry-harness/internal/desktop/viewmodel"

// ConnectorDraft is a connector's editor as it is submitted. Update saves the
// changed fields of an existing connection and authorizes again only when
// the Node drops its authorization for them.
type ConnectorDraft struct {
	Type, Service, Method string
	ExpectedRevision      uint64
	Fields                map[string]string
	Registration          bool
	Update                bool
}

// MCPDraft is a server's editor as it is saved. A slot with a value replaces
// its credential, one named in Keep keeps the saved one, and any other is
// cleared: an empty value never replaces a saved credential. Failed, when
// set, is told before the completion how a save that did not go through
// ended: uncertain, and the host's reason.
type MCPDraft struct {
	Service, Name string
	Revision      uint64
	Server        viewmodel.ConnectorServer
	Keep          []string
	Failed        func(uncertain bool, detail string)
}

// MCPConfiguration is a saved server as its editor opens on it: the service
// with its one server, the slots holding a saved credential, and Declared
// for a plugin's own server, whose name and transport are the package's.
type MCPConfiguration struct {
	Service  viewmodel.ConnectorService
	Saved    []string
	Declared bool
}

type ConnectorActions struct {
	Authorization Authorization
	Consent       *ConnectorConsent
	Save          func(ConnectorDraft, func(bool))
	Enable        func(string, bool)
	Remove        func(string)
	Confirm       func(string)
	// ConfigureMCP reads the service's saved MCP configuration and answers
	// with OpenMCPConfiguration or FailMCPConfiguration; a package that
	// declares a settings form opens that form instead.
	ConfigureMCP func(string)
	Probe        func(string)
	Reconnect    func(string)
	InspectCLI   func(string, func([]string))
	Help         func(string, string) string
	SaveMCP      func(MCPDraft, func(bool))
	AllowMCP     func(string, string, bool)
	Submit       func(string)
	Cancel       func()
	Retry        func()
	// Busy reports a command of the service still under way.
	Busy func(string) bool
}

type ConnectorConsent struct {
	Open    bool
	Target  string
	Changes []string
	Apply   func(bool)
}

func (m *Model) ConfirmConnectorConsent(target string, changes []string, apply func(bool)) {
	if m.ConnectorActions != nil {
		m.ConnectorActions.Consent = &ConnectorConsent{Open: true, Target: target, Changes: changes, Apply: apply}
	}
}

// OpenMCPConfiguration shows the saved configuration in the server's editor.
// Only the editor waiting for this service takes it: one closed while it
// waited, or opened on another service since, is left as it is.
func (m *Model) OpenMCPConfiguration(configuration MCPConfiguration) {
	e := &m.mcp
	if !e.loading || e.original != configuration.Service.ID || !e.open && !e.deferred {
		return
	}
	if len(configuration.Service.Servers) == 0 {
		e.loading = false
		return
	}
	m.editServer(&configuration.Service, configuration.Saved)
	m.mcp.declared = configuration.Declared
}

// FailMCPConfiguration leaves the editor waiting for the service unloaded,
// with the reason for its diagnostics: Reload reads again.
func (m *Model) FailMCPConfiguration(id, detail string) {
	if m.mcp.loading && m.mcp.original == id {
		m.mcp.loading, m.mcp.unread, m.mcp.detail = false, true, detail
	}
}

// CloseNodeDialogs closes the editors, sign-ins and removal prompts of the
// provider, connector and team pages, as when Settings turns to another
// host: their drafts belong to the Node they were opened on.
func (m *Model) CloseNodeDialogs() {
	m.provider = providerEditor{generation: m.provider.generation + 1}
	m.login, m.loginFor, m.removing, m.expanded = Authorization{}, "", "", ""
	m.loginWarning = loginWarning{}
	m.connector, m.mcp, m.connDetails, m.connRemoving = connectorEditor{}, mcpEditor{}, "", ""
	m.teamEdit, m.roleEdit, m.teamRemoving, m.roleRemoving = teamEditor{}, roleEditor{}, "", ""
}
