package host

import (
	"slices"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// tunnelEdit is the policy editor's draft (tunnels/editor.rs): the ports
// as typed and the policy it opened on.
type tunnelEdit struct {
	open     bool
	value    string
	original []int
}

// tunnels is the host's tunnels (workspace/host/tunnels.rs): the count,
// the allowed ports with Edit on this host's own policy (read-only on
// another's), then each tunnel with Open and Close. A policy that failed
// to load says so in a toast with Retry.
func tunnels(c *ui.Context, s *State, h *Host) {
	t := h.Tunnels
	if t == nil {
		return
	}
	p := theme.Of(c)
	count := i18n.T(c, "host_unknown")
	if t.Loaded {
		count = strconv.Itoa(len(t.Entries))
	}
	ready := t.Loaded && t.Error == ""
	section(c, "tunnels", i18n.T(c, "tunnels_title"), count, func() {
		muted(c, i18n.T(c, "tunnels_preview"))
		ui.Row(c.Key("policy")).Gap(12).Children(func() {
			text := i18n.T(c, "tunnel_ports_empty")
			if len(t.Ports) > 0 {
				text = i18n.T(c, "tunnel_ports_value", "ports", PortsLabel(t.Ports))
			}
			ui.Text(c, text).FontSize(14).Grow(1).Shrink(1)
			if t.Editable {
				small(c, "edit", icons.Settings2, i18n.T(c, "tunnel_edit"), !ready, false).OnClick(func() {
					s.tunnel = tunnelEdit{open: true, value: PortsLabel(t.Ports), original: slices.Clone(t.Ports)}
				})
			}
		})
		if !t.Editable {
			ui.Text(c, i18n.T(c, "tunnel_readonly")).FontSize(12).TextColor(p.MutedForeground)
		}
		if !t.Loaded && t.Error == "" {
			ui.Spinner(c.Key("loading")).Size(16, 16)
		}
		if t.Loaded && len(t.Entries) == 0 {
			kit.EmptyGroup(c, icons.Network, i18n.T(c, "tunnels_empty"))
		}
		for _, x := range t.Entries {
			ui.Row(c.Key(x.ID)).Gap(12).Padding(4, 0).Children(func() {
				ui.Icon(c, icons.Network).FontSize(16).Shrink(0)
				ui.Text(c, i18n.T(c, "tunnel_route", "local", x.Local, "remote", x.Remote)).FontSize(14).SingleLine().Grow(1).Shrink(1)
				kit.Action(c.Key("open"), icons.Globe, i18n.T(c, "tunnel_open"), !ready).OnClick(func() {
					// The preview does not open the tunnel's page: it
					// says where it would be (tunnel_preview).
					kit.Notify(c, i18n.T(c, "tunnel_open"), i18n.T(c, "tunnel_open_hint")+"\nhttp://localhost:"+strconv.Itoa(x.Local))
				})
				kit.Action(c.Key("close"), icons.Close, i18n.T(c, "tunnel_close"), !ready).OnClick(func() { t.Close(x) })
			})
		}
	})
}

// tunnelDialog is the policy editor (tunnels/editor.rs): 480 wide, the
// hint and the ports, Cancel and Save. A problem toasts and keeps the
// draft.
func tunnelDialog(c *ui.Context, s *State, h *Host) {
	e := &s.tunnel
	if !e.open || h.Tunnels == nil {
		return
	}
	p := theme.Of(c)
	w, _ := c.Size()
	title := i18n.T(c, "tunnel_edit")
	kit.Dialog(c.Key("tunnel-editor"), &e.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
		ui.Text(c, i18n.T(c, "tunnel_policy_hint")).FontSize(14).TextColor(p.MutedForeground)
		label := i18n.T(c, "tunnel_ports_label")
		kit.Field(c, "ports", label, "", func() {
			ui.TextInput(c.Key("input"), &e.value).Label(label).Placeholder(i18n.T(c, "tunnel_ports_placeholder")).FillWidth()
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { e.open = false })
			save := i18n.T(c, "settings_save_preview")
			kit.Primary(c.Key("save"), save, false).Children(func() { ui.Text(c, save).FontSize(14) }).OnClick(func() {
				if problem := h.Tunnels.SavePorts(e.original, e.value); problem != "" {
					kit.Toast(c, kit.Failed, i18n.T(c, problem))
					return
				}
				e.open = false
			})
		})
	}).Width(min(480, w-48)).Label(title)
}
