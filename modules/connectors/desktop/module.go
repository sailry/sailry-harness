package desktop

import (
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func Register(assembly *shell.Assembly) {
	page := Page()
	page.BindWindow = settings.BindWindow(connectorSettingsHost.BindConnectorSettings)
	assembly.SettingsPages = append(assembly.SettingsPages, page)
	assembly.Locales = append(assembly.Locales, Locales)
}

type connectorSettingsHost interface{ BindConnectorSettings() settings.WindowBinding }
