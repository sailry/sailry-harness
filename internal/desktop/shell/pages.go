package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
)

// pageFeatures are the rail features that open an independent page, and the
// page each opens.
var pageFeatures = map[string]Page{
	"page:conversation":     ConversationPage,
	"page:activity":         ActivityPage,
	"page:files":            FilesPage,
	"page:git":              GitPage,
	"page:settings_plugins": MarketPage,
}

// project returns the state of the selected conversation's project, nil
// when it has none.
func (a *App) project() *panel.Project {
	if !a.Preview {
		if a.Hooks.WorkspaceProject != nil {
			return a.Hooks.WorkspaceProject()
		}
		return nil
	}
	pr := a.Data.ProjectOf(a.Sidebar.Selected)
	if pr == nil {
		return nil
	}
	return a.Projects.Of(pr.ID)
}

// Navigate shows a page in the main column.
func (a *App) Navigate(p Page) {
	if p != a.Page {
		a.Settings.CloseDetails()
	}
	a.Page, a.shown = p, a.Sidebar.Selected
}

// ClosePage closes the independent page shown, back to the conversation.
// It reports whether there was one to close.
func (a *App) ClosePage() bool {
	if !a.closable() {
		return false
	}
	a.Page = ConversationPage
	return true
}

// closable reports whether the page shown is an independent page, which
// ⌘W closes.
func (a *App) closable() bool {
	return a.Page != ConversationPage && a.Page != SettingsPage
}

// followSelection goes back to the conversation when the sidebar selects
// another one over a project page, as selecting a session does in the
// reference.
func (a *App) followSelection() {
	switch a.Page {
	case ConversationPage, SettingsPage:
		return
	}
	if a.Sidebar.Selected != a.shown {
		a.Page = ConversationPage
	}
}

// extensionDetails reports whether the right column shows an extension's
// details: over the settings or the market.
func (a *App) extensionDetails() bool {
	return a.Settings.Details != nil && (a.Page == SettingsPage || a.Page == MarketPage)
}

// featurePage is a rail feature's page: Files and Git as the project's
// resource, over the state the panel's tabs share; Activity and Market as
// the reference's preview shows them, unavailable.
func (a *App) featurePage(c *ui.Context) {
	if a.Page == PluginPage && a.Hooks.PluginPage != nil {
		a.Hooks.PluginPage(c, a.pluginPage)
		return
	}
	if !a.Preview && a.Data.ProjectOf(a.Sidebar.Selected) != nil {
		a.unavailable(c)
		return
	}
	if k := a.paged(); k != "" {
		panel.Page(c, k, a.project(), func() { a.Navigate(FilesPage) })
		return
	}
	f, _ := a.lookupFeature(a.feature())
	kit.Result(c, kit.ResultProps{Icon: f.Icon, Title: i18n.T(c, f.Label), SubTitle: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
}

// paged is the project resource the page shows, "" for none.
func (a *App) paged() panel.Kind {
	switch a.Page {
	case FilesPage:
		return panel.Files
	case GitPage:
		return panel.Changes
	}
	return ""
}

// feature is the rail feature of the page shown, "" for none.
func (a *App) feature() string {
	if a.Page == PluginPage {
		return a.pluginPage
	}
	for id, p := range pageFeatures {
		if p == a.Page {
			return id
		}
	}
	return ""
}
