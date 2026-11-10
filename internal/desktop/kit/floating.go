package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Floating paints a floating panel (menus, selects, popovers, dialogs,
// toasts, tooltips and hover cards) in the one look they share: frost, a
// backdrop blur of FloatingBlur DIP (it samples the elements painted under
// the panel in the same frame: overlays paint above the content in one
// scene) over the opaque popover color, toned by that color again at
// FloatingTint, at the large radius, in the popover's text color, with a
// 1 DIP edge a step off that surface. No background or shadow. It is the
// one place their surface is set.
func Floating(c *ui.Context, el ui.Element) ui.Element {
	p := theme.Of(c)
	surface := p.Floating().Over(Opaque(p.Background))
	if p.Dark {
		surface = surface.Mix(Opaque(p.Foreground), FloatingLift)
	}
	m := frost{blur: FloatingBlur, base: surface, tint: surface.Alpha(FloatingTint)}
	return m.on(c, el.Radius(p.RadiusLg)).TextColor(p.PopoverForeground).Border(1, floatingEdge(p))
}

// floatingEdge is a step off the frost: lighter in dark mode, darker in
// light. It is translucent, so it follows what shows through.
func floatingEdge(p *theme.Palette) ui.Color {
	if p.Dark {
		return ui.RGBA(255, 255, 255, 0.04)
	}
	return ui.RGBA(0, 0, 0, 0.06)
}

const (
	// FloatingBlur is the frost's blur radius in DIP.
	FloatingBlur = 18
	// FloatingTint is how strongly the popover color covers the blurred
	// backdrop, from 0 (the backdrop alone) to 1 (opaque).
	FloatingTint = 0.4
	// FloatingLift is how far dark mode's surface moves from the popover
	// color toward the foreground, so the panels read lighter than the page.
	FloatingLift = 0.1
)
