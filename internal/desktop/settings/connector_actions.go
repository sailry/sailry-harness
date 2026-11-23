package settings

import "github.com/sailry/sailry-harness/internal/desktop/viewmodel"

// ConnectorDraft is a connector's editor as it is submitted. Update saves the
// changed fields of an existing connection and authorizes again only when
// the Node drops its authorization for them.
type ConnectorDraft struct {
	Type, Service, Method string
	ExpectedRevision      uint64
	CLIAccount            string
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

// ConnectorPageState receives configuration results and closes editors on a Node change.
type ConnectorPageState interface {
	OpenMCPConfiguration(MCPConfiguration)
	FailMCPConfiguration(id, detail string)
	CloseDialogs()
}

func (m *Model) OpenMCPConfiguration(configuration MCPConfiguration) {
	if page := m.ConnectorPageState; page != nil {
		page.OpenMCPConfiguration(configuration)
	}
}

func (m *Model) FailMCPConfiguration(id, detail string) {
	if page := m.ConnectorPageState; page != nil {
		page.FailMCPConfiguration(id, detail)
	}
}

// CloseManagedNodeDialogs drops editors bound to the previous settings owner.
func (m *Model) CloseManagedNodeDialogs() {
	if page := m.ConnectorPageState; page != nil {
		page.CloseDialogs()
	}
	if m.TeamPageState != nil {
		m.TeamPageState.CloseDialogs()
	}
	if m.SearchPageState != nil {
		m.SearchPageState.CloseDialogs()
	}
	if m.MemoryPageState != nil {
		m.MemoryPageState.CloseDialogs()
	}
}
