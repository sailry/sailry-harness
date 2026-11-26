package settings

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Method is how a host is added.
type Method int

const (
	ByPairing Method = iota
	ByPassword
	ByPrivateKey
	ByKeyFile
	ByAgent
	ByWSL
)

// SSH is whether the method logs in over SSH.
func (method Method) SSH() bool { return method != ByPairing && method != ByWSL }

// Sharing scopes of an SSH host's saved connection.
const (
	ScopePrivate = iota
	ScopeGlobal
	ScopeProjects
)

// ProjectChoice is a project a saved connection can be shared with.
type ProjectChoice struct{ ID, Name string }

// ConnectionStatus is what the backend reports of the page's operations
// beyond the dialog's own draft.
type ConnectionStatus struct {
	// Recovery is an installation whose outcome is uncertain: the dialog
	// locks its draft and offers Check status instead of Install.
	Recovery bool
	// Saved is the authentication of the connection this dialog already
	// saved, ByPairing for none: its secret left blank keeps the saved one.
	Saved Method
	// Projects are the local Node's projects, for the sharing scope.
	Projects []ProjectChoice
	// Distributions are the WSL distributions a Host can be installed in.
	Distributions                             []string
	DistributionsLoading, DistributionsLoaded bool
	// SSHUnavailable hides SSH Host installation when the local product
	// assembly does not ship an SSH module.
	SSHUnavailable bool
	// Revoking are the devices whose pairing is being removed, by ID.
	Revoking  map[string]bool
	Narrowing map[string]bool
}

// SetConnectionStatus is the backend's report, each frame.
func (m *Model) SetConnectionStatus(status ConnectionStatus) { m.pairing.Status = status }

// ConnectionForm holds the editable pairing and sharing fields used by the
// Connections page and projected to the live adapter through ConnectionDraft.
type ConnectionForm struct {
	AddOpen, ShareOpen                        bool
	Method, Authentication                    Method
	Relay                                     int
	Address, Code, Name, Host, Port, Username string
	Password, Key, Passphrase                 string
	Scope                                     int
	SharedProjects                            map[string]*bool
	DistributionChoice                        int
	Revoking                                  *viewmodel.Device
	Status                                    ConnectionStatus
}

type pairing = ConnectionForm

func (m *Model) ConnectionForm() *ConnectionForm { return &m.pairing }

func (m *Model) ConnectionGeneration() uint64 { return m.connectionGeneration }

// sharedProjects are the offered projects the draft shares with, joined by
// commas in the offered order.
func (pr *ConnectionForm) sharedProjects() string {
	if pr.Scope != ScopeProjects {
		return ""
	}
	var ids []string
	for _, project := range pr.Status.Projects {
		if on := pr.SharedProjects[project.ID]; on != nil && *on {
			ids = append(ids, project.ID)
		}
	}
	return strings.Join(ids, ",")
}

// distributionName is the chosen WSL distribution, "" for none.
func (pr *ConnectionForm) distributionName() string {
	if pr.DistributionChoice < 0 || pr.DistributionChoice >= len(pr.Status.Distributions) {
		return ""
	}
	return pr.Status.Distributions[pr.DistributionChoice]
}

// AddHost opens the Add host dialog over the Connections page.
func (s *State) AddHost() {
	s.connectionGeneration++
	s.pairing = ConnectionForm{Port: "22", AddOpen: true, Status: s.pairing.Status}
}
