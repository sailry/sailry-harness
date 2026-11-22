package desktop

import (
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func Register(assembly *shell.Assembly) {
	assembly.SettingsPages = append(assembly.SettingsPages, Page())
	assembly.Locales = append(assembly.Locales, Locales)
	assembly.ProviderUI = func(s *settings.State) shell.ProviderUI { return For(s) }
}
