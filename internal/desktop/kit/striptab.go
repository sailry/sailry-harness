package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// StripTabHeight is the height of a strip's tab.
const StripTabHeight = 28

// StripTab is the button of a tab in a strip of closable tabs, the right
// panel's and a package page's alike: 28 DIPs high and at most width wide,
// the active one in tab_active. It reports whether the pointer is over it;
// StripTabBody fills it.
func StripTab(c *ui.Context, key any, label string, active bool, width float32) (ui.Element, bool) {
	p := theme.Of(c)
	b := ui.ButtonBase(c.Key(key)).Label(label).Height(StripTabHeight).MaxWidth(width).PaddingStart(8).PaddingEnd(4).
		Gap(6).Radius(p.Radius).Cursor(ui.CursorPointer).TextColor(p.MutedForeground)
	hovered := b.Hovered()
	switch {
	case active:
		b.Background(p.TabActive).TextColor(p.TabActiveText).AccessSelected(true).ScrollIntoView()
	case hovered:
		b.TextColor(p.Foreground)
	}
	return b, hovered
}

// StripTabBody is a strip tab's content: its icon, or the spinner while
// busy, the title, and its close button, which shows on hover or on the
// active tab. A nil close leaves the button out.
func StripTabBody(c *ui.Context, b ui.Element, icon *ui.SVG, label string, active, hovered, busy bool, closeLabel string, close func()) ui.Element {
	return b.Children(func() {
		if busy {
			Busy(c)
		} else if icon != nil {
			ui.Icon(c, icon).FontSize(14).Shrink(0)
		}
		ui.Text(c, label).FontSize(14).SingleLine().Shrink(1)
		if close == nil {
			return
		}
		x := IconButton(c.Key("close"), icons.Close, closeLabel, 20, false).OnClick(close)
		if !hovered && !active && !x.FocusVisible() {
			x.Opacity(0)
		}
	})
}
