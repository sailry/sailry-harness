package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// ManagesNode reports whether the page exposes settings for the selected Node.
func (section Section) ManagesNode() bool {
	switch section {
	case General, Conversation, Plugins, Skills, Knowledge, Terminal, Browser, Hooks, Runtime, OCR, Decision, Permissions, Backup:
		return true
	}
	return false
}

func (m *Model) managedHosts() []ProviderHost {
	if m.ManagedHosts != nil {
		return m.ManagedHosts()
	}
	if m.ProviderActions.Hosts != nil {
		return m.ProviderActions.Hosts()
	}
	return nil
}

// managedNodeID is the chosen host's Node ID, the local Node's for the
// initial empty choice. An unavailable choice stays selected.
func (m *Model) managedNodeID() string {
	if !m.Live {
		return ""
	}
	if m.ManagedNode != "" {
		return m.ManagedNode
	}
	hosts := m.managedHosts()
	if len(hosts) == 0 {
		return ""
	}
	return hosts[0].Node
}

func settingsTargetStatus(c *ui.Context, key string) {
	kit.Result(c, kit.ResultProps{Title: i18n.T(c, key), Card: true})
}

// managedNodeSelect is the settings pages' host filter: a borderless dropdown
// naming the chosen Node, with this computer shown as the local device. It
// reports whether the choice changed in this frame.
func managedNodeSelect(c *ui.Context, m *Model, target *string) bool {
	if !m.Live {
		return false
	}
	hosts := m.managedHosts()
	if len(hosts) == 0 || len(hosts) == 1 && *target == "" {
		return false
	}
	// The first host is the local Node. The empty value is also the initial
	// choice when no host has been selected.
	value := func(i int) string {
		if i == 0 {
			return ""
		}
		return hosts[i].Node
	}
	current := -1
	for i, host := range hosts {
		if value(i) == *target || host.Node == *target {
			current = i
			break
		}
	}
	shown, icon, connected := i18n.T(c, "unavailable"), icons.Device, false
	if current >= 0 {
		host := hosts[current]
		shown, icon, connected = ProviderHostName(c, host), icons.Platform(host.Platform), host.Connected
	}
	title := i18n.T(c, "project_host")
	changed := false
	button := kit.MenuButton(c, "managed-node", func(b ui.Element) {
		label := shown
		if current >= 0 && !connected {
			label += " · " + i18n.T(c, "connections_offline")
		}
		kit.GhostLook(c, b.Label(title), false).Row().Height(28).Gap(6).PaddingX(8).MaxWidth(256).Shrink(1).MinWidth(0).Children(func() {
			ui.Icon(c, icon).FontSize(14).Shrink(0).TextColor(theme.Of(c).MutedForeground)
			ui.Text(c, label).FontSize(13).SingleLine().Shrink(1).MinWidth(0)
			ui.Icon(c, icons.ChevronDown).FontSize(12).Shrink(0).TextColor(theme.Of(c).MutedForeground)
		})
	}, func(menu *kit.Menu) {
		for i, host := range hosts {
			item := menu.Item(ProviderHostName(c, host)).Icon(icons.Platform(host.Platform)).Checked(i == current).Disabled(!host.Connected)
			if !host.Connected {
				item.Trailing(i18n.T(c, "connections_offline"))
			}
			if item.Chosen() && value(i) != *target {
				*target, changed = value(i), true
			}
		}
	}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd, CheckRight: true, MaxHeight: 320})
	hint := i18n.T(c, "provider_on_host", "host", shown)
	if !connected {
		hint += " · " + i18n.T(c, "connections_offline")
	}
	kit.Tooltip(c, button, hint)
	return changed
}

// ManagedNodeID resolves the selected settings Node, including the default local Node.
func (m *Model) ManagedNodeID() string { return m.managedNodeID() }

// ProviderHostName is the host label shown in the settings target selector.
func ProviderHostName(c *ui.Context, host ProviderHost) string {
	if host.Local {
		return i18n.T(c, "composer_host_local")
	}
	if host.Name != "" {
		return i18n.T(c, host.Name)
	}
	return host.Node
}

// ProviderHostLabel is label with its host's name when several hosts are
// connected.
func ProviderHostLabel(c *ui.Context, m *Model, node, label string) string {
	if node == "" {
		return label
	}
	hosts := m.managedHosts()
	if len(hosts) < 2 {
		return label
	}
	for _, host := range hosts {
		if host.Node == node {
			return label + " (" + ProviderHostName(c, host) + ")"
		}
	}
	return label + " (" + node + ")"
}
