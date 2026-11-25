package desktop

import (
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func Register(assembly *shell.Assembly) {
	page := Page()
	page.BindWindow = settings.BindWindow(providerSettingsHost.BindProviderSettings)
	assembly.SettingsPages = append(assembly.SettingsPages, page)
	assembly.Locales = append(assembly.Locales, Locales)
	assembly.ProviderUI = func(s *settings.State) shell.ProviderUI { return For(s) }
}

type providerSettingsHost interface{ BindProviderSettings() settings.WindowBinding }
