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
	return el.Radius(p.RadiusLg).Material(glass.Glass{Style: glass.Regular, Tint: floatingTint(p)}).TextColor(p.PopoverForeground).
		Border(1, floatingEdge(p))
}

// floatingEdge is a step off the surface the glass shows: lighter in dark
// mode, darker in light. It is translucent over the glass, because the
// glass tones its backdrop (lighter in dark mode) and an edge derived from
// the popover color matches that tone and disappears.
func floatingEdge(p *theme.Palette) ui.Color {
	if p.Dark {
		return ui.RGBA(255, 255, 255, 0.07)
	}
	return ui.RGBA(0, 0, 0, 0.06)
}

// floatingTint darkens the glass in dark mode, whose own toning lifts
// what shows through too much there: the popover color at FloatingTint.
// Light mode keeps the glass's own tone.
func floatingTint(p *theme.Palette) ui.Color {
	if !p.Dark {
		return ui.Color{}
	}
	return p.Floating().Alpha(FloatingTint)
}

// FloatingTint is how strongly dark mode's glass takes the popover color,
// from 0 (the glass's own tone) to 1 (nearly opaque).
const FloatingTint = 0.55
