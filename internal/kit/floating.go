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
// scene) at the large radius, in the popover's text color. The glass
// draws its own frost, toning and rim, so the panel sets no background,
// border or shadow. It is the one place their surface is set.
func Floating(c *ui.Context, el ui.Element) ui.Element {
	p := theme.Of(c)
	return el.Radius(p.RadiusLg).Material(glass.Glass{Style: glass.Regular}).TextColor(p.PopoverForeground)
}
