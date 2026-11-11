package kit

import (
	"hash/fnv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TileSize is the leading tile slot of a media row.
const TileSize = 36

// MediaRow is the settings lists' shared record: a 36 DIP leading tile, then
// a title of the body size in medium weight with an optional badge, and a
// one-line description in muted text at 70%, 2 DIPs below it. Either function
// may be nil.
func MediaRow(c *ui.Context, title, description string, tile, badge func()) ui.Element {
	var line func()
	if description != "" {
		line = func() {
			ui.Text(c, description).FontSize(14).LineHeight(1.3).SingleLine().TextColor(theme.Of(c).Description())
		}
	}
	return MediaRecord(c, title, tile, badge, line)
}

// MediaRecord is MediaRow with the description line built by the caller, for
// a line with parts of its own, such as one that has a tooltip.
func MediaRecord(c *ui.Context, title string, tile, badge, description func()) ui.Element {
	p := theme.Of(c)
	return ui.Row(c).Grow(1).Shrink(1).MinWidth(0).Gap(12).Children(func() {
		if tile != nil {
			ui.Row(c).Size(TileSize, TileSize).Shrink(0).Center().Children(tile)
		}
		ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(TitleGap).Children(func() {
			ui.Row(c).MinWidth(0).Gap(8).Children(func() {
				ui.Text(c, title).FontSize(16).FontWeight(500).LineHeight(1.3).SingleLine().Shrink(1).TextColor(p.Foreground)
				if badge != nil {
					ui.Row(c).Shrink(0).Children(badge)
				}
			})
			if description != nil {
				description()
			}
		})
	})
}

// Tile is the emblem of a record: a gradient chosen by the name's FNV hash,
// radius 11, with a white glyph 5/9 of its size, or the name's initial.
func Tile(c *ui.Context, name string, glyph *ui.SVG) {
	h := fnv.New32a()
	h.Write([]byte(name))
	tint := theme.Tints[h.Sum32()%uint32(len(theme.Tints))]
	ui.Row(c).Size(TileSize, TileSize).Radius(11).Center().
		Gradient(tint[0], tint[1], 180).TextColor(theme.OnTint).Children(func() {
		if glyph != nil {
			ui.Icon(c, glyph).FontSize(TileSize * 5 / 9)
			return
		}
		// A name not given yet has no initial: the tile shows its tint alone.
		if r := []rune(name); len(r) > 0 {
			ui.Text(c, string(r[:1])).FontSize(TileSize * 5 / 9).FontWeight(600)
		}
	})
}
