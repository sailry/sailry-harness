package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/panel"
)

// pageFeatures are the rail features that open an independent page
// (shell/rail/features.rs, pages.rs), and the page each opens.
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
	pr := a.Data.ProjectOf(a.Sidebar.Selected)
	if pr == nil {
		return nil
	}
	return a.Projects.Of(pr.ID)
}

// Navigate shows a page in the main column.
func (a *App) Navigate(p Page) {
	a.Page, a.shown = p, a.Sidebar.Selected
}

// ClosePage closes the independent page shown, back to the conversation.
// It reports whether there was one to close.
func (a *App) ClosePage() bool {
	switch a.Page {
	case ConversationPage, SettingsPage:
		return false
	}
	a.Page = ConversationPage
	return true
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

// featurePage is a rail feature's page: Files and Git as the project's
// resource, over the state the panel's tabs share; Activity and Market as
// the reference's preview shows them, unavailable.
func (a *App) featurePage(c *ui.Context) {
	if k := a.paged(); k != "" {
		panel.Page(c, k, a.project())
		return
	}
	f, _ := featureOf(a.feature())
	kit.Empty(c, f.Icon, i18n.T(c, f.Label), i18n.T(c, "unavailable"), nil).Grow(1).Justify(ui.Center)
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
	for id, p := range pageFeatures {
		if p == a.Page {
			return id
		}
	}
	return ""
}
