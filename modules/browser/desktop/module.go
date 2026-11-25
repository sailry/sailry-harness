package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func Register(assembly *shell.Assembly) {
	assembly.RegisterPanelResource(panel.Resource{
		Kind: panel.Browser, Label: "resource_browser", Icon: icons.Web, Order: 10,
		Mods: ui.Cmd, Key: ui.KeyT, Render: panel.RenderBrowserResource,
	}, func(host shell.PanelResourceHost) {
		if host.BindBrowser != nil {
			host.BindBrowser()
		}
	})
	page := Page()
	page.BindWindow = settings.BindWindow(browserSettingsHost.BindBrowserSettings)
	assembly.SettingsPages = append(assembly.SettingsPages, page)
	assembly.Locales = append(assembly.Locales, Locales)
}

type browserSettingsHost interface{ BindBrowserSettings() settings.WindowBinding }
