package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/onboarding"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// ProviderUI is the model channels page's presentation for this window.
type ProviderUI interface {
	Picker(c *ui.Context, chosen, back func())
	CloseProviderEditor(nextNode string)
	LoginDialog(*ui.Context)
	ShowProviderAuthorization(node, id string, view *settings.Authorization)
	ProviderAuthorization() *settings.Authorization
	WarnProviderAuthorization(title, text string, proceed, cancel func())
}

func (a *App) hasWelcome() bool {
	return a.ProviderUI != nil && a.Settings.HasSection(settings.Providers)
}

// welcome is the welcome guide over the window (Shell::open_welcome). A
// provider chosen in it continues in Settings › Providers with the add
// editor on that choice; the editor's first Back reopens the guide
// (welcome_provider, launch).
func (a *App) welcome(c *ui.Context) {
	if !a.hasWelcome() {
		a.Welcome.Open = false
		return
	}
	onboarding.View(c, &a.Welcome, a.Settings.ProductName, func() {
		a.ProviderUI.Picker(c, func() {
			a.Welcome.Open, a.Welcome.Seen = false, true
			a.Navigate(SettingsPage)
			a.Settings.Open(settings.Providers)
		}, func() { a.Welcome.Open = true })
	})
}
