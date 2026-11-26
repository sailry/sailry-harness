package desktop

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

type Option uint8

const CollectionsOnly Option = 1

// Register adds the full knowledge settings page, locales, and link handlers.
func Register(assembly *shell.Assembly) { Module()(assembly) }

func Module(options ...Option) shell.Module {
	page := PageWithEmbedding
	if slices.Contains(options, CollectionsOnly) {
		page = Page
	}
	return func(assembly *shell.Assembly) {
		assembly.SettingsPages = append(assembly.SettingsPages, page())
		assembly.Locales = append(assembly.Locales, Locales)
		registerLinks(assembly)
	}
}
