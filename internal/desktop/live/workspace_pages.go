package live

import (
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

// showItem shows a file or a diff of the project: on the rail's page of its
// kind the page shows it, else its panel tab does. The panel is synced first
// off the conversation page, where it may still hold another project's tabs.
func (p *workspaceProject) showItem(kind panel.Kind, id string) {
	app := p.runtime.App
	if kind == panel.Files {
		p.panel.Files.Show(id)
	} else {
		p.panel.Git.Show(id)
	}
	if kind == panel.Files && app.Page == shell.FilesPage || kind == panel.Changes && app.Page == shell.GitPage {
		return
	}
	if app.Page != shell.ConversationPage {
		app.Panel.Sync(p.panel)
	}
	app.PanelOpen = true
	app.Panel.Show(panel.Tab{Kind: kind, ID: id})
}
