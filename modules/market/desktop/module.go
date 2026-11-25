package desktop

import "github.com/sailry/sailry-harness/internal/desktop/shell"

func Register(assembly *shell.Assembly) {
	assembly.WorkspacePages = append(assembly.WorkspacePages, Page())
	assembly.Locales = append(assembly.Locales, Locales)
	registerLinks(assembly)
}
