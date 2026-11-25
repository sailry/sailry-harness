package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

func registerLinks(host shell.PanelResourceHost) {
	app, navigation := host.App, host.BrowserNavigation
	open := func(c *ui.Context, target native.LinkTarget, _ native.LinkActions) error {
		if target.External || navigation.Available == nil || navigation.Navigate == nil || !navigation.Available() {
			c.OpenURL(target.URL)
			return nil
		}
		app.RevealConversationPanel()
		tab := app.Panel.AddBrowser(true)
		if tab.ID == "" || !navigation.Navigate(tab.ID, target.URL) {
			return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "browser tab is unavailable"}
		}
		return nil
	}
	for _, scheme := range []string{"http", "https"} {
		if _, err := app.RegisterLinkHandler(native.LinkRoute{Kind: native.LinkScheme, Name: scheme}, open); err != nil {
			host.Report(err)
		}
	}
}
