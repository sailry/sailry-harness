package desktop

import "github.com/sailry/sailry-harness/internal/desktop/shell"

func Register(assembly *shell.Assembly) {
	page := Page()
	page.Feature.Label = pageLabel
	assembly.WorkspacePages = append(assembly.WorkspacePages, page)
	assembly.Locales = append(assembly.Locales, Locales)
}
