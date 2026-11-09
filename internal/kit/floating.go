package kit

import (
	"github.com/egoist/mygo/plugins/glass"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Floating paints a floating panel (menus, selects, popovers, dialogs,
// toasts, tooltips and hover cards) in the one look they share: MyGo's
// Regular glass (its shader, which samples the elements painted under
// the panel in the same frame: overlays paint above the content in one
// scene) at the large radius, in the popover's text color, with a 1 DIP
// edge a step off the popover color. The glass draws its own frost and
// toning, so the panel sets no background or shadow. It is the one place
// their surface is set.
func Floating(c *ui.Context, el ui.Element) ui.Element {
	p := theme.Of(c)
	return el.Radius(p.RadiusLg).Material(glass.Glass{Style: glass.Regular}).TextColor(p.PopoverForeground).
		Border(1, floatingEdge(p))
}

// floatingEdge is a step off the popover color: lighter in dark mode,
// darker in light, at partial alpha, so the edge reads soft.
func floatingEdge(p *theme.Palette) ui.Color {
	toward := ui.RGB(0, 0, 0)
	if p.Dark {
		toward = ui.RGB(255, 255, 255)
	}
	return p.Floating().Mix(toward, 0.07).Alpha(0.8)
}
