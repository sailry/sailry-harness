package testassembly

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func RegisterFilesFixture(assembly *shell.Assembly) {
	RegisterFilePanelFixture(assembly)
	assembly.WorkspacePages = append(assembly.WorkspacePages, FileResourcePage())
}

func RegisterFilePanelFixture(assembly *shell.Assembly) {
	assembly.RegisterPanelResource(panel.Resource{
		Kind: panel.Files, Label: "files", Icon: icons.Folder, Order: 30, Mods: ui.Cmd, Key: ui.KeyP,
		Render: func(c *ui.Context, state *panel.State, tab panel.Tab) {
			panel.RenderFilePanelResource(c, state, tab, fileResourceChoices)
		},
	})
}

func FileResourcePage() shell.NativePage {
	return shell.NativePage{
		Page: shell.FilesPage, Resource: panel.Files, Usage: protocol.FeatureUsePageFiles,
		Feature: shell.Feature{ID: string(shell.FilesPage), Icon: icons.Folder, Label: "files", DefaultPinned: true},
		Render: func(c *ui.Context, app *shell.App) {
			app.RenderResource(c, func(project *panel.Project) { FileDocument(c, project) })
		},
		RenderDetails: func(c *ui.Context, _ *shell.App, project *panel.Project) {
			ui.Column(c.Key("file-resource-fixture")).Grow(1).MinWidth(0).MinHeight(0).Children(func() {
				fileResourceChoices(project, c, true, project.Files.Show)
			})
		},
	}
}

func FileDocument(c *ui.Context, project *panel.Project) {
	if project.Actions == nil {
		placeholder := ui.Box(c.Key("initial-fixture-document")).Size(0, 0)
		ui.Local(placeholder, project, func() bool {
			if len(project.Files.Open()) == 0 && project.Data.Files != nil {
				path := project.Files.File()
				if path == "" {
					path = "sailry-go/README.md"
				}
				project.Files.Show(path)
			}
			return true
		})
	}
	project.DocumentTabs(c, panel.Files)
	project.FileEditor(c, project.Files.File(), true)
}

func fileResourceChoices(project *panel.Project, c *ui.Context, _ bool, open func(string)) {
	if project.Actions != nil || project.Data.Files == nil {
		return
	}
	for _, name := range []string{"README.md", "go.mod"} {
		ui.Button(c.Key(name), "Open fixture "+name).OnClick(func() { open("sailry-go/" + name) })
	}
}
