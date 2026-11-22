package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.OCR,
		Group:   "settings_group_tools",
		Icon:    icons.ALargeSmall,
		Order:   700,
		Render: func(c *ui.Context, s *settings.State) {
			settings.LocalModelLibrary(c, s.Model, &s.OCR, "runtime_ocr", "runtime_ocr_model", "runtime_ocr_enable", "search_driver_auto")
		},
	}
}
