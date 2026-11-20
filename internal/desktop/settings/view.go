package settings

import (
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
)

// Conversation text sizes in DIPs, a pixel a step.
const (
	MinTextSize = 12
	MaxTextSize = 18
)

// view is the View page: the interface size, which zooms the window's
// content but not its columns, and the conversation's text size within it.
func view(c *ui.Context, m *Model) {
	kit.Card(c, func() {
		levels := make([]string, len(preferences.ViewZoomLevels))
		for i, level := range preferences.ViewZoomLevels {
			levels[i] = strconv.Itoa(int(level)) + "%"
		}
		kit.SegmentedRow(c, "view-size", i18n.T(c, "settings_view_size"), "", &m.ViewZoom, levels...)
		title := i18n.T(c, "settings_message_text_size")
		kit.WideRow(c, "text-size", title, "", func() {
			ui.StepSlider(c.Key("slider"), &m.TextSize, MinTextSize, MaxTextSize, 1).Label(title).Grow(1).Shrink(1).MinWidth(0)
		})
	})
}
