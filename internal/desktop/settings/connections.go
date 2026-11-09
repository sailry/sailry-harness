package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Method is how a host is added.
type Method int

const (
	ByPairing Method = iota
	ByPassword
	ByPrivateKey
	ByKeyFile
)

var methods = []string{"host_pairing_mode", "ssh_password", "ssh_private_key", "ssh_key_file"}

// pairing is the Connections page's dialogs: adding a host, sharing this
// computer's code, and the pairing being revoked. Pairing, installing over
// SSH and sharing reach the network; the preview stops at the notice.
type pairing struct {
	adding, sharing         bool
	relay                   int // of relayOptions
	address                 string
	method                  int
	code                    string
	name, host, port, user  string
	secret, key, passphrase string
	revoking                *preview.Device
}

// relayOptions are the connection servers: the public network, the named
// relays, and a custom address.
func relayOptions(c *ui.Context, relays []string) []string {
	return append(append([]string{i18n.T(c, "pairing_relay_iroh")}, relays...), i18n.T(c, "pairing_relay_custom"))
}

// connections is the hosts, with sharing and adding one, and the mobile
// controllers.
func connections(c *ui.Context, m *Model) {
	pr := &m.pairing
	kit.Group(c, "hosts", i18n.T(c, "connections_hosts"), "", func() {
		kit.Secondary(c.Key("share"), i18n.T(c, "pairing_share")).OnClick(func() { pr.sharing = true })
		label := i18n.T(c, "settings_add")
		kit.Primary(c.Key("add"), label, false).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() {
			*pr = pairing{adding: true, port: "22"}
		})
	}, func() { devices(c, m, true) })
	kit.Group(c, "mobile", i18n.T(c, "connections_mobile"), "", nil, func() { devices(c, m, false) })
	addHost(c, m)
	shareCode(c, m)
	if d := pr.revoking; d != nil {
		open := true
		// Removing the pairing tells the device to forget this one.
		if ui.AlertDialog(c.Key("revoke"), &open, i18n.T(c, "live_revoke")+" "+deviceName(c, *d),
			i18n.T(c, "connections_revoke_confirm"), i18n.T(c, "settings_cancel"), i18n.T(c, "live_revoke")) == 1 {
			notice(c)
		}
		if !open {
			pr.revoking = nil
		}
	}
}

