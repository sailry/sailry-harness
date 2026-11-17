package host

import (
	"slices"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// portForm is the new or edited forward dialog: the forward edited (nil
// for a new one), the ports as typed and whether the forward is being made.
type portForm struct {
	open          bool
	editing       *Port
	remote, local string
	pending       bool
}

// portsHeader is the host header's forwards: the first three, each opening
// its address, then Port forwarding, the forwards' menu.
func portsHeader(c *ui.Context, s *State, h *Host, actions Actions) {
	for i, x := range h.Ports {
		if i == 3 {
			break
		}
		b := small(c, "port-"+strconv.Itoa(i), icons.Globe, strconv.Itoa(x.Remote), !x.Listening, false)
		kit.Tooltip(c, b, x.Route())
		b.OnClick(func() { openPort(c, x, actions) })
	}
	label := i18n.T(c, "port_title")
	kit.MenuButton(c, "host-ports-"+h.ID, func(b ui.Element) {
		p := theme.Of(c)
		kit.GhostLook(c, b.Label(label), false).Height(28).PaddingX(8).Gap(6).Children(func() {
			ui.Icon(c, icons.Network).FontSize(14)
			ui.Text(c, label).FontSize(14).SingleLine()
			ui.Icon(c, icons.ChevronDown).FontSize(12).TextColor(p.MutedForeground)
		})
	}, func(m *kit.Menu) { s.PortMenu(c, m, h, actions) }, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
}

// PortMenu lists the forwards, each a submenu that edits or deletes it,
// then New forward. The header's button and the page's More menu both show
// it.
func (s *State) PortMenu(c *ui.Context, m *kit.Menu, h *Host, actions Actions) {
	for _, x := range h.Ports {
		item := m.Submenu(x.Route(), func(sub *kit.Menu) {
			if sub.Item(i18n.T(c, "settings_edit")).Icon(icons.Pencil).Chosen() {
				edited := x
				s.ports = portForm{open: true, editing: &edited, remote: strconv.Itoa(x.Remote), local: strconv.Itoa(x.Local)}
			}
			if sub.Item(i18n.T(c, "settings_delete")).Icon(icons.Trash).Danger().Chosen() {
				deletePort(h, x, actions)
			}
		}).Icon(icons.Network)
		if !x.Listening {
			item.Trailing(i18n.T(c, "port_closed"))
		}
	}
	if len(h.Ports) > 0 {
		m.Separator()
	}
	if m.Item(i18n.T(c, "port_add")).Icon(icons.Plus).Chosen() {
		s.ports = portForm{open: true}
	}
}

func openPort(c *ui.Context, port Port, actions Actions) {
	if actions.OpenPort != nil {
		actions.OpenPort(port)
		return
	}
	kit.PreviewNotice(c)
}

// deletePort ends a forward: the host's, or the preview's in memory.
func deletePort(h *Host, x Port, actions Actions) {
	if actions.ClosePort != nil {
		actions.ClosePort(x)
		return
	}
	h.Ports = slices.DeleteFunc(h.Ports, func(p Port) bool { return p.Remote == x.Remote })
}

// portDialogs is the new or edited forward dialog. An edit checks the ports
// against the other forwards. The forwarding owner replaces the route only
// after preparation succeeds; a failed save keeps the old mapping and draft.
func portDialogs(c *ui.Context, s *State, h *Host, actions Actions) {
	pk := &s.ports
	p := theme.Of(c)
	w, _ := c.Size()
	if !pk.open {
		return
	}
	title, action := i18n.T(c, "port_add"), i18n.T(c, "port_create")
	edited := pk.editing
	others := h.Ports
	if edited != nil {
		title, action = i18n.T(c, "port_edit"), i18n.T(c, "settings_save")
		others = slices.DeleteFunc(slices.Clone(h.Ports), func(p Port) bool { return p.Remote == edited.Remote })
	}
	kit.Dialog(c.Key("port-add"), &pk.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).PaddingEnd(32).TextColor(p.PopoverForeground)
		for _, f := range []struct {
			key, label string
			value      *string
		}{{"remote", "port_remote", &pk.remote}, {"local", "port_local", &pk.local}} {
			label := i18n.T(c, f.label)
			kit.Field(c, f.key, label, "", func() {
				ui.TextInput(c.Key("input"), f.value).Label(label).FillWidth()
			})
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { pk.open = false })
			kit.PrimaryBusy(c.Key("create"), action, false, pk.pending).OnClick(func() {
				x, problem := AddPort(others, pk.remote, pk.local)
				if problem != "" {
					kit.Toast(c, kit.Failed, i18n.T(c, problem))
					return
				}
				if edited != nil && x.Remote == edited.Remote && x.Local == edited.Local {
					pk.open = false
					return
				}
				if edited != nil && actions.ReplacePort != nil {
					pk.pending = true
					actions.ReplacePort(*edited, pk.remote, pk.local, func(success bool) {
						pk.pending = false
						if success {
							pk.open = false
						}
					})
					return
				}
				if edited != nil && (actions.CreatePort != nil || actions.ClosePort != nil) {
					kit.Toast(c, kit.Failed, i18n.T(c, "port_failed"))
					return
				}
				if actions.CreatePort != nil {
					pk.pending = true
					actions.CreatePort(pk.remote, pk.local, func(success bool) {
						pk.pending = false
						if success {
							pk.open = false
						}
					})
					return
				}
				if edited != nil {
					deletePort(h, *edited, actions)
				}
				h.Ports = append(h.Ports, x)
				slices.SortFunc(h.Ports, func(a, b Port) int { return a.Remote - b.Remote })
				pk.open = false
				kit.PreviewNotice(c)
			})
		})
	}).Width(min(420, w-48)).Gap(16).Label(title)
}
