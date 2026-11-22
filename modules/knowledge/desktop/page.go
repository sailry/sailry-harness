package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// Page provides collection management on its own.
func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Knowledge,
		Group:   "settings_group_ai",
		Icon:    icons.BookOpen,
		Order:   200,
		Render:  collections,
	}
}

// PageWithEmbedding adds the local embedding model library above collections
// on the same settings route.
func PageWithEmbedding() settings.NativePage {
	page := Page()
	page.Render = func(c *ui.Context, s *settings.State) {
		settings.LocalModelLibrary(c, s.Model, &s.Knowledge, "knowledge_settings", "knowledge_default_model", "", "knowledge_keyword_only")
		collections(c, s)
	}
	return page
}