// devices lists the hosts (execution) or the mobile controllers: each a media
// row of its platform, name and short ID, then its latency, its status dot
// and label, and revoking its pairing. A host's context menu edits its
// network.
func devices(c *ui.Context, m *Model, execution bool) {
	p := theme.Of(c)
	var list []preview.Device
	for _, d := range m.Devices {
		if d.Execution == execution {
			list = append(list, d)
		}
	}
	if len(list) == 0 {
		kit.Result(c, kit.ResultProps{Icon: icons.Inbox, Title: i18n.T(c, "settings_empty"), Card: true})
		return
	}
	kit.Card(c, func() {
		for _, d := range list {
			row := ui.Row(c.Key(d.ID)).Padding(12, 0).Gap(12)
			if execution {
				row.ContextMenu(func(menu *ui.Menu) {
					// Editing a host's network asks the host for it.
					if menu.Item(i18n.T(c, "connection_action_edit")).Chosen() {
						notice(c)
					}
				})
			}
			row.Children(func() {
				kit.MediaRow(c, deviceName(c, d), d.ID, func() {
					ui.Icon(c, platformIcon(d)).FontSize(20).TextColor(p.Foreground)
				}, nil)
				ui.Row(c).Shrink(0).Gap(12).Children(func() {
					if d.Online && !d.Local && d.RTT > 0 {
						ui.Textf(c, "%d ms", d.RTT).FontSize(14).TextColor(p.MutedForeground)
					}
					status := deviceStatus(d)
					tone := kit.Neutral
					if d.Online {
						tone = kit.Positive
					}
					ui.Row(c).Gap(8).Children(func() {
						kit.StatusDot(c, tone, i18n.T(c, status)).Size(6, 6)
						ui.Text(c, i18n.T(c, status)).FontSize(14).TextColor(p.MutedForeground)
					})
					ui.Row(c).Width(28).Justify(ui.End).Children(func() {
						if d.Local {
							return
						}
						kit.MenuButton(c.Key("menu"), "device-menu-"+d.ID, func(b ui.Element) {
							kit.IconLook(c, b, icons.EllipsisVertical, i18n.T(c, "more"), "", 28, false)
						}, func(menu *kit.Menu) {
							// Editing a host's network asks the host for it.
							if execution && menu.Item(i18n.T(c, "connection_action_edit")).Chosen() {
								notice(c)
							}
							if menu.Item(i18n.T(c, "live_revoke")).Danger().Chosen() {
								m.pairing.revoking = &d
							}
						}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
					})
				})
			})
		}
	})
}

func deviceName(c *ui.Context, d preview.Device) string {
	if d.Name != "" {
		return d.Name
	}
	return i18n.T(c, "connections_device", "id", d.ID)
}

// deviceStatus is the status label's key: the route of an online remote
// device, else this computer, online or offline (label_key).
func deviceStatus(d preview.Device) string {
	switch {
	case d.Online && !d.Local && d.Route == "p2p":
		return "connection_route_p2p"
	case d.Online && !d.Local && d.Route == "relay":
		return "connection_route_relay"
	case d.Local:
		return "composer_host_local"
	case d.Online:
		return "connections_online"
	}
	return "connections_offline"
}

func platformIcon(d preview.Device) *ui.SVG {
	switch d.Platform {
	case "android":
		return icons.Android
	case "ios", "macos":
		return icons.Apple
	case "windows":
		return icons.Windows
	case "linux":
		return icons.Linux
	}
	if d.Execution {
		return icons.Device
	}
	return icons.Smartphone
}

// relayFields are relay::controls: the server, and its address when
// custom.
func relayFields(c *ui.Context, pr *pairing, relays []string) {
	options := relayOptions(c, relays)
	label := i18n.T(c, "pairing_relay_server")
	kit.Field(c, "relay", label, "", func() { pick(c, "relay", label, &pr.relay, options, false).Fill() })
	if pr.relay == len(options)-1 {
		title := i18n.T(c, "pairing_relay_address")
		kit.Field(c, "address", title, "", func() {
			ui.TextInput(c.Key("input"), &pr.address).Label(title).Placeholder(i18n.T(c, "pairing_relay_address_hint")).FillWidth()
		})
	}
}

// addHost is the network, the method, and the pairing code or the SSH login,
// with Connect or Install.
func addHost(c *ui.Context, m *Model) {
	pr := &m.pairing
	if !pr.adding {
		return
	}
	p := theme.Of(c)
	title := i18n.T(c, "host_add")
	// The title over its hint; the method as segments; the pairing code
	// large and centered, or the SSH login with host and port on one row;
	// the relay network last, as it rarely changes.
	kit.Dialog(c.Key("add-host"), &pr.adding, func() {
		ui.Column(c.Key("head")).Gap(4).Children(func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			ui.Text(c, i18n.T(c, "host_pairing_hint")).FontSize(14).TextColor(p.MutedForeground)
		})
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Padding(4, 0, 0, 0).Children(func() {
			kit.Segmented(c, "method", &pr.method, labels(c, methods), kit.SegmentedProps{Block: true}).Label(i18n.T(c, "connection"))
			if Method(pr.method) == ByPairing {
				ui.Row(c.Key("code")).FillWidth().Justify(ui.Center).Padding(16, 0).Children(func() {
					// Six digits (link rendezvous valid_code); a full code
					// would pair, which the preview does not.
					kit.InputOTP(c, "input", &pr.code, kit.InputOTPProps{Length: 6, Size: kit.SizeLarge}).Label(i18n.T(c, "host_pairing_mode"))
				})
			} else {
				sshFields(c, pr)
			}
			relayFields(c, pr, m.Relays)
		})
		action := i18n.T(c, "pairing_connect")
		if Method(pr.method) != ByPairing {
			action = i18n.T(c, "host_install")
		}
		footer(c, action, func() { pr.adding = false }, func() { notice(c) })
	}).Width(min(520, width(c)-48)).Label(title)
}

