package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/theme"
)

// SplitProps set a SplitButton's look: the main action's icon, whether
// it is disabled, 28 high instead of 32 (Kit's small), and filling its
// row's width (Kit's form trigger).
type SplitProps struct {
	Icon     *ui.SVG
	Disabled bool
	Small    bool
	Fill     bool
}

// SplitButton is Kit's DropdownButton (component/button/dropdown_button.rs)
// as Sailry's Menu with a primary item builds it: the main action, a
// secondary button labeled label, joined to a caret that opens a Menu of
// the other actions, which build fills. id names the menu as
// MenuButton's; more names the caret for its tooltip and assistive
// technology. It returns the main button, whose click the caller reads.
// Own, on ButtonBase and MenuButton: MyGo has no split button.
func SplitButton(c *ui.Context, id, label, more string, o SplitProps, build func(m *Menu)) ui.Element {
	p := theme.Of(c)
	h, text := float32(32), float32(14)
	if o.Small {
		h, text = 28, 13
	}
	look := func(b ui.Element) ui.Element {
		b.Height(h).Background(p.ButtonSecondary).TextColor(p.ButtonSecondaryForeground).Cursor(ui.CursorPointer)
		switch {
		case b.Pressed():
			b.Background(p.ButtonSecondaryActive)
		case b.Hovered():
			b.Background(p.ButtonSecondaryHover)
		}
		return b
	}
	var main ui.Element
	row := ui.Row(c.Key(id)).Height(h).Shrink(0).Radius(p.Radius).Border(1, p.Border).Clip()
	if o.Fill {
		row.FillWidth()
	}
	row.Children(func() {
		main = look(ui.ButtonBase(c.Key("main")).Label(label).Disabled(o.Disabled)).Grow(1).Center().Gap(6).PaddingX(12)
		if o.Disabled {
			main.Opacity(0.5)
		}
		main.Children(func() {
			if o.Icon != nil {
				ui.Icon(c, o.Icon).FontSize(text)
			}
			ui.Text(c, label).FontSize(text).SingleLine()
		})
		ui.Box(c).Width(1).FillHeight().Background(p.Border).Shrink(0)
		MenuButton(c.Key("more"), id, func(b ui.Element) {
			Tooltip(c, look(b.Label(more)).Width(h-4).Center(), more)
			b.Children(func() { ui.Icon(c, icons.ChevronDown).FontSize(text) })
		}, build, MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
	})
	return main
}
