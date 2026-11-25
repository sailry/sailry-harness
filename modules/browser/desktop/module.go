package desktop

import (
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func Register(assembly *shell.Assembly) {
	assembly.RegisterPanelResource(panel.Resource{
		Kind: panel.Browser, Label: "resource_browser", Icon: icons.Web, Order: 10,
		Keystroke: "secondary-t", Numbered: true, New: (*panel.State).OpenBrowserTab,
		Render: renderTab, Title: panel.BrowserTitle, Busy: panel.BrowserBusy, Release: panel.ReleaseBrowserTab,
	}, func(host shell.PanelResourceHost) {
		if host.BindBrowser != nil {
			host.BindBrowser()
			registerLinks(host)
		}
	})
	page := Page()
	page.BindWindow = settings.BindWindow(browserSettingsHost.BindBrowserSettings)
	assembly.SettingsPages = append(assembly.SettingsPages, page)
	assembly.Locales = append(assembly.Locales, Locales)
}

type browserSettingsHost interface{ BindBrowserSettings() settings.WindowBinding }