// sshFields are the name, the host and port on one row, the user, then the
// method's secret.
func sshFields(c *ui.Context, pr *pairing) {
	p := theme.Of(c)
	field := func(key string, value *string) {
		label := i18n.T(c, key)
		kit.Field(c, key, label, "", func() { ui.TextInput(c.Key("input"), value).Label(label).FillWidth() })
	}
	field("settings_name", &pr.name)
	ui.Row(c.Key("address")).Gap(12).Children(func() {
		ui.Column(c.Key("host")).Grow(1).MinWidth(0).Children(func() { field("ssh_host", &pr.host) })
		ui.Column(c.Key("port")).Width(96).Shrink(0).Children(func() { field("ssh_port", &pr.port) })
	})
	field("ssh_username", &pr.user)
	switch Method(pr.method) {
	case ByPassword:
		label := i18n.T(c, "ssh_password")
		kit.Field(c, "secret", label, "", func() { ui.TextInput(c.Key("input"), &pr.secret).Password().Label(label).FillWidth() })
	case ByPrivateKey:
		label := i18n.T(c, "ssh_private_key")
		kit.Field(c, "key", label, "", func() {
			ui.TextArea(c.Key("input"), &pr.key).Label(label).Placeholder(i18n.T(c, "form_private_key_hint")).FillWidth().Height(96).Font("monospace")
		})
	case ByKeyFile:
		label := i18n.T(c, "ssh_key_path")
		kit.Field(c, "path", label, "", func() {
			ui.Row(c).Gap(6).Children(func() {
				ui.TextInput(c.Key("input"), &pr.key).Label(label).Placeholder("~/.ssh/id_ed25519").Grow(1)
				// Choosing the file reads the disk.
				kit.IconButton(c.Key("browse"), icons.FolderOpen, i18n.T(c, "ssh_key_file"), 32, false).Border(1, p.Input).OnClick(func() { notice(c) })
			})
		})
	}
	if Method(pr.method) != ByPassword {
		label := i18n.T(c, "ssh_passphrase")
		kit.Field(c, "passphrase", label, "", func() {
			ui.TextInput(c.Key("input"), &pr.passphrase).Password().Label(label).Placeholder(i18n.T(c, "form_passphrase_hint")).FillWidth()
		})
	}
}

// shareCode is the share dialog: the network, this computer's pairing code in
// six cells, and how to use it. The preview has no pairing service, so it
// shows no code.
func shareCode(c *ui.Context, m *Model) {
	pr := &m.pairing
	if !pr.sharing {
		return
	}
	p := theme.Of(c)
	title := i18n.T(c, "pairing_local_code")
	kit.Dialog(c.Key("share"), &pr.sharing, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Column(c.Key("body")).Gap(16).Padding(0, 0, 8, 0).AlignItems(ui.Center).Children(func() {
			ui.Column(c).FillWidth().Gap(16).Children(func() { relayFields(c, pr, m.Relays) })
			ui.Row(c.Key("digits")).Gap(8).Children(func() {
				for i := range 6 {
					ui.Text(c.Key(i), "–").Size(48, 64).TextAlign(ui.Center).Center().FontSize(30).FontWeight(600).
						Radius(p.RadiusLg).Background(p.Secondary).TextColor(p.MutedForeground)
				}
			})
			ui.Text(c, i18n.T(c, "pairing_preview_disabled")).FontSize(14).TextColor(p.MutedForeground)
			ui.Text(c, i18n.T(c, "pairing_share_hint")).FontSize(14).TextAlign(ui.Center).TextColor(p.MutedForeground)
		})
	}).Width(min(400, width(c)-48)).Label(title)
}

// AddHost opens the Add host dialog (Shell::add_host), as the rail's Add
// host does over the Connections page.
func (s *State) AddHost() { s.pairing = pairing{adding: true, port: "22"} }
