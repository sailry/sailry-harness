package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// experimental holds the sidebar's performance indicator. Developer demo
// and acceptance runs are started from the command line.
func experimental(c *ui.Context, m *Model) {
	kit.Group(c, "experimental", "", "", nil, func() {
		kit.Card(c, func() {
			kit.SwitchRow(c, "metrics", i18n.T(c, "settings_sidebar_metrics"), "", &m.SidebarMetrics)
		})
	})
}
