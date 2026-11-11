package host

import (
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// portPicker is the port picker and the new forward dialog: the search, the
// forward chosen (its local port, 0 for the list), and the new forward's
// ports as typed.
type portPicker struct {
	open, adding  bool
	query         string
	selected      int
	remote, local string
	pending       bool
}

// portsHeader is the host header's forwards: the first three, each opening
// its address, then Port forwarding, which opens the picker.
func portsHeader(c *ui.Context, s *State, h *Host, actions Actions) {
	for i, x := range h.Ports {
		if i == 3 {
			break
		}
		b := small(c, "port-"+strconv.Itoa(i), icons.Globe, strconv.Itoa(x.Remote), !x.Listening, false)
		kit.Tooltip(c, b, x.Route())
		b.OnClick(func() { openPort(c, x, actions) })
	}
	small(c, "ports", icons.Network, i18n.T(c, "port_title"), false, false).OnClick(func() {
		s.ports = portPicker{open: true}
	})
}

func openPort(c *ui.Context, port Port, actions Actions) {
	if actions.OpenPort != nil {
		actions.OpenPort(port)
		return
	}
	kit.Message(c, i18n.T(c, "preview_notice"))
}

// pick is a row of the picker.
type pick struct {
	key, label string
	icon       *ui.SVG
	disabled   bool
	run        func()
}

// portDialogs are the picker (a searchable list 480 wide as the command
// pickers: the forwards, then New forward; a forward chosen lists Open
// address, Copy address, Close forwarding and Back) and the new forward
// dialog.
func portDialogs(c *ui.Context, s *State, h *Host, actions Actions) {
	pk := &s.ports
	p := theme.Of(c)
	w, _ := c.Size()
	if pk.open {
		var rows []pick
		if i := slices.IndexFunc(h.Ports, func(x Port) bool { return x.Local == pk.selected }); pk.selected != 0 && i >= 0 {
			x := h.Ports[i]
			rows = []pick{
				{"open", i18n.T(c, "port_address"), icons.Globe, !x.Listening, func() { openPort(c, x, actions); pk.open = false }},
				{"copy", i18n.T(c, "port_copy"), icons.Copy, false, func() { c.WriteClipboard(x.Address()); pk.open = false }},
				{"close", i18n.T(c, "port_close"), icons.Close, false, func() {
					if actions.ClosePort != nil {
						actions.ClosePort(x)
					} else {
						h.Ports = slices.Delete(h.Ports, i, i+1)
					}
					pk.selected = 0
				}},
				{"back", i18n.T(c, "reference_back"), icons.ChevronLeft, false, func() { pk.selected = 0 }},
			}
		} else {
			pk.selected = 0
			for i, x := range h.Ports {
				label := x.Route()
				if !x.Listening {
					label += " · " + i18n.T(c, "port_closed")
				}
				rows = append(rows, pick{"row-" + strconv.Itoa(i), label, icons.Network, false, func() { pk.selected = x.Local }})
			}
			rows = append(rows, pick{"add", i18n.T(c, "port_add"), icons.Plus, false, func() {
				*pk = portPicker{adding: true}
			}})
		}
		q := strings.ToLower(strings.TrimSpace(pk.query))
		rows = slices.DeleteFunc(rows, func(r pick) bool { return q != "" && !strings.Contains(strings.ToLower(r.label), q) })
		label := i18n.T(c, "port_search")
		kit.Dialog(c.Key("port-picker"), &pk.open, func() {
			ui.SearchField(c.Key("search"), &pk.query).Placeholder(label).Label(label).FillWidth().AutoFocus()
			ui.Column(c.Key("rows")).Gap(2).Children(func() {
				if len(rows) == 0 {
					ui.Text(c, i18n.T(c, "connection_search_empty")).FontSize(14).Padding(12).TextColor(p.MutedForeground)
				}
				for _, r := range rows {
					b := ui.ButtonBase(c.Key(r.key)).Label(r.label).Height(36).PaddingX(8).Gap(8).Radius(p.Radius).Justify(ui.Start).
						Disabled(r.disabled).Cursor(ui.CursorPointer).TextColor(p.PopoverForeground)
					switch {
					case r.disabled:
						b.Opacity(0.5)
					case b.Hovered() || b.FocusVisible():
						b.Background(p.Hover)
					}
					b.Children(func() {
						ui.Icon(c, r.icon).FontSize(16)
						ui.Text(c, r.label).FontSize(14).SingleLine().Shrink(1)
					}).OnClick(r.run)
				}
			})
		}).Width(min(480, w-48)).Gap(8).Label(label)
	}
	if !pk.adding {
		return
	}
	title := i18n.T(c, "port_add")
	kit.Dialog(c.Key("port-add"), &pk.adding, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
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
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { pk.adding = false })
			create := i18n.T(c, "port_create")
			kit.PrimaryBusy(c.Key("create"), create, false, pk.pending).OnClick(func() {
				x, problem := AddPort(h.Ports, pk.remote, pk.local)
				if problem != "" {
					kit.Toast(c, kit.Failed, i18n.T(c, problem))
					return
				}
				if actions.CreatePort != nil {
					pk.pending = true
					actions.CreatePort(pk.remote, pk.local, func(success bool) {
						pk.pending = false
						if success {
							pk.adding = false
						}
					})
					return
				}
				h.Ports = append(h.Ports, x)
				pk.adding = false
				kit.Message(c, i18n.T(c, "preview_notice"))
			})
		})
	}).Width(min(420, w-48)).Label(title)
}
