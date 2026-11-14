package settings

import (
	"runtime"
	"slices"
	"strings"

	"github.com/ELY-Open-Source/CrychicUI/feedback"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
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

// methodLabels are the methods' labels, by Method.
var methodLabels = []string{"host_pairing_mode", "ssh_password", "ssh_private_key", "ssh_key_file", "ssh_agent_short", "host_wsl"}

// hostOS is the system the methods are offered for.
var hostOS = runtime.GOOS

// hostMethods are the methods this computer can act on: WSL only on
// Windows, and the SSH agent only where the Node reaches it through a
// socket, which Windows' agent does not offer.
func hostMethods() []Method {
	offered := []Method{ByPairing, ByPassword, ByPrivateKey, ByKeyFile}
	if hostOS == "windows" {
		return append(offered, ByWSL)
	}
	return append(offered, ByAgent)
}

// SSH is whether the method logs in over SSH.
func (method Method) SSH() bool { return method != ByPairing && method != ByWSL }

// Sharing scopes of an SSH host's saved connection.
const (
	ScopePrivate = iota
	ScopeGlobal
	ScopeProjects
)

var scopes = []string{"connection_private", "connection_global", "connection_projects"}

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
	// Revoking are the devices whose pairing is being removed, by ID.
	Revoking map[string]bool
}

// SetConnectionStatus is the backend's report, each frame.
func (m *Model) SetConnectionStatus(status ConnectionStatus) { m.pairing.status = status }

// pairing is the Connections page's dialogs: adding a host, sharing this
// computer's code, and the pairing being revoked. Pairing, installing over
// SSH or into WSL and sharing reach the network; the preview stops at the
// notice.
type pairing struct {
	adding, sharing         bool
	relay                   int // of relayOptions
	address                 string
	method                  int // a Method
	code                    string
	name, host, port, user  string
	secret, key, passphrase string
	scope                   int              // a sharing scope
	shared                  map[string]*bool // the projects shared with, by ID
	distribution            int              // of status.Distributions
	revoking                *viewmodel.Device
	status                  ConnectionStatus
}

// sharedProjects are the offered projects the draft shares with, joined by
// commas in the offered order.
func (pr *pairing) sharedProjects() string {
	if pr.scope != ScopeProjects {
		return ""
	}
	var ids []string
	for _, project := range pr.status.Projects {
		if on := pr.shared[project.ID]; on != nil && *on {
			ids = append(ids, project.ID)
		}
	}
	return strings.Join(ids, ",")
}

// distributionName is the chosen WSL distribution, "" for none.
func (pr *pairing) distributionName() string {
	if pr.distribution < 0 || pr.distribution >= len(pr.status.Distributions) {
		return ""
	}
	return pr.status.Distributions[pr.distribution]
}

