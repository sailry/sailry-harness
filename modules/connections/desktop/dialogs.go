package desktop

import (
	"runtime"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// methodLabels name SSH authentication methods within the installation flow.
var methodLabels = []string{"host_pairing_mode", "ssh_password", "ssh_private_key", "ssh_key_file", "ssh_agent_short", "host_wsl"}

// hostOS is the system the methods are offered for.
var hostOS = runtime.GOOS

// sshMethods are the authentication methods this Node can use to install a
// Host. Its agent socket is available on Unix systems.
func sshMethods() []settings.Method {
	offered := []settings.Method{settings.ByPassword, settings.ByPrivateKey, settings.ByKeyFile}
	if hostOS != "windows" {
		offered = append(offered, settings.ByAgent)
	}
	return offered
}

var scopes = []string{"connection_private", "connection_global", "connection_projects"}

// installable reports whether the form has the inputs required by its method.
func installable(draft settings.ConnectionDraft) bool {
	switch method := draft.Method; {
	case method == settings.ByWSL:
		return draft.Distribution != ""
	case method.SSH():
		return draft.Scope != settings.ScopeProjects || draft.Projects != ""
	}
	return true
}

// relayOptions are the connection servers: the public network, the named
// relays, and a custom address.
func relayOptions(c *ui.Context, relays []string) []string {
	return append(append([]string{i18n.T(c, "pairing_relay_direct")}, relays...), i18n.T(c, "pairing_relay_custom"))
}

// relayFields are relay::controls: the server, and its address when
// custom.
func relayFields(c *ui.Context, relay *int, address *string, relays []string, bare bool) {
	options := relayOptions(c, relays)
	label := i18n.T(c, "pairing_relay_server")
	relayField(c, "relay", label, bare, func() { settings.Pick(c, "relay", label, relay, options, false).Fill() })
	if *relay == len(options)-1 {
		title := i18n.T(c, "pairing_relay_address")
		relayField(c, "address", title, bare, func() {
			ui.TextInput(c.Key("input"), address).Label(title).Placeholder(i18n.T(c, "pairing_relay_address_hint")).FillWidth()
		})
	}
}

// addHost separates connecting an existing device from installing a Host. Only
// installation chooses the new Host's network and SSH authentication.
func addHost(c *ui.Context, m *settings.Model) {
	pr := m.ConnectionForm()
	if !pr.AddOpen {
		m.ConnectionFailure = nil
		return
	}
	p := theme.Of(c)
	width, _ := c.Size()
	title := i18n.T(c, "host_add")
	locked := m.ConnectionPending || pr.Status.Recovery
	var tabs float32
	kit.Dialog(c.Key("add-host"), &pr.AddOpen, func() {
		ui.Column(c.Key("head")).Gap(4).Children(func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			hint := "host_pairing_hint"
			if pr.Method != settings.ByPairing {
				hint = "host_install_hint"
			}
			ui.Text(c, i18n.T(c, hint)).FontSize(14).TextColor(p.MutedForeground)
		})
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Padding(4, 0, 0, 0).Children(func() {
			names := []string{i18n.T(c, "host_pairing_mode"), i18n.T(c, "host_ssh_mode")}
			if hostOS == "windows" {
				names = append(names, i18n.T(c, "host_wsl"))
			}
			choice := 0
			if pr.Method.SSH() {
				choice = 1
			} else if pr.Method == settings.ByWSL {
				choice = 2
			}
			props := kit.SegmentedProps{Block: true, Natural: true}
			tabs = kit.SegmentedWidth(c, names, props)
			kit.Segmented(c, "mode", &choice, names, props).Disabled(locked).Label(i18n.T(c, "connection"))
			switch choice {
			case 0:
				pr.Method = settings.ByPairing
			case 1:
				if !pr.Authentication.SSH() {
					pr.Authentication = settings.ByPassword
				}
				pr.Method = pr.Authentication
			case 2:
				pr.Method = settings.ByWSL
			}
			switch method := pr.Method; {
			case method == settings.ByPairing:
				codePanel(c, func() {
					kit.InputOTP(c, "input", &pr.Code, kit.InputOTPProps{Length: 6, Size: kit.SizeLarge, Fill: true}).Disabled(locked).Label(i18n.T(c, "host_pairing_mode"))
					ui.Text(c, i18n.T(c, "pairing_pin_hint")).FontSize(13).TextColor(p.MutedForeground)
				})
			case method == settings.ByWSL:
				connectionRelayFields(c, &pr.Relay, &pr.Address, m.Relays, m.ConnectionRelays, locked, false)
				wslFields(c, pr, locked)
			default:
				connectionRelayFields(c, &pr.Relay, &pr.Address, m.Relays, m.ConnectionRelays, locked, false)
				offered := sshMethods()
				names := make([]string, len(offered))
				for index, method := range offered {
					names[index] = i18n.T(c, methodLabels[method])
				}
				choice := max(slices.Index(offered, method), 0)
				label := i18n.T(c, "ssh_authentication")
				kit.Field(c, "authentication", label, "", func() {
					tabs = max(tabs, kit.SegmentedWidth(c, names, props))
					kit.Segmented(c, "method", &choice, names, props).Disabled(locked).Label(label)
				})
				pr.Authentication = offered[choice]
				pr.Method = pr.Authentication
				sshFields(c, pr, m, locked)
				scopeFields(c, pr, locked)
			}
			switch failure := m.ConnectionFailure; {
			case m.ConnectionPending:
				installProgress(c, m.ConnectionStage, pr.Method != settings.ByPairing && !pr.Status.Recovery)
			case failure != nil:
				text := i18n.T(c, failure.Title)
				if failure.Detail != "" {
					text += "\n" + failure.Detail
				}
				kit.Alert(c, "failure", failure.Tone, text, nil)
			}
		})
		action := i18n.T(c, "pairing_connect")
		switch {
		case pr.Status.Recovery:
			action = i18n.T(c, "host_check_status")
		case pr.Method != settings.ByPairing:
			action = i18n.T(c, "host_install")
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { pr.AddOpen = false })
			// A step under way shows in its button, which cannot start another.
			kit.PrimaryBusy(c.Key("ok"), action, !pr.Status.Recovery && !installable(m.ConnectionDraft()), m.ConnectionPending).OnClick(func() {
				m.ConnectionFailure = nil
				if m.ConnectionActions == nil || m.ConnectionActions.Add == nil {
					kit.Message(c, i18n.T(c, "preview_notice"))
					return
				}
				draft := m.ConnectionDraft()
				generation := m.ConnectionGeneration()
				completed := func(ok bool) {
					current := m.ConnectionDraft()
					current.Relay = draft.Relay
					if ok && generation == m.ConnectionGeneration() && draft == current {
						pr.AddOpen = false
					}
				}
				// Checking asks the host what became of the installation
				// already sent; it never sends another.
				if pr.Status.Recovery {
					if m.ConnectionActions.CheckStatus != nil {
						m.ConnectionActions.CheckStatus(completed)
					}
					return
				}
				m.ConnectionActions.Add(draft, completed)
			})
		})
	}).Width(min(max(pairingWidth, tabs+2*kit.DialogInset), width-48)).Label(title)
}

