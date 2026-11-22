package settings

import "github.com/sailry/sailry-harness/internal/desktop/viewmodel"

type ConnectionDraft struct {
	Method                                    Method
	Relay                                     int
	Address, Code, Name, Host, Port, Username string
	Password, Key, Passphrase                 string
	Scope                                     int    // a sharing scope
	Projects                                  string // shared project IDs, joined by commas
	Distribution                              string // WSL
}

type ConnectionActions struct {
	// Inspect opens a device monitor; it does not select a configuration Node.
	Inspect     func(viewmodel.Device)
	Add         func(ConnectionDraft, func(bool))
	CheckStatus func(func(bool))
	Revoke      func(viewmodel.Device)
	Narrow      func(viewmodel.Device, string)
	EditNetwork func(viewmodel.Device)
	SaveNetwork func(*NetworkEditor)
	BrowseKey   func(func(string))
	RetryShare  func()
}

type ShareState struct {
	Kind, Code string
}

type RelayChoice struct {
	Label, LabelKey string
	Custom, Pending bool
	Millis          *uint64
}

type NetworkEditor struct {
	Device      viewmodel.Device
	Revision    uint64
	Relay       int
	Address     string
	Relays      []RelayChoice
	Loading     bool
	Pending     bool
	Locked      bool
	Retry       bool
	Unavailable bool
}

type HostTrust struct {
	Host, Algorithm, Fingerprint string
	Changed                      bool
	Confirm                      func()
}

func (m *Model) ConnectionDraft() ConnectionDraft {
	form := &m.pairing
	return ConnectionDraft{
		Method: form.Method, Relay: form.Relay, Address: form.Address, Code: form.Code,
		Name: form.Name, Host: form.Host, Port: form.Port, Username: form.Username,
		Password: form.Password, Key: form.Key, Passphrase: form.Passphrase,
		Scope: form.Scope, Projects: form.sharedProjects(), Distribution: form.distributionName(),
	}
}

func (m *Model) Sharing() bool { return m.pairing.ShareOpen }

func (m *Model) Adding() bool { return m.pairing.AddOpen }

func (m *Model) CloseShare() { m.pairing.ShareOpen = false }

func (m *Model) CloseConnectionDialogs() {
	m.pairing.AddOpen, m.pairing.ShareOpen = false, false
	m.NetworkEditor = nil
}

func (m *Model) SetConnectionNetwork(relay int, address string) {
	m.pairing.Relay, m.pairing.Address = relay, address
}
