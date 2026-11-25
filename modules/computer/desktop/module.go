package desktop

import (
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func Register(assembly *shell.Assembly) {
	page := Page()
	page.BindWindow = settings.BindWindow(computerSettingsHost.BindComputerSettings)
	assembly.SettingsPages = append(assembly.SettingsPages, page)
	assembly.Permissions = append(assembly.Permissions, permissionContribution())
	assembly.Locales = append(assembly.Locales, Locales)
}

type computerSettingsHost interface{ BindComputerSettings() settings.WindowBinding }