// installSteps are an installation's stages in order, by the keys its
// progress reports (live sshProgressKey) and the label each shows as a step.
var installSteps = [...][2]string{
	{"connection_connecting", "host_step_connect"},
	{"host_install_detecting", "host_install_detecting"},
	{"host_install_installing", "host_install_installing"},
	{"host_install_pairing", "host_step_pair"},
}

// installProgress is the attempt under way: an installation's steps, those
// done checked, the current one turning and those ahead faint, in a muted
// panel; pairing or checking an installation, its one stage turning.
func installProgress(c *ui.Context, stage string, installing bool) {
	p := theme.Of(c)
	if stage == "" {
		stage = "connection_connecting"
	}
	current := -1
	for index, step := range installSteps {
		if step[0] == stage {
			current = index
		}
	}
	ui.Column(c.Key("progress")).FillWidth().Gap(10).Padding(12).Radius(p.Radius).Background(p.Muted).Role(ui.RoleStatus).Children(func() {
		row := func(key any, label string, state int) {
			ui.Row(c.Key(key)).Gap(10).AlignItems(ui.Center).Children(func() {
				ui.Box(c.Key("mark")).Size(16, 16).Shrink(0).Center().Children(func() {
					switch {
					case state < 0:
						ui.Icon(c, icons.Check).FontSize(14).TextColor(p.Success)
					case state == 0:
						kit.BusySized(c, 14)
					default:
						ui.Box(c).Size(6, 6).Radius(3).Background(p.MutedForeground.Alpha(0.4))
					}
				})
				color := p.Foreground
				if state != 0 {
					color = p.MutedForeground
				}
				ui.Text(c, label).FontSize(14).SingleLine().TextColor(color)
			})
		}
		if !installing || current < 0 {
			row("stage", i18n.T(c, stage), 0)
			return
		}
		for index, step := range installSteps {
			row(step[0], i18n.T(c, step[1]), index-current)
		}
	})
}

