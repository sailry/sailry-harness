package desktop

import (
	"github.com/ELY-Open-Source/CrychicUI/feedback"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Connections,
		Group:   "settings_group_tools",
		Icon:    icons.Network,
		Order:   200,
		Render:  render,
	}
}

// render shows the hosts, with sharing and adding one, and the mobile
// controllers.
func render(c *ui.Context, s *settings.State) {
	m := s.Model
	pr := m.ConnectionForm()
	kit.Group(c, "hosts", i18n.T(c, "connections_hosts"), "", func() {
		kit.Secondary(c.Key("share"), i18n.T(c, "pairing_share")).OnClick(func() { pr.ShareOpen = true })
		label := i18n.T(c, "settings_add")
		kit.Primary(c.Key("add"), label, false).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(s.AddHost)
	}, func() { devices(c, m, true) })
	kit.Group(c, "mobile", i18n.T(c, "connections_mobile"), "", nil, func() { devices(c, m, false) })
	addHost(c, m)
	shareCode(c, m)
	connectionDialogs(c, m)
	if d := pr.Revoking; d != nil {
		open := true
		// Removing the pairing tells the device to forget this one.
		if kit.AlertDialog(c.Key("revoke"), &open, i18n.T(c, "live_revoke")+" "+deviceName(c, *d),
			i18n.T(c, "connections_revoke_confirm"), i18n.T(c, "settings_cancel"), i18n.T(c, "live_revoke")) == 1 {
			if m.ConnectionActions != nil && m.ConnectionActions.Revoke != nil {
				m.ConnectionActions.Revoke(*d)
			} else {
				kit.Message(c, i18n.T(c, "preview_notice"))
			}
		}
		if !open {
			pr.Revoking = nil
		}
	}
}

// devices lists the hosts (execution) or the mobile controllers: each a media
// row of its platform, name and short ID, then its latency, its status dot
// and label, and revoking its pairing. A host's context menu edits its
// network and opens its monitor.
func devices(c *ui.Context, m *settings.Model, execution bool) {
	form := m.ConnectionForm()
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
				if execution && m.ConnectionActions != nil && m.ConnectionActions.Inspect != nil && menu.Item(i18n.T(c, "workspace_host")).Chosen() {
					m.ConnectionActions.Inspect(d)
				}
				if !d.Local && d.Direction == "both" {
					for _, choice := range []struct{ direction, key string }{{"host", "connections_only_host"}, {"controller", "connections_only_controller"}} {
						if menu.Item(i18n.T(c, choice.key)).Disabled(form.Status.Narrowing[d.ID]).Chosen() && m.ConnectionActions != nil && m.ConnectionActions.Narrow != nil {
							m.ConnectionActions.Narrow(d, choice.direction)
						}
					}
				}
				// Editing a host's network asks the host for it.
				if execution && menu.Item(i18n.T(c, "connection_action_edit")).Chosen() {
					if m.ConnectionActions != nil && m.ConnectionActions.EditNetwork != nil {
						m.ConnectionActions.EditNetwork(d)
					} else {
						kit.Message(c, i18n.T(c, "preview_notice"))
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
						if d.Direction == "both" {
							key := "connections_bidirectional"
							if d.Legacy {
								key = "connections_legacy_bidirectional"
							}
							ui.Text(c, i18n.T(c, key)+" ·").FontSize(14).LineHeight(1.3).SingleLine().Shrink(1).TextColor(p.Description())
						}
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
						if d.Local && (!execution || m.ConnectionActions == nil || m.ConnectionActions.Inspect == nil) {
							return
						}
						// A pairing being removed cannot be removed again.
						if form.Status.Revoking[d.ID] {
							ui.Row(c.Key("revoking")).Size(28, 28).Center().Children(func() {
								kit.Busy(c).Label(i18n.T(c, "live_revoke"))
							})
							return
						}
						kit.MenuButton(c.Key("menu"), "device-menu-"+d.ID, func(b ui.Element) {
							kit.IconLook(c, b, icons.EllipsisVertical, i18n.T(c, "more"), "", 28, false)
						}, func(menu *kit.Menu) {
							if execution && m.ConnectionActions != nil && m.ConnectionActions.Inspect != nil && menu.Item(i18n.T(c, "workspace_host")).Chosen() {
								m.ConnectionActions.Inspect(d)
							}
							if d.Local {
								return
							}
							// Editing a host's network asks the host for it.
							if execution && menu.Item(i18n.T(c, "connection_action_edit")).Chosen() {
								if m.ConnectionActions != nil && m.ConnectionActions.EditNetwork != nil {
									m.ConnectionActions.EditNetwork(d)
								} else {
									kit.Message(c, i18n.T(c, "preview_notice"))
								}
							}
							if menu.Item(i18n.T(c, "live_revoke")).Danger().Chosen() {
								form.Revoking = &d
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
	icon := icons.Platform(d.Platform)
	if icon == icons.Device && !d.Execution {
		return icons.Smartphone
	}
	return icon
}
