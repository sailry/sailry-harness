package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
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
	if !a.sampled() {
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
//
// The sidebar also shows beside a host and a project page, where a click on
// the selected session's row, or New session reusing the unsent chat, leaves
// the selection as it is: the sidebar marks those, so they go back too.
func (a *App) followSelection() {
	opened := a.Sidebar.TakeShown()
	if a.Sidebar.TakeDraft() {
		// New session: the draft's editor takes the focus, also when the
		// page already shows that draft.
		a.Composer(a.Sidebar.Selected).Focus()
	}
	switch a.Page {
	case ConversationPage, SettingsPage:
		return
	}
	if opened || a.Sidebar.Selected != a.shown {
		a.Page = ConversationPage
	}
}

// extensionDetails reports whether the right column shows an extension's
// details: over the settings or the market.
func (a *App) extensionDetails() bool {
	return a.Settings.Details != nil && (a.Page == SettingsPage || a.Page == MarketPage)
}

// featurePage is a rail feature's page: Files and Git as the project's
// resource, over the state the panel's tabs share, in the preview and live
// alike; Activity and Market as the reference's preview shows them,
// unavailable.
func (a *App) featurePage(c *ui.Context) {
	if a.Page == PluginPage && a.Hooks.PluginPage != nil {
		a.Hooks.PluginPage(c, a.pluginPage)
		return
	}
	if k := a.paged(); k != "" {
		pr := a.project()
		switch {
		case pr != nil:
			panel.Page(c, k, pr, func() { a.Navigate(FilesPage) })
		case a.Data.ProjectOf(a.Sidebar.Selected) != nil:
			// The project is known and its worktree is not: its host is away.
			a.unavailable(c)
		default:
			a.projectPrompt(c, k)
		}
		return
	}
	f, _ := a.lookupFeature(a.feature())
	kit.Result(c, kit.ResultProps{Icon: f.Icon, Title: i18n.T(c, f.Label), SubTitle: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
}

// projectPrompt fills a Files or Git page without a project: the resource's
// empty state and New project, which opens the project editor. The page has
// no sidebar to build the editor, so it builds the sidebar's dialogs.
func (a *App) projectPrompt(c *ui.Context, k panel.Kind) {
	kit.Result(c, kit.ResultProps{Icon: panel.ResourceOf(k).Icon, Title: i18n.T(c, "resource_project_empty"), Extra: func() {
		label := i18n.T(c, "project_new")
		kit.Primary(c.Key("new-project"), label, false).Children(func() {
			ui.Icon(c, icons.Plus).FontSize(16)
			ui.Text(c, label).FontSize(14)
		}).OnClick(func() { a.Sidebar.OpenEditor(nil) })
	}}).Grow(1).Justify(ui.Center)
	sidebar.Dialogs(c, a.Sidebar)
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