// sshFields are the name, the host and port on one row, the user, then the
// method's secret; the agent has none. A secret left blank keeps the one the
// dialog already saved for the same method.
func sshFields(c *ui.Context, pr *settings.ConnectionForm, m *settings.Model, locked bool) {
	p := theme.Of(c)
	field := func(key string, value *string) {
		label := i18n.T(c, key)
		kit.Field(c, key, label, "", func() { ui.TextInput(c.Key("input"), value).Disabled(locked).Label(label).FillWidth() })
	}
	field("settings_name", &pr.Name)
	ui.Row(c.Key("address")).Gap(12).Children(func() {
		ui.Column(c.Key("host")).Grow(1).MinWidth(0).Children(func() { field("ssh_host", &pr.Host) })
		ui.Column(c.Key("port")).Width(96).Shrink(0).Children(func() { field("ssh_port", &pr.Port) })
	})
	field("ssh_username", &pr.Username)
	keep := ""
	if pr.Status.Saved == pr.Method {
		keep = i18n.T(c, "ssh_keep_credential")
	}
	switch pr.Method {
	case settings.ByPassword:
		label := i18n.T(c, "ssh_password")
		kit.Field(c, "secret", label, keep, func() {
			ui.TextInput(c.Key("input"), &pr.Password).Disabled(locked).Password().Label(label).FillWidth()
		})
	case settings.ByPrivateKey:
		label := i18n.T(c, "ssh_private_key")
		kit.Field(c, "key", label, keep, func() {
			ui.TextArea(c.Key("input"), &pr.Key).Disabled(locked).Label(label).Placeholder(i18n.T(c, "form_private_key_hint")).FillWidth().Height(96).Font("monospace")
		})
	case settings.ByKeyFile:
		label := i18n.T(c, "ssh_key_path")
		kit.Field(c, "path", label, keep, func() {
			ui.Row(c).Gap(6).Children(func() {
				ui.TextInput(c.Key("input"), &pr.Key).Disabled(locked).Label(label).Placeholder("~/.ssh/id_ed25519").Grow(1)
				// Choosing the file reads the disk.
				kit.IconButton(c.Key("browse"), icons.FolderOpen, i18n.T(c, "ssh_key_file"), 32, false).Disabled(locked).Border(1, p.Input).OnClick(func() {
					if m.ConnectionActions != nil && m.ConnectionActions.BrowseKey != nil {
						generation, key := m.ConnectionGeneration(), pr.Key
						m.ConnectionActions.BrowseKey(func(path string) {
							if m.Adding() && generation == m.ConnectionGeneration() && pr.Method == settings.ByKeyFile && pr.Key == key {
								pr.Key = path
							}
						})
					} else {
						kit.Message(c, i18n.T(c, "preview_notice"))
					}
				})
			})
		})
	case settings.ByAgent:
		ui.Text(c, i18n.T(c, "host_ssh_agent_hint")).FontSize(13).TextColor(p.MutedForeground)
	}
	if method := pr.Method; method == settings.ByPrivateKey || method == settings.ByKeyFile {
		label := i18n.T(c, "ssh_passphrase")
		kit.Field(c, "passphrase", label, "", func() {
			ui.TextInput(c.Key("input"), &pr.Passphrase).Disabled(locked).Password().Label(label).Placeholder(i18n.T(c, "form_passphrase_hint")).FillWidth()
		})
	}
}

