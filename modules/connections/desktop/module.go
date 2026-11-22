package desktop

import "github.com/sailry/sailry-harness/internal/desktop/shell"

func Register(assembly *shell.Assembly) {
	assembly.SettingsPages = append(assembly.SettingsPages, Page())
	assembly.Locales = append(assembly.Locales, Locales)
}
