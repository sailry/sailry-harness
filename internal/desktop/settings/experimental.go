package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// experimental holds the sidebar's performance indicator and this
// Desktop's memory map. Developer demo and acceptance runs are started from
// the command line.
func experimental(c *ui.Context, m *Model) {
	kit.Group(c, "experimental", "", "", nil, func() {
		kit.Card(c, func() {
			kit.SwitchRow(c, "metrics", i18n.T(c, "settings_sidebar_metrics"), "", &m.SidebarMetrics)
		})
	})
	if len(m.Memory) == 0 {
		return
	}
	kit.Group(c, "memory", i18n.T(c, "settings_ram"), "", nil, func() {
		kit.Treemap(c, "memory-map", m.Memory, kit.TreemapOptions{})
	})
	if len(m.PluginMemoryRows) == 0 {
		return
	}
	kit.Group(c, "plugin-memory", i18n.T(c, "ram_plugins"), "", nil, func() {
		ui.Column(c).FillWidth().Gap(12).Children(func() {
			hasValues := false
			for _, group := range m.PluginMemory {
				hasValues = hasValues || len(group.Items) > 0
			}
			if hasValues {
				kit.Treemap(c, "plugin-memory-map", m.PluginMemory, kit.TreemapOptions{})
			}
			for _, row := range m.PluginMemoryRows {
				kit.Row(c, row.ID, row.Name, "", func() {
					ui.Text(c, row.Total).FontSize(kit.RowFont)
				}).Tooltip(i18n.T(c, "ram_plugins_estimate") + " · " + row.Detail)
			}
		})
	})
}