// scopeFields are who else may use the saved SSH connection: nobody, every
// project, or the chosen ones.
func scopeFields(c *ui.Context, pr *settings.ConnectionForm, locked bool) {
	p := theme.Of(c)
	label := i18n.T(c, "connection_scope")
	kit.Field(c, "scope", label, "", func() {
		ui.RadioGroup(c, func() {
			ui.Row(c.Key("choices")).Gap(16).Wrap().Children(func() {
				for index, key := range scopes {
					ui.Radio(c.Key(index), &pr.Scope, index, i18n.T(c, key)).Disabled(locked)
				}
			})
		}).Label(label)
		if pr.Scope != settings.ScopeProjects {
			return
		}
		if len(pr.Status.Projects) == 0 {
			ui.Text(c, i18n.T(c, "connection_no_projects")).FontSize(14).TextColor(p.MutedForeground)
			return
		}
		if pr.SharedProjects == nil {
			pr.SharedProjects = map[string]*bool{}
		}
		ui.Scroll(c.Key("projects")).MaxHeight(192).Gap(8).Children(func() {
			for _, project := range pr.Status.Projects {
				if pr.SharedProjects[project.ID] == nil {
					pr.SharedProjects[project.ID] = new(bool)
				}
				ui.Checkbox(c.Key(project.ID), pr.SharedProjects[project.ID], project.Name).Disabled(locked)
			}
		})
	})
}

// wslFields are the distribution the Host is installed in, of those the
// Node found.
func wslFields(c *ui.Context, pr *settings.ConnectionForm, locked bool) {
	p := theme.Of(c)
	label := i18n.T(c, "host_wsl_distribution")
	status := pr.Status
	kit.Field(c, "distribution", label, i18n.T(c, "host_wsl_requirement"), func() {
		switch {
		case status.DistributionsLoading:
			kit.Shimmer(c, i18n.T(c, "host_runtime_loading"), 14, p.MutedForeground)
		case len(status.Distributions) > 0:
			settings.Pick(c, "distribution", label, &pr.DistributionChoice, status.Distributions, locked).Fill()
		case status.DistributionsLoaded:
			ui.Text(c, i18n.T(c, "host_wsl_empty")).FontSize(14).TextColor(p.MutedForeground)
		}
	})
}

