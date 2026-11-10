package settings

import "github.com/sailry/sailry-harness/internal/desktop/viewmodel"

type ConnectorDraft struct {
	Type, Service, Method string
	ExpectedRevision      uint64
	Fields                map[string]string
	Registration          bool
}

type MCPDraft struct {
	Service, Name string
	Revision      uint64
	Server        viewmodel.ConnectorServer
}

type ConnectorActions struct {
	Authorization Authorization
	Consent       *ConnectorConsent
	Save          func(ConnectorDraft, func(bool))
	Enable        func(string, bool)
	Remove        func(string)
	Confirm       func(string)
	ConfigureMCP  func(string)
	Probe         func(string)
	Reconnect     func(string)
	InspectCLI    func(string, func([]string))
	Help          func(string, string) string
	SaveMCP       func(MCPDraft, func(bool))
	AllowMCP      func(string, string, bool)
	Submit        func(string)
	Cancel        func()
	Retry         func()
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

func (m *Model) OpenMCPConfiguration(service viewmodel.ConnectorService) {
	m.editServer(&service)
}
