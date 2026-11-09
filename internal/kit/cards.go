package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/theme"
)

// The resource cards of settings/resource_card.rs: plugins, skills, market
// entries and teams are this card, with their own tile, text and actions.

// Grid lays cards out two to a row, 12 apart.
func Grid(c *ui.Context, cards func()) ui.Element {
	return ui.Grid(c).Columns(2).Gap(12).FillWidth().Children(cards)
}

// ResourceCard is a card: a filled box with a 1 DIP border, radius 8,
// padded 16 by 12, holding a summary button (its tile, a semibold title
// and two lines of muted description at 75%) that opens it, and actions
// after it. actions gets whether the pointer is over the card, for the
// actions shown only then; open and actions may be nil.
func ResourceCard(c *ui.Context, key any, title, description string, tile func(), open func(), actions func(hovered bool)) {
	p := theme.Of(c)
	card := ui.Row(c.Key(key)).MinWidth(0).Gap(12).Padding(12, 16).Radius(p.RadiusLg).Border(1, p.Border).Background(p.Surface)
	hovered := card.Hovered() || card.FocusWithin()
	card.Children(func() {
		b := ui.ButtonBase(c.Key("summary")).Label(title).Grow(1).Shrink(1).MinWidth(0).Gap(12).Justify(ui.Start)
		if open != nil {
			b.Cursor(ui.CursorPointer).OnClick(open)
		}
		b.Children(func() {
			if tile != nil {
				ui.Row(c).Shrink(0).Children(tile)
			}
			ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(4).Children(func() {
				ui.Text(c, title).FontSize(14).FontWeight(600).SingleLine().TextColor(p.SurfaceForeground)
				ui.Text(c, description).FontSize(14).MaxLines(2).TextColor(p.MutedForeground.Alpha(0.75))
			})
		})
		if actions != nil {
			ui.Row(c).Shrink(0).Gap(8).Children(func() { actions(hovered) })
		}
	})
}

// CardMenu is a card's "…" menu (plugins_actions), shown while the card is
// hovered or the menu has the focus.
func CardMenu(c *ui.Context, label string, hovered bool, build func(m *Menu)) ui.Element {
	b := IconMenu(c, "menu", icons.Ellipsis, label, 28, false, nil, build)
	if !hovered && !b.FocusVisible() {
		b.Opacity(0)
	}
	return b
}