// shareCode is the share dialog: the title over how to use the code, the
// network, and this computer's pairing code with its validity or progress,
// as in Add host. The preview has no pairing service, so it shows no code.
func shareCode(c *ui.Context, m *settings.Model) {
	pr := m.ConnectionForm()
	if !pr.ShareOpen {
		return
	}
	p, t := theme.Of(c), c.Theme()
	width, _ := c.Size()
	title := i18n.T(c, "pairing_local_code")
	kit.Dialog(c.Key("share"), &pr.ShareOpen, func() {
		ui.Column(c.Key("head")).Gap(4).Children(func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			ui.Text(c, i18n.T(c, "pairing_share_hint")).FontSize(14).TextColor(p.MutedForeground)
		})
		ui.Column(c.Key("body")).Gap(16).Padding(4, 0, 0, 0).Children(func() {
			connectionRelayFields(c, &pr.Relay, &pr.Address, m.Relays, m.ConnectionRelays, false, true)
			codePanel(c, func() {
				ready := m.Live && len(m.Share.Code) == 6
				// The cells of a large InputOTP, so the code reads as the one
				// typed on the other device.
				ui.Row(c.Key("digits")).AlignSelf(ui.Stretch).Gap(8).Children(func() {
					for i := range 6 {
						digit, color := "–", p.MutedForeground
						if ready {
							digit, color = m.Share.Code[i:i+1], p.Foreground
						}
						ui.Row(c.Key(i)).Grow(1).Basis(0).MinWidth(48).Height(56).AlignItems(ui.Center).Justify(ui.Center).Radius(p.RadiusLg).
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
	}).Width(min(pairingWidth, width-48)).Label(title)
}

// pairingWidth is the pairing dialogs' width: the code's cells span it
// as the fields do, at about the cells' height.
const pairingWidth = 440

// codePanel holds a pairing code as wide as the dialog's fields, with its
// status line centered below.
func codePanel(c *ui.Context, children func()) {
	ui.Column(c.Key("code")).FillWidth().PaddingY(12).Gap(12).AlignItems(ui.Center).Children(children)
}

// relayField is a relay control under its label, or alone where the
// dialog says what it is for (bare): the control still names itself.
func relayField(c *ui.Context, key, label string, bare bool, control func()) {
	if bare {
		ui.Column(c.Key(key)).Children(control)
		return
	}
	kit.Field(c, key, label, "", control)
}

func connectionRelayFields(c *ui.Context, relay *int, address *string, relays []string, choices []settings.RelayChoice, disabled, bare bool) {
	if len(choices) == 0 {
		relayFields(c, relay, address, relays, bare)
		return
	}
	options := make([]string, len(choices))
	for index, choice := range choices {
		label := choice.Label
		if choice.LabelKey != "" {
			label = i18n.T(c, choice.LabelKey)
		}
		if !choice.Pending && choice.Millis != nil {
			label += " · " + i18n.T(c, "usage_duration_ms", "value", *choice.Millis)
		}
		options[index] = label
	}
	label := i18n.T(c, "pairing_relay_server")
	relayField(c, "relay", label, bare, func() { settings.Pick(c, "relay", label, relay, options, disabled).Fill() })
	if *relay >= 0 && *relay < len(choices) && choices[*relay].Custom {
		title := i18n.T(c, "pairing_relay_address")
		relayField(c, "address", title, bare, func() {
			ui.TextInput(c.Key("input"), address).Disabled(disabled).Label(title).Placeholder(i18n.T(c, "pairing_relay_address_hint")).FillWidth()
		})
	}
}

func connectionDialogs(c *ui.Context, m *settings.Model) {
	if editor := m.NetworkEditor; editor != nil {
		open := true
		width, _ := c.Size()
		title := i18n.T(c, "host_network_edit")
		kit.Dialog(c.Key("network-editor"), &open, func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			ui.Text(c, deviceName(c, editor.Device)).FontSize(14).TextColor(theme.Of(c).MutedForeground)
			if editor.Loading {
				kit.Shimmer(c, i18n.T(c, "host_runtime_loading"), 14, theme.Of(c).MutedForeground)
			} else {
				connectionRelayFields(c, &editor.Relay, &editor.Address, m.Relays, editor.Relays, editor.Pending || editor.Locked || editor.Unavailable, false)
			}
			ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
				kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).Disabled(editor.Pending).OnClick(func() { m.NetworkEditor = nil })
				label := i18n.T(c, "settings_save")
				if editor.Retry {
					label = i18n.T(c, "settings_retry")
				}
				kit.PrimaryBusy(c.Key("save"), label, editor.Loading || editor.Unavailable, editor.Pending).OnClick(func() {
					if m.ConnectionActions != nil && m.ConnectionActions.SaveNetwork != nil {
						m.ConnectionActions.SaveNetwork(editor)
					}
				})
			})
		}).Width(min(420, width-48)).Gap(16).Label(title)
		// A save under way keeps its editor, as its Cancel says.
		if !open && !editor.Pending {
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
		// Trusting closes the prompt and the connection goes on, still
		// pending; any other way out of the prompt ends it.
		if kit.AlertDialog(c.Key("host-key"), &open, title, body, i18n.T(c, "settings_cancel"), i18n.T(c, "ssh_trust")) == 1 {
			m.HostTrust = nil
			trust.Confirm()
		} else if !open {
			m.HostTrust = nil
			m.ConnectionPending = false
		}
	}
}
