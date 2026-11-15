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
// the active one in tab_active and a hovered one in accent. Its padding is
// even, so a divider stands midway between two tabs' text. It reports
// whether the pointer is over it; StripTabBody fills it.
func StripTab(c *ui.Context, key any, label string, active bool, width float32) (ui.Element, bool) {
	p := theme.Of(c)
	b := ui.ButtonBase(c.Key(key)).Label(label).Height(StripTabHeight).MaxWidth(width).
		PaddingX(8).Gap(6).Justify(ui.Start).Radius(p.Radius).Cursor(ui.CursorPointer).TextColor(p.MutedForeground)
	hovered := b.Hovered()
	switch {
	case active:
		b.Background(p.TabActive).TextColor(p.TabActiveText).AccessSelected(true).ScrollIntoView()
	case hovered:
		b.Background(p.Hover).TextColor(p.Foreground)
	}
	return b, hovered
}

// StripTabBody is a strip tab's content: its icon, or the spinner while
// busy, the title, and its close button (StripTabBodyWith).
func StripTabBody(c *ui.Context, b ui.Element, icon *ui.SVG, label string, active, hovered, busy bool, closeLabel string, close func()) ui.Element {
	var lead func()
	switch {
	case busy:
		lead = func() { Busy(c) }
	case icon != nil:
		lead = func() { ui.Icon(c, icon).FontSize(14).Shrink(0) }
	}
	return StripTabBodyWith(c, b, lead, label, active, hovered, closeLabel, close)
}

// StripTabBodyWith is a strip tab's content: its lead, as an icon or a
// session's status mark, the title, and its close button. The button takes
// no room, so the strip never moves: while the pointer is over the tab or
// the button has the keyboard focus, it floats at the tab's end and the tab
// keeps the width it had. The lead then gives way, the title moving into
// its place, so the button covers none of the title; a tab without a lead
// cuts its title short instead. A nil lead has none, and a nil close no
// button.
func StripTabBodyWith(c *ui.Context, b ui.Element, lead func(), label string, active, hovered bool, closeLabel string, close func()) ui.Element {
	return b.Children(func() {
		var x ui.Element
		shown := false
		if close != nil {
			x = IconButton(c.Key("close"), icons.Close, closeLabel, 20, false).OnClick(close).
				Absolute().Right(4).Top((StripTabHeight - 20) / 2)
			shown = hovered || x.FocusVisible()
			if !shown {
				x.Opacity(0)
			} else if w := b.LayoutBounds().W; w > 0 {
				b.Width(w)
			}
		}
		if lead != nil && !shown {
			ui.Row(c.Key("lead")).Shrink(0).AlignItems(ui.Center).Children(lead)
		}
		text := ui.Text(c, label).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
		if shown && lead == nil {
			text.PaddingEnd(20)
		}
	})
}

// StripDivider is the short line between two strip tabs, as high as the
// title band's dividers. It is the secondary text dimmed: the border tokens
// vanish between tabs on a translucent band.
func StripDivider(c *ui.Context, key any) {
	ui.Box(c.Key(key)).Size(1, 16).Radius(0.5).Shrink(0).Background(theme.Of(c).MutedForeground.Alpha(0.2)).PassThrough()
}

// StripGap is the room on each side of a strip's divider.
const StripGap = 8
