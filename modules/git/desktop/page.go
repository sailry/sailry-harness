package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func Page() shell.NativePage {
	return shell.NativePage{
		Page:     shell.GitPage,
		Feature:  shell.Feature{ID: "page:git", Icon: icons.Network, Label: "git", DefaultPinned: true},
		Resource: panel.Changes,
		Render: func(c *ui.Context, app *shell.App) {
			app.RenderResource(c, func(project *panel.Project) { render(c, project) })
		},
		RenderDetails: details,
		BindKeys:      func(c *ui.Context, project *panel.Project, scope ui.Element) { project.GitKeys(c, scope) },
	}
}

func render(c *ui.Context, project *panel.Project) {
	column := ui.Column(c.Key(panel.Changes)).Grow(1).MinWidth(0).MinHeight(0)
	width := column.LayoutBounds().W
	column.Children(func() {
		project.Git.Show(project.Git.Change())
		project.ActionDialog(c)
		project.DocumentTabs(c, panel.Changes)
		project.GitDiff(c, project.Git.Change(), width)
		project.GitDialogs(c)
	})
}

func details(c *ui.Context, app *shell.App, project *panel.Project) {
	reveal := app.RevealResourceFile()
	ui.Column(c.Key(panel.Changes)).Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		project.GitRepository(c, true, false, project.Git.Show, reveal)
	})
}
