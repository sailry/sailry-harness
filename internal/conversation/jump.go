package conversation

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// jump is Kit MessageScroller's button (crates/component/message_scroller.rs):
// it shows while the transcript is scrolled off its end, with no threshold,
// as any scroll up stops the list following its end. It fades in over
// 200 ms with an ease-out cubic as it rises from 8 to 16 DIPs above the
// bottom, and a click shows the end at once, which follows it again.
//
// The fade is Element.AnimateWith, which runs at the display's rate: MyGo
// has no frame-rate parameter for element transitions (only
// Painter.AnimationFrameRate for drawings). Reduce Motion makes it instant.
func jump(c *ui.Context, l *ui.ListState, n int) {
	p := theme.Of(c)
	first, last := l.Visible()
	visible := n > 0 && last >= first && !l.AtEnd() // laid out, and off its end
	target := float32(0)
	if visible {
		target = 1
	}
	wrap := ui.Row(c.Key("jump")).Absolute().Left(0).Right(0).Justify(ui.Center)
	v := target
	if !c.Preferences().ReduceMotion {
		v = wrap.AnimateWith("jump", target, kit.Fast, kit.EaseOutCubic)
	}
	if v <= 0 {
		return // gone: nothing to hit or announce
	}
	label := i18n.T(c, "turn_latest")
	wrap.Bottom(8 + 8*v).Opacity(v).Children(func() {
		b := ui.ButtonBase(c.Key("latest")).Label(label).Tooltip(label).Size(32, 32).Radius(16).
			Border(1, p.Border).Background(p.Background).TextColor(p.Foreground).Center().
			Cursor(ui.CursorPointer).Disabled(!visible)
		if visible && b.Hovered() {
			b.Background(p.Hover)
		}
		b.Children(func() { ui.Icon(c, icons.ArrowDown).FontSize(16) }).OnClick(l.ScrollToEnd)
	})
}
