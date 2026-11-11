package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type ConnectionDraft struct {
	Method                                    Method
	Relay                                     int
	Address, Code, Name, Host, Port, Username string
	Password, Key, Passphrase                 string
}

type ConnectionActions struct {
	Add         func(ConnectionDraft, func(bool))
	Revoke      func(viewmodel.Device)
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
	value := m.pairing
	return ConnectionDraft{Method: Method(value.method), Relay: value.relay, Address: value.address, Code: value.code, Name: value.name, Host: value.host, Port: value.port, Username: value.user, Password: value.secret, Key: value.key, Passphrase: value.passphrase}
}

func (m *Model) Sharing() bool { return m.pairing.sharing }

func (m *Model) Adding() bool { return m.pairing.adding }

func (m *Model) CloseShare() { m.pairing.sharing = false }

func (m *Model) CloseConnectionDialogs() {
	m.pairing.adding, m.pairing.sharing = false, false
	m.NetworkEditor = nil
}

func (m *Model) SetConnectionNetwork(relay int, address string) {
	m.pairing.relay, m.pairing.address = relay, address
}

func connectionDialogs(c *ui.Context, m *Model) {
	if editor := m.NetworkEditor; editor != nil {
		open := true
		title := i18n.T(c, "host_network_edit")
		kit.Dialog(c.Key("network-editor"), &open, func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			ui.Text(c, deviceName(c, editor.Device)).FontSize(14).TextColor(theme.Of(c).MutedForeground)
			if editor.Loading {
				kit.Shimmer(c, i18n.T(c, "host_runtime_loading"), 14, theme.Of(c).MutedForeground)
			} else {
				draft := pairing{relay: editor.Relay, address: editor.Address}
				connectionRelayFields(c, &draft, m.Relays, editor.Relays, editor.Pending || editor.Locked || editor.Unavailable)
				editor.Relay, editor.Address = draft.relay, draft.address
			}
			ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
				kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).Disabled(editor.Pending).OnClick(func() { m.NetworkEditor = nil })
				label := i18n.T(c, "settings_save")
				if editor.Retry {
					label = i18n.T(c, "settings_retry")
				}
				kit.Primary(c.Key("save"), label, false).Disabled(editor.Loading || editor.Pending || editor.Unavailable).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() {
					if m.ConnectionActions != nil && m.ConnectionActions.SaveNetwork != nil {
						m.ConnectionActions.SaveNetwork(editor)
					}
				})
			})
		}).Width(min(420, width(c)-48)).Gap(16).Label(title)
		if !open {
			m.NetworkEditor = nil
		}
	}
	if trust := m.HostTrust; trust != nil {
		open := true
		title := i18n.T(c, "ssh_trust_title")
		if trust.Changed {
			title = i18n.T(c, "ssh_key_changed")
		}
		body := trust.Host + "\n" + trust.Algorithm + "\n" + trust.Fingerprint
		if kit.AlertDialog(c.Key("host-key"), &open, title, body, i18n.T(c, "settings_cancel"), i18n.T(c, "ssh_trust")) == 1 {
			m.HostTrust = nil
			trust.Confirm()
		}
		if !open {
			m.HostTrust = nil
			m.ConnectionPending = false
		}
	}
}
