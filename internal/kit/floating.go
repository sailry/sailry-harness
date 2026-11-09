package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Floating paints a floating panel (menus, selects, popovers, dialogs,
// toasts, tooltips and hover cards) in the one look they share: the
// palette's popover color, the large radius, the border and MyGo's
// popover shadow. It is the one place their surface is set, so a
// material (frosted glass) changes them all at once.
func Floating(c *ui.Context, el ui.Element) ui.Element {
	p := theme.Of(c)
	return el.Radius(p.RadiusLg).Background(p.Floating()).TextColor(p.PopoverForeground).Border(1, p.Border).
		Shadow(0, 6, 20, 0, p.Backdrop().Alpha(0.18))
}
