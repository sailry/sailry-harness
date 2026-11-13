package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// experimental is the development and debugging settings: the sidebar's
// performance indicator, demo conversations and explicit acceptance runs.
// The shell swaps conversations as DemoMode changes; settings stay the window's own.
func experimental(c *ui.Context, m *Model) {
	kit.Group(c, "experimental", "", "", nil, func() {
		kit.Card(c, func() {
			kit.SwitchRow(c, "metrics", i18n.T(c, "settings_sidebar_metrics"), "", &m.SidebarMetrics)
			kit.SwitchRow(c, "demo", i18n.T(c, "settings_demo_mode"), i18n.T(c, "settings_demo_mode_description"), &m.DemoMode)
			settingSwitch(c, "test", i18n.T(c, "settings_test_mode"), i18n.T(c, "settings_test_mode_description"), &m.TestMode, m.Acceptance.Running)
		})
	})
	if m.TestMode {
		acceptance(c, m)
	}
}
