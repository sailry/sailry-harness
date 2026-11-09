package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/onboarding"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// welcome is the welcome guide over the window (Shell::open_welcome). A
// provider chosen in it continues in Settings › Providers with the add
// editor on that choice; the editor's first Back reopens the guide
// (welcome_provider, launch).
func (a *App) welcome(c *ui.Context) {
	onboarding.View(c, &a.Welcome, func() {
		settings.ProviderPicker(c, a.Settings, func() {
			a.Welcome.Open, a.Welcome.Seen = false, true
			a.Navigate(SettingsPage)
			a.Settings.Open(settings.Providers)
		}, func() { a.Welcome.Open = true })
	})
}
