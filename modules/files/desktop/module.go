package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func Register(assembly *shell.Assembly) {
	RegisterPanel(assembly)
	page := Page()
	page.Feature.Label = pageLabel
	assembly.WorkspacePages = append(assembly.WorkspacePages, page)
}

// RegisterPanel installs the conversation file browser without the standalone page.
func RegisterPanel(assembly *shell.Assembly) {
	page := Page()
	assembly.RegisterPanelResource(panel.Resource{
		Kind: page.Resource, Label: page.Feature.Label, Icon: page.Feature.Icon, Order: 30,
		Mods: ui.Cmd, Key: ui.KeyP, Render: panel.RenderProjectResource,
	})
	assembly.Locales = append(assembly.Locales, Locales)
}