// installable is whether the draft names what its method needs before the
// backend can act on it.
func (pr *pairing) installable() bool {
	switch method := Method(pr.method); {
	case method == ByWSL:
		return pr.distributionName() != ""
	case method.SSH():
		return pr.scope != ScopeProjects || pr.sharedProjects() != ""
	}
	return true
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
			m.openAddHost()
		})
	}, func() { devices(c, m, true) })
	kit.Group(c, "mobile", i18n.T(c, "connections_mobile"), "", nil, func() { devices(c, m, false) })
	addHost(c, m)
	shareCode(c, m)
	connectionDialogs(c, m)
	if d := pr.revoking; d != nil {
		open := true
		// Removing the pairing tells the device to forget this one.
		if kit.AlertDialog(c.Key("revoke"), &open, i18n.T(c, "live_revoke")+" "+deviceName(c, *d),
			i18n.T(c, "connections_revoke_confirm"), i18n.T(c, "settings_cancel"), i18n.T(c, "live_revoke")) == 1 {
			if m.ConnectionActions != nil && m.ConnectionActions.Revoke != nil {
				m.ConnectionActions.Revoke(*d)
			} else {
				notice(c)
			}
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
	var list []viewmodel.Device
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
			row.ContextMenu(func(menu *ui.Menu) {
				// Editing a host's network asks the host for it.
				if execution && menu.Item(i18n.T(c, "connection_action_edit")).Chosen() {
					if m.ConnectionActions != nil && m.ConnectionActions.EditNetwork != nil {
						m.ConnectionActions.EditNetwork(d)
					} else {
						notice(c)
					}
				}
				if menu.Item(i18n.T(c, "connections_copy_id")).Chosen() {
					c.WriteClipboard(d.ID)
				}
			})
			row.Children(func() {
				kit.MediaRecord(c, deviceName(c, d), func() {
					ui.Icon(c, platformIcon(d)).FontSize(20).TextColor(p.Foreground)
				}, nil, func() {
					// One muted line: the system, the Sailry release and the
					// start of the Node ID, whose tooltip is the whole ID.
					ui.Row(c).MinWidth(0).Gap(4).Children(func() {
						if detail := deviceDetail(d); detail != "" {
							ui.Text(c, detail+" ·").FontSize(14).LineHeight(1.3).SingleLine().Shrink(1).TextColor(p.Description())
						}
						short := ui.Text(c, deviceShortID(d.ID)).FontSize(14).LineHeight(1.3).SingleLine().Shrink(0).TextColor(p.Description())
						if deviceShortID(d.ID) != d.ID {
							kit.Tooltip(c, short, d.ID)
						}
					})
				})
				ui.Row(c).Shrink(0).Gap(12).Children(func() {
					if d.Online && !d.Local && d.RTT > 0 {
						// The size of the status beside it, which is the library's.
						ui.Textf(c, "%d ms", d.RTT).FontSize(c.Theme().Rem(0.85)).TextColor(p.MutedForeground)
					}
					// CrychicUI's status, as the host page's header: the dot
					// in the success color for a device that is reachable
					// and muted for one that is not, beside the label.
					text, state := i18n.T(c, deviceStatus(d)), feedback.Offline
					if d.Online {
						state = feedback.Online
					}
					feedback.ConnectionStatus(c.Key("status"), state, feedback.ConnectionStatusOptions{Online: text, Reconnecting: text, Offline: text}).
						Shrink(0).AlignSelf(ui.Center).AlignItems(ui.Center).TextColor(p.MutedForeground)
					ui.Row(c).Width(28).Justify(ui.End).Children(func() {
						if d.Local {
							return
						}
						// A pairing being removed cannot be removed again.
						if m.pairing.status.Revoking[d.ID] {
							ui.Row(c.Key("revoking")).Size(28, 28).Center().Children(func() {
								kit.Busy(c).Label(i18n.T(c, "live_revoke"))
							})
							return
						}
						kit.MenuButton(c.Key("menu"), "device-menu-"+d.ID, func(b ui.Element) {
							kit.IconLook(c, b, icons.EllipsisVertical, i18n.T(c, "more"), "", 28, false)
						}, func(menu *kit.Menu) {
							// Editing a host's network asks the host for it.
							if execution && menu.Item(i18n.T(c, "connection_action_edit")).Chosen() {
								if m.ConnectionActions != nil && m.ConnectionActions.EditNetwork != nil {
									m.ConnectionActions.EditNetwork(d)
								} else {
									notice(c)
								}
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

func deviceName(c *ui.Context, d viewmodel.Device) string {
	if d.Name != "" {
		return d.Name
	}
	return i18n.T(c, "connections_device", "id", deviceShortID(d.ID))
}

// deviceShortID is the start of a Node ID, enough to tell devices apart.
func deviceShortID(id string) string { return id[:min(8, len(id))] }

// deviceDetail is what a device reported of itself: its system with the
// version when known, then its Sailry release.
func deviceDetail(d viewmodel.Device) string {
	system := map[string]string{"macos": "macOS", "windows": "Windows", "linux": "Linux", "ios": "iOS", "android": "Android"}[d.Platform]
	if system != "" && d.OS != "" {
		system += " " + d.OS
	}
	if d.Version == "" {
		return system
	}
	if system == "" {
		return "Sailry " + d.Version
	}
	return system + " · Sailry " + d.Version
}

// deviceStatus is the status label's key: the route of an online remote
// device, else this computer, online or offline (label_key).
func deviceStatus(d viewmodel.Device) string {
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

func platformIcon(d viewmodel.Device) *ui.SVG {
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

// addHost is the network, the method, and the pairing code, the SSH login or
// the WSL distribution, with Connect or Install. An installation whose
// outcome is uncertain locks the draft and offers Check status.
func addHost(c *ui.Context, m *Model) {
	pr := &m.pairing
	if !pr.adding {
		return
	}
	p := theme.Of(c)
	title := i18n.T(c, "host_add")
	locked := m.ConnectionPending || pr.status.Recovery
	// The title over its hint; the relay network; the method as segments;
	// the pairing code centered, or the SSH login with host and port on one
	// row.
	kit.Dialog(c.Key("add-host"), &pr.adding, func() {
		ui.Column(c.Key("head")).Gap(4).Children(func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			ui.Text(c, i18n.T(c, "host_pairing_hint")).FontSize(14).TextColor(p.MutedForeground)
		})
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Padding(4, 0, 0, 0).Children(func() {
			offered := hostMethods()
			names := make([]string, len(offered))
			for index, method := range offered {
				names[index] = i18n.T(c, methodLabels[method])
			}
			choice := max(slices.Index(offered, Method(pr.method)), 0)
			connectionRelayFields(c, pr, m.Relays, m.ConnectionRelays, locked)
			kit.Segmented(c, "method", &choice, names, kit.SegmentedProps{}).AlignSelf(ui.Start).Disabled(locked).Label(i18n.T(c, "connection"))
			pr.method = int(offered[choice])
			switch method := Method(pr.method); {
			case method == ByPairing:
				codePanel(c, func() {
					ui.Row(c.Key("cells")).FillWidth().Justify(ui.Center).Children(func() {
						kit.InputOTP(c, "input", &pr.code, kit.InputOTPProps{Length: 6, Size: kit.SizeLarge}).Disabled(locked).Label(i18n.T(c, "host_pairing_mode"))
					})
					ui.Text(c, i18n.T(c, "pairing_pin_hint")).FontSize(13).TextColor(p.MutedForeground)
				})
			case method == ByWSL:
				wslFields(c, pr, locked)
			default:
				sshFields(c, pr, m, locked)
				scopeFields(c, pr, locked)
			}
			if m.ConnectionPending {
				stage := m.ConnectionStage
				if stage == "" {
					stage = "connection_connecting"
				}
				kit.Shimmer(c.Key("progress"), i18n.T(c, stage), 14, p.MutedForeground)
			}
		})
		action := i18n.T(c, "pairing_connect")
		switch {
		case pr.status.Recovery:
			action = i18n.T(c, "host_check_status")
		case Method(pr.method) != ByPairing:
			action = i18n.T(c, "host_install")
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { pr.adding = false })
			// A step under way shows in its button, which cannot start another.
			kit.Primary(c.Key("ok"), action, false).Disabled(m.ConnectionPending || !pr.status.Recovery && !pr.installable()).Children(func() {
				if m.ConnectionPending {
					kit.Busy(c)
				}
				ui.Text(c, action).FontSize(14)
			}).OnClick(func() {
				if m.ConnectionActions == nil || m.ConnectionActions.Add == nil {
					notice(c)
					return
				}
				draft := m.ConnectionDraft()
				generation := m.connectionGeneration
				completed := func(ok bool) {
					current := m.ConnectionDraft()
					current.Relay = draft.Relay
					if ok && generation == m.connectionGeneration && draft == current {
						pr.adding = false
					}
				}
				// Checking asks the host what became of the installation
				// already sent; it never sends another.
				if pr.status.Recovery {
					if m.ConnectionActions.CheckStatus != nil {
						m.ConnectionActions.CheckStatus(completed)
					}
					return
				}
				m.ConnectionActions.Add(draft, completed)
			})
		})
	}).Width(min(620, width(c)-48)).Label(title)
}

// sshFields are the name, the host and port on one row, the user, then the
// method's secret; the agent has none. A secret left blank keeps the one the
// dialog already saved for the same method.
func sshFields(c *ui.Context, pr *pairing, m *Model, locked bool) {
	p := theme.Of(c)
	field := func(key string, value *string) {
		label := i18n.T(c, key)
		kit.Field(c, key, label, "", func() { ui.TextInput(c.Key("input"), value).Disabled(locked).Label(label).FillWidth() })
	}
	field("settings_name", &pr.name)
	ui.Row(c.Key("address")).Gap(12).Children(func() {
		ui.Column(c.Key("host")).Grow(1).MinWidth(0).Children(func() { field("ssh_host", &pr.host) })
		ui.Column(c.Key("port")).Width(96).Shrink(0).Children(func() { field("ssh_port", &pr.port) })
	})
	field("ssh_username", &pr.user)
	keep := ""
	if pr.status.Saved == Method(pr.method) {
		keep = i18n.T(c, "ssh_keep_credential")
	}
	switch Method(pr.method) {
	case ByPassword:
		label := i18n.T(c, "ssh_password")
		kit.Field(c, "secret", label, keep, func() {
			ui.TextInput(c.Key("input"), &pr.secret).Disabled(locked).Password().Label(label).FillWidth()
		})
	case ByPrivateKey:
		label := i18n.T(c, "ssh_private_key")
		kit.Field(c, "key", label, keep, func() {
			ui.TextArea(c.Key("input"), &pr.key).Disabled(locked).Label(label).Placeholder(i18n.T(c, "form_private_key_hint")).FillWidth().Height(96).Font("monospace")
		})
	case ByKeyFile:
		label := i18n.T(c, "ssh_key_path")
		kit.Field(c, "path", label, keep, func() {
			ui.Row(c).Gap(6).Children(func() {
				ui.TextInput(c.Key("input"), &pr.key).Disabled(locked).Label(label).Placeholder("~/.ssh/id_ed25519").Grow(1)
				// Choosing the file reads the disk.
				kit.IconButton(c.Key("browse"), icons.FolderOpen, i18n.T(c, "ssh_key_file"), 32, false).Disabled(locked).Border(1, p.Input).OnClick(func() {
					if m.ConnectionActions != nil && m.ConnectionActions.BrowseKey != nil {
						generation, key := m.connectionGeneration, pr.key
						m.ConnectionActions.BrowseKey(func(path string) {
							if m.Adding() && generation == m.connectionGeneration && Method(pr.method) == ByKeyFile && pr.key == key {
								pr.key = path
							}
						})
					} else {
						notice(c)
					}
				})
			})
		})
	}
	if method := Method(pr.method); method == ByPrivateKey || method == ByKeyFile {
		label := i18n.T(c, "ssh_passphrase")
		kit.Field(c, "passphrase", label, "", func() {
			ui.TextInput(c.Key("input"), &pr.passphrase).Disabled(locked).Password().Label(label).Placeholder(i18n.T(c, "form_passphrase_hint")).FillWidth()
		})
	}
}

// scopeFields are who else may use the saved SSH connection: nobody, every
// project, or the chosen ones.
func scopeFields(c *ui.Context, pr *pairing, locked bool) {
	p := theme.Of(c)
	label := i18n.T(c, "connection_scope")
	kit.Field(c, "scope", label, "", func() {
		ui.RadioGroup(c, func() {
			ui.Row(c.Key("choices")).Gap(16).Wrap().Children(func() {
				for index, key := range scopes {
					ui.Radio(c.Key(index), &pr.scope, index, i18n.T(c, key)).Disabled(locked)
				}
			})
		}).Label(label)
		if pr.scope != ScopeProjects {
			return
		}
		if len(pr.status.Projects) == 0 {
			ui.Text(c, i18n.T(c, "connection_no_projects")).FontSize(14).TextColor(p.MutedForeground)
			return
		}
		if pr.shared == nil {
			pr.shared = map[string]*bool{}
		}
		ui.Scroll(c.Key("projects")).MaxHeight(192).Gap(8).Children(func() {
			for _, project := range pr.status.Projects {
				if pr.shared[project.ID] == nil {
					pr.shared[project.ID] = new(bool)
				}
				ui.Checkbox(c.Key(project.ID), pr.shared[project.ID], project.Name).Disabled(locked)
			}
		})
	})
}

// wslFields are the distribution the Host is installed in, of those the
// Node found.
func wslFields(c *ui.Context, pr *pairing, locked bool) {
	p := theme.Of(c)
	label := i18n.T(c, "host_wsl_distribution")
	status := pr.status
	kit.Field(c, "distribution", label, i18n.T(c, "host_wsl_requirement"), func() {
		switch {
		case status.DistributionsLoading:
			kit.Shimmer(c, i18n.T(c, "host_runtime_loading"), 14, p.MutedForeground)
		case len(status.Distributions) > 0:
			pick(c, "distribution", label, &pr.distribution, status.Distributions, locked).Fill()
		case status.DistributionsLoaded:
			ui.Text(c, i18n.T(c, "host_wsl_empty")).FontSize(14).TextColor(p.MutedForeground)
		}
	})
}

// shareCode is the share dialog: the title over how to use the code, the
// network, and this computer's pairing code with its validity or progress,
// as in Add host. The preview has no pairing service, so it shows no code.
func shareCode(c *ui.Context, m *Model) {
	pr := &m.pairing
	if !pr.sharing {
		return
	}
	p, t := theme.Of(c), c.Theme()
	title := i18n.T(c, "pairing_local_code")
	kit.Dialog(c.Key("share"), &pr.sharing, func() {
		ui.Column(c.Key("head")).Gap(4).Children(func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			ui.Text(c, i18n.T(c, "pairing_share_hint")).FontSize(14).TextColor(p.MutedForeground)
		})
		ui.Column(c.Key("body")).Gap(16).Padding(4, 0, 0, 0).Children(func() {
			connectionRelayFields(c, pr, m.Relays, m.ConnectionRelays, false)
			codePanel(c, func() {
				ready := m.Live && len(m.Share.Code) == 6
				// The cells of a large InputOTP, so the code reads as the one
				// typed on the other device.
				ui.Row(c.Key("digits")).Gap(8).Children(func() {
					for i := range 6 {
						digit, color := "–", p.MutedForeground
						if ready {
							digit, color = m.Share.Code[i:i+1], p.Foreground
						}
						ui.Row(c.Key(i)).Size(48, 56).AlignItems(ui.Center).Justify(ui.Center).Radius(p.RadiusLg).
							Background(t.Surface).Border(1, p.Input).Children(func() {
							ui.Text(c, digit).FontSize(24).FontWeight(600).FontFeatures("tnum").TextColor(color)
						})
					}
				})
				status := func(key string) { ui.Text(c, i18n.T(c, key)).FontSize(13).TextColor(p.MutedForeground) }
				switch {
				case !m.Live:
					status("pairing_preview_disabled")
				case m.Share.Kind == "preparing" || m.Share.Kind == "retrying" || m.Share.Kind == "checking":
					key := "pairing_preparing"
					if m.Share.Kind == "checking" {
						key = "pairing_relay_checking"
					}
					kit.Shimmer(c, i18n.T(c, key), 13, p.MutedForeground)
				case m.Share.Kind == "failed":
					kit.Secondary(c.Key("retry"), i18n.T(c, "update_retry")).OnClick(func() {
						if m.ConnectionActions != nil && m.ConnectionActions.RetryShare != nil {
							m.ConnectionActions.RetryShare()
						}
					})
				case m.Share.Code != "":
					status("pairing_refresh_hint")
				}
			})
		})
	}).Width(min(460, width(c)-48)).Label(title)
}

// codePanel centers a pairing code on the dialog with its status line
// below.
func codePanel(c *ui.Context, children func()) {
	ui.Column(c.Key("code")).FillWidth().PaddingY(12).Gap(12).AlignItems(ui.Center).Children(children)
}

// AddHost opens the Add host dialog (Shell::add_host), as the rail's Add
// host does over the Connections page.
func (s *State) AddHost() { s.Model.openAddHost() }

func (m *Model) openAddHost() {
	m.connectionGeneration++
	m.pairing = pairing{adding: true, port: "22", status: m.pairing.status}
}

func connectionRelayFields(c *ui.Context, pr *pairing, relays []string, choices []RelayChoice, disabled bool) {
	if len(choices) == 0 {
		relayFields(c, pr, relays)
		return
	}
	options := make([]string, len(choices))
	for index, choice := range choices {
		label := choice.Label
		if choice.LabelKey != "" {
			label = i18n.T(c, choice.LabelKey)
		}
		timing := i18n.T(c, "composer_metric_unknown")
		if choice.Pending {
			timing = "…"
		} else if choice.Millis != nil {
			timing = i18n.T(c, "usage_duration_ms", "value", *choice.Millis)
		}
		options[index] = label + " · " + timing
	}
	label := i18n.T(c, "pairing_relay_server")
	kit.Field(c, "relay", label, "", func() { pick(c, "relay", label, &pr.relay, options, disabled).Fill() })
	if pr.relay >= 0 && pr.relay < len(choices) && choices[pr.relay].Custom {
		title := i18n.T(c, "pairing_relay_address")
		kit.Field(c, "address", title, "", func() {
			ui.TextInput(c.Key("input"), &pr.address).Disabled(disabled).Label(title).Placeholder(i18n.T(c, "pairing_relay_address_hint")).FillWidth()
		})
	}
}
