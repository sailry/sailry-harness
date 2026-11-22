package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func Page() shell.NativePage {
	return shell.NativePage{
		Page:     shell.FilesPage,
		Feature:  shell.Feature{ID: "page:files", Icon: icons.Folder, Label: "files", DefaultPinned: true},
		Usage:    protocol.FeatureUsePageFiles,
		Resource: panel.Files,
		Render: func(c *ui.Context, app *shell.App) {
			app.RenderResource(c, func(project *panel.Project) { Render(c, project) })
		},
		RenderDetails: func(c *ui.Context, _ *shell.App, project *panel.Project) { Details(c, project) },
	}
}

func Render(c *ui.Context, project *panel.Project) {
	ui.Column(c.Key(panel.Files)).Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		project.Files.Show(project.Files.File())
		project.ActionDialog(c)
		project.DocumentTabs(c, panel.Files)
		project.FileEditor(c, project.Files.File(), true)
	})
}

func Details(c *ui.Context, project *panel.Project) {
	ui.Column(c.Key(panel.Files)).Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		project.FileTree(c, true, project.Files.Show)
	})
}
